//! Stateful Bitbucket Cloud protocol fake.

pub mod api;
pub mod model;
pub mod server;
pub mod state;

pub use model::{
    BranchRef, BuildStatus, CloneLink, HtmlLink, MainBranch, Participant, PrAuthor, PrEndpoint,
    PrLinks, PullRequest, RepoLinks, RepoRef, Repository, RepositoryResponse, User,
};
pub use server::{Server, StartError};
pub use state::{AppliedFault, FaultRule, RequestRecord, SharedState, SnapshotError, State};
