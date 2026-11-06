//! Running Bitbucket server and fixture controls.

use std::fmt;
use std::io;
use std::net::SocketAddr;

use ninot_common::profile::{Profile, Profiler};
use ninot_common::server::Server as Harness;
use ninot_common::state::{Snapshot, SnapshotError as CommonSnapshotError, StateHandle};

use crate::api::{self, ApiContext};
use crate::state::{STATE_VERSION, SharedState, State};

/// Why a Bitbucket server could not start.
#[derive(Debug)]
pub enum StartError {
    /// The network listener or serving runtime could not start.
    Io(io::Error),
    /// A snapshot did not match the current state schema.
    Snapshot(CommonSnapshotError),
}

impl fmt::Display for StartError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Bitbucket server could not start: {error}"),
            Self::Snapshot(error) => {
                write!(formatter, "Bitbucket snapshot could not start: {error}")
            }
        }
    }
}

impl std::error::Error for StartError {}

/// An HTTP server with shared fixture state and request profiling.
pub struct Server {
    /// The live HTTP listener.
    harness: Harness,
    /// State shared by fixture calls and handlers.
    state: SharedState,
    /// Request profiler attached to the HTTP router.
    profiler: Profiler,
}

impl Server {
    /// Starts an empty Bitbucket server on an available local port.
    ///
    /// # Errors
    ///
    /// Returns an error when binding or starting the serving runtime fails.
    pub fn start() -> Result<Self, StartError> {
        Self::start_with_state(State::default())
    }

    /// Starts a Bitbucket server with the supplied state on an available port.
    ///
    /// # Errors
    ///
    /// Returns an error when binding or starting the serving runtime fails.
    pub fn start_with_state(state: State) -> Result<Self, StartError> {
        Self::start_with_handle(StateHandle::new(state))
    }

    /// Starts a server using an existing state handle so callers can share live fixtures.
    ///
    /// # Errors
    ///
    /// Returns an error when binding or starting the serving runtime fails.
    pub fn start_with_handle(state: SharedState) -> Result<Self, StartError> {
        Self::start_on_with_handle(SocketAddr::from(([127, 0, 0, 1], 0)), state)
    }

    /// Starts a Bitbucket server on `address` with the supplied state.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested address cannot be served.
    pub fn start_on_with_state(address: SocketAddr, initial: State) -> Result<Self, StartError> {
        Self::start_on_with_handle(address, StateHandle::new(initial))
    }

    /// Starts a server on `address` with the supplied shared state handle.
    ///
    /// # Errors
    ///
    /// Returns an error when binding or starting the serving runtime fails.
    pub fn start_on_with_handle(
        address: SocketAddr,
        state: SharedState,
    ) -> Result<Self, StartError> {
        let listener = Harness::bind(address)?;
        let local_addr = listener.local_addr()?;
        state.update(|initial| initial.base_url = format!("http://{local_addr}/2.0"));
        let profiler = Profiler::new();
        let router = api::router(
            ApiContext {
                state: state.clone(),
            },
            profiler.clone(),
        );
        let harness = Harness::serve(listener, router)?;
        Ok(Self {
            harness,
            state,
            profiler,
        })
    }

    /// Starts a server with the state stored in a versioned common snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the snapshot version/shape is unsupported or serving fails.
    pub fn start_from_snapshot(snapshot: &Snapshot) -> Result<Self, StartError> {
        let state =
            StateHandle::<State>::restore(snapshot, STATE_VERSION).map_err(StartError::Snapshot)?;
        Self::start_with_handle(state)
    }

    /// Base URL of the ordinary HTTP API, including `/2.0`.
    pub fn url(&self) -> String {
        self.state.read(|state| state.base_url.clone())
    }

    /// Returns the handle to fixture state shared by all request handlers.
    pub fn state(&self) -> SharedState {
        self.state.clone()
    }

    /// Captures the full state with its schema version.
    pub fn snapshot(&self) -> Snapshot {
        self.state.snapshot(STATE_VERSION)
    }

    /// Enables request profiling and clears earlier observations.
    pub fn start_profile(&self) {
        self.profiler.start();
    }

    /// Stops request profiling and returns its ordered observations.
    pub fn stop_profile(&self) -> Profile {
        self.profiler.stop()
    }

    /// Whether request profiling is currently enabled.
    pub fn is_recording(&self) -> bool {
        self.profiler.is_recording()
    }

    /// Stops serving and waits for the serving thread to exit.
    pub fn stop(mut self) {
        self.harness.stop();
    }
}

impl From<io::Error> for StartError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
