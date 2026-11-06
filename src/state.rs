//! Typed Bitbucket fixture state and handles into its shared resources.

use std::collections::BTreeMap;

use ninot_common::state::StateHandle;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{
    BranchRef, BuildStatus, CloneLink, HtmlLink, MainBranch, Participant, PrAuthor, PrEndpoint,
    PrLinks, PullRequest, RepoLinks, RepoRef, Repository, User,
};

/// Version of the Bitbucket state snapshot format.
pub const STATE_VERSION: u32 = 1;

/// A shared handle to the state served by one Bitbucket fake.
pub type SharedState = StateHandle<State>;

/// State of one fake Bitbucket Cloud installation.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State {
    /// API base URL, filled by the running server and omitted from snapshots.
    #[serde(skip)]
    pub base_url: String,
    /// Optional credentials required by ordinary API requests.
    #[serde(default)]
    pub credentials: Option<(String, String)>,
    /// The authenticated user response.
    #[serde(default = "default_user")]
    pub user: User,
    /// Repositories, keyed by their full workspace and slug.
    #[serde(default)]
    pub repositories: BTreeMap<String, Repository>,
    /// Ordered provider fault rules.
    #[serde(default)]
    pub faults: Vec<FaultRule>,
    /// Request records are runtime observations, not part of a snapshot.
    #[serde(skip)]
    pub requests: Vec<RequestRecord>,
    /// Faults already applied, oldest first.
    #[serde(skip)]
    pub applied_faults: Vec<AppliedFault>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            credentials: None,
            user: default_user(),
            repositories: BTreeMap::new(),
            faults: Vec::new(),
            requests: Vec::new(),
            applied_faults: Vec::new(),
        }
    }
}

/// Returns the default Bitbucket account exposed by the fake.
fn default_user() -> User {
    User {
        username: "u".to_owned(),
        display_name: "U".to_owned(),
        account_id: "acc".to_owned(),
        uuid: "{u}".to_owned(),
        ..User::default()
    }
}

impl State {
    /// Registers a repository and returns a handle that mutates this shared state.
    /// Registers a repository fixture and returns its state-backed handle.
    #[allow(clippy::too_many_arguments)]
    pub fn add_repo(
        state: &SharedState,
        workspace: &str,
        slug: &str,
        full_name: Option<&str>,
        description: &str,
        default_branch: &str,
        is_private: bool,
        language: &str,
        size: u64,
        updated_on: &str,
        clone_https: Option<&str>,
    ) -> RepositoryHandle {
        let full_name = full_name
            .filter(|name| !name.is_empty())
            .map_or_else(|| format!("{workspace}/{slug}"), str::to_owned);
        let https = clone_https.filter(|url| !url.is_empty()).map_or_else(
            || format!("https://bitbucket.org/{full_name}.git"),
            str::to_owned,
        );
        let repository = Repository {
            slug: slug.to_owned(),
            full_name: full_name.clone(),
            description: description.to_owned(),
            mainbranch: MainBranch {
                name: default_branch.to_owned(),
                ..MainBranch::default()
            },
            is_private,
            language: language.to_owned(),
            size,
            updated_on: updated_on.to_owned(),
            links: RepoLinks {
                clone_links: vec![
                    CloneLink {
                        name: "https".to_owned(),
                        href: https,
                        ..CloneLink::default()
                    },
                    CloneLink {
                        name: "ssh".to_owned(),
                        href: format!("git@bitbucket.org:{full_name}.git"),
                        ..CloneLink::default()
                    },
                ],
                html: HtmlLink {
                    href: format!("https://bitbucket.org/{full_name}"),
                    ..HtmlLink::default()
                },
                ..RepoLinks::default()
            },
            ..Repository::default()
        };
        state.update(|state| {
            state.repositories.insert(full_name.clone(), repository);
        });
        RepositoryHandle {
            state: state.clone(),
            full_name,
        }
    }

    /// Replaces the user read model returned by `GET /user`.
    pub fn set_user(&mut self, username: &str, display_name: &str, account_id: &str, uuid: &str) {
        self.user = User {
            username: username.to_owned(),
            display_name: display_name.to_owned(),
            account_id: account_id.to_owned(),
            uuid: uuid.to_owned(),
            ..User::default()
        };
    }

    /// Returns the versioned snapshot accepted by `restore`.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the state cannot be represented as JSON.
    pub fn snapshot(&self) -> Result<Value, serde_json::Error> {
        let mut data = serde_json::to_value(self)?;
        let object = data.as_object_mut().ok_or_else(|| {
            serde::de::Error::custom("Bitbucket state must serialize to a JSON object")
        })?;
        let credentials = object.remove("credentials").unwrap_or(Value::Null);
        let faults = object
            .remove("faults")
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let repositories = object
            .remove("repositories")
            .unwrap_or_else(|| Value::Object(Default::default()));
        object.insert(
            "fixture".to_owned(),
            serde_json::json!({"credentials": credentials, "faults": faults}),
        );
        object.insert("repos".to_owned(), repositories);
        object.insert("version".to_owned(), Value::from(STATE_VERSION));
        object.insert("service".to_owned(), Value::from("bitbucket"));
        Ok(data)
    }

    /// Restores a versioned snapshot into this state, rejecting other versions.
    ///
    /// # Errors
    ///
    /// Returns an error when the service/version marker is wrong or the snapshot shape is invalid.
    pub fn restore(&mut self, snapshot: Value) -> Result<(), SnapshotError> {
        if snapshot.get("version").and_then(Value::as_u64) != Some(u64::from(STATE_VERSION))
            || snapshot.get("service").and_then(Value::as_str) != Some("bitbucket")
        {
            return Err(SnapshotError::Version);
        }
        let mut snapshot = snapshot;
        let Some(object) = snapshot.as_object_mut() else {
            return Err(SnapshotError::Shape(serde::de::Error::custom(
                "snapshot must be an object",
            )));
        };
        let fixture = object
            .remove("fixture")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        let credentials = fixture.get("credentials").cloned().unwrap_or(Value::Null);
        let faults = fixture
            .get("faults")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let repositories = object
            .remove("repos")
            .unwrap_or_else(|| Value::Object(Default::default()));
        object.insert("repositories".to_owned(), repositories);
        object.insert("credentials".to_owned(), credentials);
        object.insert("faults".to_owned(), faults);
        let mut restored: State = serde_json::from_value(snapshot).map_err(SnapshotError::Shape)?;
        restored.base_url.clone_from(&self.base_url);
        *self = restored;
        Ok(())
    }

    /// Removes every fault rule and fault observation.
    pub fn clear_faults(&mut self) {
        self.faults.clear();
        self.applied_faults.clear();
    }
}

/// Why a state snapshot could not be restored.
#[derive(Debug)]
pub enum SnapshotError {
    /// The service identifier or schema version was unsupported.
    Version,
    /// The snapshot did not match the current typed state.
    Shape(serde_json::Error),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Version => formatter.write_str("unsupported Bitbucket snapshot"),
            Self::Shape(error) => write!(formatter, "invalid Bitbucket snapshot: {error}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// A serializable fault rule selected through the fixture API.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FaultRule {
    /// Optional HTTP method match.
    pub method: Option<String>,
    /// Optional path substring match.
    pub path: Option<String>,
    /// One-based matching request occurrence.
    pub nth: Option<u32>,
    /// Fault action name.
    pub kind: String,
    /// Provider response or action settings.
    pub payload: Value,
    /// Remaining uses of the rule.
    pub remaining: u32,
    /// Number of matching requests already seen.
    #[serde(default, rename = "matched", alias = "seen")]
    pub matched: u32,
}

/// A request recorded by the fixture's ordinary HTTP server.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RequestRecord {
    /// HTTP method.
    pub method: String,
    /// Full request URL, including the query.
    pub url: String,
    /// Request headers, preserving their values.
    pub headers: BTreeMap<String, String>,
    /// Request body bytes.
    pub body: Vec<u8>,
}

/// A fault applied to an ordinary API request.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppliedFault {
    /// Request method.
    pub method: String,
    /// Request path.
    pub path: String,
    /// Fault kind.
    pub kind: String,
}

/// A direct handle to a repository persisted in a shared state.
#[derive(Clone)]
pub struct RepositoryHandle {
    /// Shared state that owns the repository.
    pub state: SharedState,
    /// Full repository key.
    pub full_name: String,
}

impl RepositoryHandle {
    /// Returns the current repository model, if it still exists.
    pub fn get(&self) -> Option<Repository> {
        self.state
            .read(|state| state.repositories.get(&self.full_name).cloned())
    }

    /// Returns handles to pull requests currently owned by this repository.
    pub fn pull_requests(&self) -> Vec<PullRequestHandle> {
        self.state.read(|state| {
            state
                .repositories
                .get(&self.full_name)
                .map(|repo| {
                    repo.pull_requests
                        .iter()
                        .map(|pull| PullRequestHandle {
                            state: self.state.clone(),
                            full_name: self.full_name.clone(),
                            id: pull.id,
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// Stores a file on the repository's main branch, replacing any previous content.
    ///
    /// # Errors
    ///
    /// Returns an error when the owning repository is no longer registered.
    pub fn add_file(&self, path: &str, content: Vec<u8>) -> Result<(), String> {
        self.state.update(|state| {
            let Some(repo) = state.repositories.get_mut(&self.full_name) else {
                return Err(format!("no repository {:?}", self.full_name));
            };
            repo.files
                .insert(path.trim_start_matches('/').to_owned(), content);
            Ok(())
        })
    }

    /// Creates a fixture pull request and returns its typed handle.
    ///
    /// # Errors
    ///
    /// Returns an error when the owning repository is no longer registered.
    #[allow(clippy::too_many_arguments)]
    pub fn create_pull_request(
        &self,
        id: u64,
        title: &str,
        description: &str,
        state: &str,
        draft: bool,
        source_branch: &str,
        target_branch: &str,
        source_repo: Option<&str>,
        target_repo: Option<&str>,
        author_display_name: &str,
        author_nickname: &str,
        author_account_id: &str,
        participants: Vec<Participant>,
        updated_on: &str,
        url: Option<&str>,
    ) -> Result<PullRequestHandle, String> {
        let full_name = self.full_name.clone();
        self.state.update(|state_data| {
            let Some(repo) = state_data.repositories.get_mut(&full_name) else {
                return Err(format!("no repository {full_name:?}"));
            };
            let pull = PullRequest {
                id,
                title: title.to_owned(),
                description: description.to_owned(),
                state: state.to_owned(),
                draft,
                source: endpoint(source_branch, source_repo.unwrap_or(&repo.full_name)),
                destination: endpoint(target_branch, target_repo.unwrap_or(&repo.full_name)),
                author: PrAuthor {
                    display_name: author_display_name.to_owned(),
                    nickname: author_nickname.to_owned(),
                    account_id: author_account_id.to_owned(),
                    ..PrAuthor::default()
                },
                participants,
                links: PrLinks {
                    html: HtmlLink {
                        href: url.filter(|href| !href.is_empty()).map_or_else(
                            || {
                                format!(
                                    "https://bitbucket.org/{}/pull-requests/{id}",
                                    repo.full_name
                                )
                            },
                            str::to_owned,
                        ),
                        ..HtmlLink::default()
                    },
                    ..PrLinks::default()
                },
                updated_on: updated_on.to_owned(),
                statuses: Vec::new(),
                ..PullRequest::default()
            };
            repo.pull_requests.push(pull);
            Ok(PullRequestHandle {
                state: self.state.clone(),
                full_name,
                id,
            })
        })
    }
}

/// Creates one source or destination reference in a pull request.
fn endpoint(branch: &str, repository: &str) -> PrEndpoint {
    PrEndpoint {
        branch: BranchRef {
            name: branch.to_owned(),
            ..BranchRef::default()
        },
        repository: RepoRef {
            full_name: repository.to_owned(),
            ..RepoRef::default()
        },
        ..PrEndpoint::default()
    }
}

/// A direct handle to a pull request persisted in a repository.
#[derive(Clone)]
pub struct PullRequestHandle {
    /// Shared state that owns the pull request.
    pub state: SharedState,
    /// Full repository key.
    pub full_name: String,
    /// Pull request number.
    pub id: u64,
}

impl PullRequestHandle {
    /// Returns the current pull request model, if it still exists.
    pub fn get(&self) -> Option<PullRequest> {
        self.state.read(|state| {
            state
                .repositories
                .get(&self.full_name)?
                .pull_requests
                .iter()
                .find(|pull| pull.id == self.id)
                .cloned()
        })
    }

    /// Sets the pull request title.
    ///
    /// # Errors
    ///
    /// Returns an error when the pull request no longer exists.
    pub fn set_title(&self, title: &str) -> Result<(), String> {
        self.update(|pull| pull.title = title.to_owned())
    }

    /// Sets the pull request description.
    ///
    /// # Errors
    ///
    /// Returns an error when the pull request no longer exists.
    pub fn set_description(&self, description: &str) -> Result<(), String> {
        self.update(|pull| pull.description = description.to_owned())
    }

    /// Adds a build status at the start of the newest-first status list.
    ///
    /// # Errors
    ///
    /// Returns an error when the pull request no longer exists.
    pub fn add_status(&self, status: BuildStatus) -> Result<(), String> {
        self.update(|pull| pull.statuses.insert(0, status))
    }

    /// Applies one mutation to the current pull request.
    fn update(&self, change: impl FnOnce(&mut PullRequest)) -> Result<(), String> {
        self.state.update(|state| {
            let Some(pull) = state
                .repositories
                .get_mut(&self.full_name)
                .and_then(|repo| {
                    repo.pull_requests
                        .iter_mut()
                        .find(|pull| pull.id == self.id)
                })
            else {
                return Err(format!("no pull request {}#{}", self.full_name, self.id));
            };
            change(pull);
            Ok(())
        })
    }
}

/// Builds a pull-request participant with convenient fixture defaults.
pub fn participant(account_id: &str, approved: bool, role: Option<&str>) -> Participant {
    Participant {
        account_id: account_id.to_owned(),
        approved,
        role: role.unwrap_or("REVIEWER").to_owned(),
        ..Participant::default()
    }
}
