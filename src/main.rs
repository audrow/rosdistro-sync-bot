use dotenv::dotenv;
use std::{collections::HashMap, env};
use tokio;

use octocrab::{self, models, params};

use log::{debug, info};
use reqwest;
use serde::{Deserialize, Serialize};

const SYNC_HOLD_LABEL: &str = "in_sync_hold";

#[derive(Debug, Deserialize, Serialize)]
struct SyncStatus {
    distro: String,
    in_sync_hold: bool,
}

type DistroToSyncStatus = HashMap<String, bool>;

fn sync_statuses_to_hashmap(sync_statuses: &Vec<SyncStatus>) -> DistroToSyncStatus {
    let mut distro_map = HashMap::<String, bool>::new();
    for sync_status in sync_statuses {
        distro_map.insert(sync_status.distro.clone(), sync_status.in_sync_hold);
    }
    distro_map
}

async fn run(
    repo_org: String,
    repo_name: String,
    personal_access_token: String,
    distro_to_sync_status: DistroToSyncStatus,
) {
    let octocrab = octocrab::Octocrab::builder()
        .personal_token(personal_access_token)
        .build()
        .expect("Creating octocrab instance failed");
    let issue_handler = octocrab.issues(repo_org, repo_name);

    let page = issue_handler
        .list()
        .state(params::State::Open)
        .per_page(100)
        .send()
        .await
        .expect("Getting page of issues failed");

    let issues = octocrab
        .all_pages::<models::issues::Issue>(page)
        .await
        .expect("Getting issues failed");

    let distros = distro_to_sync_status.keys().collect::<Vec<_>>();

    for issue in issues {
        let mut labels: Vec<_> = issue
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect();

        let distro = distros
            .iter()
            .find(|distro| labels.contains(distro))
            .expect("distro not found in labels");
        let is_in_sync = *distro_to_sync_status
            .get(&**distro)
            .expect("distro not found in distro_map");
        let is_labeled_as_in_sync_hold = labels.iter().any(|label| label == SYNC_HOLD_LABEL);

        if is_in_sync == is_labeled_as_in_sync_hold {
            debug!(
                "Issue {} is labeled correctly {} the '{}' label",
                issue.number,
                if is_in_sync { "with" } else { "without" },
                SYNC_HOLD_LABEL
            );
            continue; // labeled correctly do nothing
        }

        if is_in_sync && !is_labeled_as_in_sync_hold {
            info!(
                "Adding '{}' label to issue #{}: {}",
                SYNC_HOLD_LABEL, issue.number, issue.title
            );
            labels.push(String::from(SYNC_HOLD_LABEL));
        } else if !is_in_sync && is_labeled_as_in_sync_hold {
            info!(
                "Removing '{}' label from issue #{}: {}",
                SYNC_HOLD_LABEL, issue.number, issue.title
            );
            labels.remove(
                labels
                    .iter()
                    .position(|label| label == SYNC_HOLD_LABEL)
                    .expect("SYNC_HOLD_LABEL not found in labels"),
            );
        } else {
            unreachable!("This should never happen");
        }

        issue_handler
            .update(issue.number)
            .labels(&labels)
            .send()
            .await
            .expect("Updating issue failed");
        debug!("Updated issue #{}: {:?}", issue.number, issue.title);
    }
}

async fn get_rosdisto_to_sync_status(url: String) -> DistroToSyncStatus {
    let response = reqwest::get(&url)
        .await
        .expect("request to get sync status YAML failed");
    let contents = response
        .text()
        .await
        .expect("request didn't return valid UTF-8");
    debug!("Contents from {url}: {contents:?}");
    let sync_statuses: Vec<SyncStatus> =
        serde_yaml::from_str(&contents).expect("Unable to parse sync status YAML");
    sync_statuses_to_hashmap(&sync_statuses)
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let repo_org = env::var("GITHUB_REPO_ORG").expect("GITHUB_REPO_ORG not defined");
    let repo_name = env::var("GITHUB_REPO_NAME").expect("GITHUB_REPO_NAME not defined");
    let repo_branch_name =
        env::var("GITHUB_REPO_BRANCH_NAME").expect("GITHUB_REPO_BRANCH_NAME not defined");
    let repo_path_to_sync_status = env::var("GITHUB_REPO_PATH_TO_SYNC_STATUS")
        .expect("GITHUB_REPO_PATH_TO_SYNC_STATUS not defined");
    let personal_access_token =
        env::var("GITHUB_PERSONAL_ACCESS_TOKEN").expect("GITHUB_PERSONAL_ACCESS_TOKEN not defined");

    let url_to_file = format!("https://raw.githubusercontent.com/{repo_org}/{repo_name}/{repo_branch_name}/{repo_path_to_sync_status}");
    let distro_to_sync_status = get_rosdisto_to_sync_status(url_to_file).await;
    info!("distro_to_sync_status: {:?}", distro_to_sync_status);

    run(
        repo_org,
        repo_name,
        personal_access_token,
        distro_to_sync_status,
    )
    .await;
}
