//! Typed Bitbucket Cloud request and response models.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The authenticated Bitbucket user returned by `/user` and embedded in pull requests.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct User {
    /// The user's login name.
    #[serde(default)]
    pub username: String,
    /// The display name shown in the UI.
    #[serde(default)]
    pub display_name: String,
    /// The stable Bitbucket account identifier.
    #[serde(default)]
    pub account_id: String,
    /// The provider UUID.
    #[serde(default)]
    pub uuid: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A repository's default branch reference.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MainBranch {
    /// The branch name.
    #[serde(default)]
    pub name: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One repository clone link.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CloneLink {
    /// Clone protocol name, such as `https` or `ssh`.
    #[serde(default)]
    pub name: String,
    /// The clone URL.
    #[serde(default)]
    pub href: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A web link.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HtmlLink {
    /// The linked URL.
    #[serde(default)]
    pub href: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Repository links returned by Bitbucket.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RepoLinks {
    /// Clone URLs indexed by protocol.
    #[serde(default, rename = "clone")]
    pub clone_links: Vec<CloneLink>,
    /// The repository's web page.
    #[serde(default)]
    pub html: HtmlLink,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A repository read model.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Repository {
    /// The repository slug.
    #[serde(default)]
    pub slug: String,
    /// The workspace and slug separated by `/`.
    #[serde(default)]
    pub full_name: String,
    /// Repository description.
    #[serde(default)]
    pub description: String,
    /// Default branch information.
    #[serde(default)]
    pub mainbranch: MainBranch,
    /// Whether the repository is private.
    #[serde(default)]
    pub is_private: bool,
    /// Repository language.
    #[serde(default)]
    pub language: String,
    /// Repository size in bytes.
    #[serde(default)]
    pub size: u64,
    /// Last update timestamp as returned by Bitbucket.
    #[serde(default)]
    pub updated_on: String,
    /// Repository URLs.
    #[serde(default)]
    pub links: RepoLinks,
    /// Pull requests owned by this repository.
    #[serde(default, rename = "prs")]
    pub pull_requests: Vec<PullRequest>,
    /// File contents on the main branch, keyed by repository-relative path.
    #[serde(default)]
    pub files: BTreeMap<String, Vec<u8>>,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Public repository response, without fixture-owned pull requests.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RepositoryResponse {
    /// The repository slug.
    pub slug: String,
    /// The workspace and slug separated by `/`.
    pub full_name: String,
    /// Repository description.
    pub description: String,
    /// Default branch information.
    pub mainbranch: MainBranch,
    /// Whether the repository is private.
    pub is_private: bool,
    /// Repository language.
    pub language: String,
    /// Repository size in bytes.
    pub size: u64,
    /// Last update timestamp as returned by Bitbucket.
    pub updated_on: String,
    /// Repository URLs.
    pub links: RepoLinks,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl From<&Repository> for RepositoryResponse {
    fn from(repository: &Repository) -> Self {
        Self {
            slug: repository.slug.clone(),
            full_name: repository.full_name.clone(),
            description: repository.description.clone(),
            mainbranch: repository.mainbranch.clone(),
            is_private: repository.is_private,
            language: repository.language.clone(),
            size: repository.size,
            updated_on: repository.updated_on.clone(),
            links: repository.links.clone(),
            extra: repository.extra.clone(),
        }
    }
}

/// A branch name within a pull-request endpoint.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct BranchRef {
    /// The branch name.
    #[serde(default)]
    pub name: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A repository reference within a pull-request endpoint.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RepoRef {
    /// The full repository name.
    #[serde(default)]
    pub full_name: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// The source or destination endpoint of a pull request.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PrEndpoint {
    /// The branch reference.
    #[serde(default)]
    pub branch: BranchRef,
    /// The repository reference.
    #[serde(default)]
    pub repository: RepoRef,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Author fields embedded in a pull request.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PrAuthor {
    /// Display name.
    #[serde(default)]
    pub display_name: String,
    /// Account nickname.
    #[serde(default)]
    pub nickname: String,
    /// Stable account identifier.
    #[serde(default)]
    pub account_id: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A reviewer or participant in a pull request.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Participant {
    /// Stable account identifier.
    #[serde(default)]
    pub account_id: String,
    /// Whether this participant approved the pull request.
    #[serde(default)]
    pub approved: bool,
    /// Participant role, commonly `REVIEWER`.
    #[serde(default = "default_reviewer_role")]
    pub role: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Returns the default pull-request participant role.
fn default_reviewer_role() -> String {
    "REVIEWER".to_owned()
}

/// The links block embedded in a pull request.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PrLinks {
    /// The pull request's HTML page.
    #[serde(default)]
    pub html: HtmlLink,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// One pull-request build status.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct BuildStatus {
    /// Build result state, such as `SUCCESSFUL` or `FAILED`.
    #[serde(default)]
    pub state: String,
    /// Build system key.
    #[serde(default)]
    pub key: String,
    /// Human-readable build name.
    #[serde(default)]
    pub name: String,
    /// Build details URL.
    #[serde(default)]
    pub url: String,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// A Bitbucket pull request.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PullRequest {
    /// Pull request number.
    #[serde(default)]
    pub id: u64,
    /// Pull request title.
    #[serde(default)]
    pub title: String,
    /// Pull request description.
    #[serde(default)]
    pub description: String,
    /// Provider state, such as `OPEN`, `MERGED` or `DECLINED`.
    #[serde(default = "default_open_state")]
    pub state: String,
    /// Whether the pull request is a draft.
    #[serde(default)]
    pub draft: bool,
    /// Source branch and repository.
    #[serde(default)]
    pub source: PrEndpoint,
    /// Destination branch and repository.
    #[serde(default)]
    pub destination: PrEndpoint,
    /// Pull request author.
    #[serde(default)]
    pub author: PrAuthor,
    /// Reviewers and other participants.
    #[serde(default)]
    pub participants: Vec<Participant>,
    /// Pull request links.
    #[serde(default)]
    pub links: PrLinks,
    /// Last update timestamp as returned by Bitbucket.
    #[serde(default)]
    pub updated_on: String,
    /// Build statuses, newest first.
    #[serde(default)]
    pub statuses: Vec<BuildStatus>,
    /// Provider fields outside the subset used by consumers.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Returns the default state for a newly seeded pull request.
fn default_open_state() -> String {
    "OPEN".to_owned()
}

/// Optional value in a partial update, preserving absent versus explicit `null`.
#[derive(Clone, Debug, Default)]
pub enum PatchField<T> {
    /// The request did not include the field.
    #[default]
    Missing,
    /// The request explicitly set the field to null.
    Null,
    /// The request supplied a value.
    Value(T),
}

impl<'de, T> Deserialize<'de> for PatchField<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match Option::<T>::deserialize(deserializer)? {
            Some(value) => Self::Value(value),
            None => Self::Null,
        })
    }
}

/// Create pull-request payload as sent by Bitbucket clients.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct CreatePullRequestRequest {
    /// Pull request title.
    #[serde(default)]
    pub title: String,
    /// Pull request description.
    #[serde(default)]
    pub description: String,
    /// Optional source endpoint.
    #[serde(default)]
    pub source: Option<PrEndpoint>,
    /// Optional destination endpoint.
    #[serde(default)]
    pub destination: Option<PrEndpoint>,
    /// Draft setting.
    #[serde(default)]
    pub draft: Option<DraftRequest>,
    /// Provider-specific request fields ignored by this subset.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Draft flag in a pull-request create request.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DraftRequest {
    /// Whether to create a draft pull request.
    #[serde(default)]
    pub enabled: bool,
    /// Provider-specific request fields ignored by this subset.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Partial pull-request update payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct UpdatePullRequestRequest {
    /// Optional title update.
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    pub title: PatchField<String>,
    /// Optional description update.
    #[serde(default, deserialize_with = "deserialize_patch_field")]
    pub description: PatchField<String>,
    /// Provider-specific request fields ignored by this subset.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Preserves whether a partial update field was absent, null, or valued.
fn deserialize_patch_field<'de, D, T>(deserializer: D) -> Result<PatchField<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    PatchField::deserialize(deserializer)
}
