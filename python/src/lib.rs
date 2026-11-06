//! Python binding for Ninot Bitbucket.

use ninot_bitbucket::Server as RustServer;
use ninot_bitbucket::model::{BuildStatus as RustBuildStatus, Participant as RustParticipant};
use ninot_bitbucket::state::{FaultRule, RepositoryHandle, State as RustState};
use ninot_common::state::StateHandle;
use pyo3::exceptions::{PyLookupError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};
use serde_json::Value;

/// Convert a Python value into a JSON-compatible fixture value.
fn to_json(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Value> {
    let json = py.import("json")?;
    let encoded = json.call_method1("dumps", (value,))?.extract::<String>()?;
    serde_json::from_str(&encoded).map_err(|error| PyValueError::new_err(error.to_string()))
}

/// Convert a JSON value to a Python value.
fn to_python(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    let json = py.import("json")?;
    Ok(json.call_method1("loads", (value.to_string(),))?.unbind())
}

/// A pull-request participant fixture.
#[pyclass(skip_from_py_object)]
#[derive(Clone)]
struct Participant {
    inner: RustParticipant,
}

#[pymethods]
impl Participant {
    /// Build a participant with a reviewer role by default.
    #[new]
    #[pyo3(signature = (account_id = "", approved = false, role = "REVIEWER"))]
    fn new(account_id: &str, approved: bool, role: &str) -> Self {
        Self {
            inner: RustParticipant {
                account_id: account_id.to_owned(),
                approved,
                role: role.to_owned(),
                ..RustParticipant::default()
            },
        }
    }

    /// Stable Bitbucket account identifier.
    #[getter]
    fn account_id(&self) -> &str {
        &self.inner.account_id
    }

    /// Whether the participant approved the pull request.
    #[getter]
    fn approved(&self) -> bool {
        self.inner.approved
    }

    /// Participant role.
    #[getter]
    fn role(&self) -> &str {
        &self.inner.role
    }
}

/// A server-side fixture state that can be populated before serving.
#[pyclass(skip_from_py_object)]
#[derive(Clone)]
struct State {
    inner: ninot_bitbucket::SharedState,
}

#[pymethods]
impl State {
    /// Create an empty fixture state.
    #[new]
    fn new() -> Self {
        Self {
            inner: StateHandle::new(RustState::default()),
        }
    }

    /// Register a repository and return its direct fixture handle.
    #[pyo3(signature = (workspace, slug, *, full_name=None, description="", default_branch="master", is_private=false, language="", size=0, updated_on="", clone_https=None))]
    #[allow(clippy::too_many_arguments)]
    fn add_repo(
        &self,
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
    ) -> PyResult<Repository> {
        let inner = RustState::add_repo(
            &self.inner,
            workspace,
            slug,
            full_name,
            description,
            default_branch,
            is_private,
            language,
            size,
            updated_on,
            clone_https,
        );
        Ok(Repository { inner })
    }

    /// Replace the user returned by `GET /2.0/user`.
    #[pyo3(signature = (*, username="", display_name="", account_id="", uuid=""))]
    fn set_user(&self, username: &str, display_name: &str, account_id: &str, uuid: &str) {
        self.inner
            .update(|state| state.set_user(username, display_name, account_id, uuid));
    }

    /// Optional Basic authentication credentials required by provider requests.
    #[getter]
    fn credentials(&self) -> Option<(String, String)> {
        self.inner.read(|state| state.credentials.clone())
    }

    /// Set or clear the credentials required by provider requests.
    #[setter]
    fn set_credentials(&self, credentials: Option<(String, String)>) {
        self.inner.update(|state| state.credentials = credentials);
    }

    /// Register a deterministic fault rule for provider requests.
    #[pyo3(signature = (*, method=None, path=None, nth=None, kind="response", payload=None, times=1))]
    #[allow(clippy::too_many_arguments)]
    fn add_fault(
        &self,
        py: Python<'_>,
        method: Option<String>,
        path: Option<String>,
        nth: Option<u32>,
        kind: &str,
        payload: Option<&Bound<'_, PyAny>>,
        times: u32,
    ) -> PyResult<()> {
        if nth.is_some_and(|occurrence| occurrence == 0) {
            return Err(PyValueError::new_err("fault occurrence must be positive"));
        }
        let payload = payload
            .map(|value| to_json(py, value))
            .transpose()?
            .unwrap_or(Value::Null);
        self.inner.update(|state| {
            state.faults.push(FaultRule {
                method,
                path,
                nth,
                kind: kind.to_owned(),
                payload,
                remaining: times,
                matched: 0,
            })
        });
        Ok(())
    }

    /// Select a request occurrence for fluent fault configuration.
    #[pyo3(signature = (*, method=None, path=None, nth=None))]
    fn when(
        &self,
        method: Option<String>,
        path: Option<String>,
        nth: Option<u32>,
    ) -> PyResult<FaultBuilder> {
        if nth.is_some_and(|occurrence| occurrence == 0) {
            return Err(PyValueError::new_err("fault occurrence must be positive"));
        }
        Ok(FaultBuilder {
            state: self.clone(),
            method: method.map(|method| method.to_ascii_uppercase()),
            path,
            nth,
        })
    }

    /// Inject a provider HTTP error with optional response fields.
    #[pyo3(signature = (*, status=500, path_contains=None, times=1, json=None, text=None, headers=None))]
    #[allow(clippy::too_many_arguments)]
    fn fail_requests(
        &self,
        py: Python<'_>,
        status: u16,
        path_contains: Option<String>,
        times: u32,
        json: Option<&Bound<'_, PyAny>>,
        text: Option<String>,
        headers: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        let json = json.map(|value| to_json(py, value)).transpose()?;
        let headers = headers
            .map(|value| to_json(py, value))
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        let mut payload = serde_json::json!({"status_code": status, "headers": headers});
        if let Some(json) = json {
            payload["json"] = json;
        } else {
            payload["text"] = Value::String(text.unwrap_or_else(|| format!("injected {status}")));
        }
        self.when(None, path_contains, None)?
            .fail_payload(payload, times)
    }

    /// Remove registered fault rules and their observations.
    fn clear_faults(&self) {
        self.inner.update(RustState::clear_faults);
    }

    /// Ordered ordinary HTTP requests recorded by the service.
    #[getter]
    fn requests(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        let requests = self.inner.read(|state| state.requests.clone());
        let request_type = py.import("httpx")?.getattr("Request")?;
        let rows = PyList::empty(py);
        for request in requests {
            let kwargs = PyDict::new(py);
            kwargs.set_item("headers", request.headers)?;
            kwargs.set_item("content", PyBytes::new(py, &request.body))?;
            rows.append(request_type.call((request.method, request.url), Some(&kwargs))?)?;
        }
        Ok(rows.unbind())
    }

    /// Faults already applied to ordinary HTTP requests.
    #[getter]
    fn applied_faults(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let faults = self.inner.read(|state| state.applied_faults.clone());
        to_python(
            py,
            &serde_json::to_value(faults)
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?,
        )
    }

    /// Repositories keyed by their full workspace and slug name.
    #[getter]
    fn _repos(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let names = self
            .inner
            .read(|state| state.repositories.keys().cloned().collect::<Vec<_>>());
        let repositories = PyDict::new(py);
        for name in names {
            let handle = RepositoryHandle {
                state: self.inner.clone(),
                full_name: name.clone(),
            };
            repositories.set_item(name, Py::new(py, Repository { inner: handle })?)?;
        }
        Ok(repositories.into_any().unbind())
    }

    /// Return a versioned JSON snapshot of this fixture.
    fn snapshot(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let snapshot = self
            .inner
            .read(|state| state.snapshot())
            .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        to_python(py, &snapshot)
    }

    /// Restore this fixture from a versioned JSON snapshot.
    fn restore(&self, py: Python<'_>, snapshot: &Bound<'_, PyAny>) -> PyResult<()> {
        let snapshot = to_json(py, snapshot)?;
        self.inner
            .update(|state| state.restore(snapshot))
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }
}

/// A scoped fault selector that installs response and transport failures.
#[pyclass]
struct FaultBuilder {
    state: State,
    method: Option<String>,
    path: Option<String>,
    nth: Option<u32>,
}

impl FaultBuilder {
    /// Add one configured fault action to the shared state.
    fn fail_payload(&self, payload: Value, times: u32) -> PyResult<()> {
        if times == 0 {
            return Err(PyValueError::new_err("fault times must be positive"));
        }
        self.state.inner.update(|state| {
            state.faults.push(FaultRule {
                method: self.method.clone(),
                path: self.path.clone(),
                nth: self.nth,
                kind: "response".to_owned(),
                payload,
                remaining: times,
                matched: 0,
            });
        });
        Ok(())
    }
}

#[pymethods]
impl FaultBuilder {
    /// Return an error response on the selected request occurrences.
    #[pyo3(signature = (status=500, *, json=None, text=None, headers=None, retry_after=None, once=false, times=None))]
    #[allow(clippy::too_many_arguments)]
    fn fail(
        &self,
        py: Python<'_>,
        status: u16,
        json: Option<&Bound<'_, PyAny>>,
        text: Option<String>,
        headers: Option<&Bound<'_, PyAny>>,
        retry_after: Option<u64>,
        once: bool,
        times: Option<u32>,
    ) -> PyResult<()> {
        let headers = headers
            .map(|value| to_json(py, value))
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        let mut payload = serde_json::json!({"status_code": status, "headers": headers});
        if let Some(retry_after) = retry_after {
            payload["headers"]["Retry-After"] = Value::String(retry_after.to_string());
        }
        if let Some(json) = json {
            payload["json"] = to_json(py, json)?;
        } else {
            payload["text"] = Value::String(text.unwrap_or_else(|| format!("injected {status}")));
        }
        self.fail_payload(payload, if once { 1 } else { times.unwrap_or(1) })
    }

    /// Return a malformed successful response on selected requests.
    #[pyo3(signature = (mode="truncate", *, once=false, times=None))]
    fn corrupt(&self, mode: &str, once: bool, times: Option<u32>) -> PyResult<()> {
        if !["truncate", "empty", "not-json", "null"].contains(&mode) {
            return Err(PyValueError::new_err(format!(
                "unknown corruption mode: {mode}"
            )));
        }
        self.add_action("corrupt", serde_json::json!({"mode": mode}), once, times)
    }

    /// Drop the connection for selected requests.
    #[pyo3(signature = (*, once=false, times=None))]
    fn drop(&self, once: bool, times: Option<u32>) -> PyResult<()> {
        self.add_action("drop", serde_json::json!({}), once, times)
    }

    /// Delay the selected response.
    #[pyo3(signature = (*, delay=1.0, once=false, times=None))]
    fn timeout(&self, delay: f64, once: bool, times: Option<u32>) -> PyResult<()> {
        if delay < 0.0 {
            return Err(PyValueError::new_err("fault delay must be nonnegative"));
        }
        self.add_action("timeout", serde_json::json!({"delay": delay}), once, times)
    }
}

impl FaultBuilder {
    /// Add a non-response fault action.
    fn add_action(
        &self,
        kind: &str,
        payload: Value,
        once: bool,
        times: Option<u32>,
    ) -> PyResult<()> {
        let times = if once { 1 } else { times.unwrap_or(1) };
        if times == 0 {
            return Err(PyValueError::new_err("fault times must be positive"));
        }
        self.state.inner.update(|state| {
            state.faults.push(FaultRule {
                method: self.method.clone(),
                path: self.path.clone(),
                nth: self.nth,
                kind: kind.to_owned(),
                payload,
                remaining: times,
                matched: 0,
            })
        });
        Ok(())
    }
}

/// A repository handle whose methods mutate the state served by its owner.
#[pyclass]
struct Repository {
    inner: RepositoryHandle,
}

#[pymethods]
impl Repository {
    /// Create and register a pull request fixture.
    #[pyo3(signature = (pr_id, *, title="", description="", state="OPEN", draft=false, source_branch="feature", target_branch="master", source_repo=None, target_repo=None, author_display_name="", author_nickname="", author_account_id="", participants=None, updated_on="", url=None))]
    #[allow(clippy::too_many_arguments)]
    fn create_pull_request(
        &self,
        py: Python<'_>,
        pr_id: u64,
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
        participants: Option<Vec<Py<Participant>>>,
        updated_on: &str,
        url: Option<&str>,
    ) -> PyResult<PullRequest> {
        let participant_values = participants
            .unwrap_or_default()
            .into_iter()
            .map(|participant| participant.borrow(py).inner.clone())
            .collect();
        let inner = self
            .inner
            .create_pull_request(
                pr_id,
                title,
                description,
                state,
                draft,
                source_branch,
                target_branch,
                source_repo,
                target_repo,
                author_display_name,
                author_nickname,
                author_account_id,
                participant_values,
                updated_on,
                url,
            )
            .map_err(PyLookupError::new_err)?;
        Ok(PullRequest { inner })
    }

    /// Store a file on the repository's main branch.
    fn add_file(&self, path: &str, content: &Bound<'_, PyAny>) -> PyResult<()> {
        let content = match content.cast::<PyBytes>() {
            Ok(bytes) => bytes.as_bytes().to_vec(),
            Err(_) => content.extract::<String>()?.into_bytes(),
        };
        self.inner
            .add_file(path, content)
            .map_err(PyLookupError::new_err)
    }

    /// Repository full workspace/slug name.
    #[getter]
    fn full_name(&self) -> PyResult<String> {
        self.inner
            .get()
            .map(|repo| repo.full_name)
            .ok_or_else(|| PyLookupError::new_err("repository no longer exists"))
    }

    /// Pull requests currently stored on the repository.
    #[getter]
    fn prs(&self, py: Python<'_>) -> PyResult<Vec<Py<PullRequest>>> {
        self.inner
            .pull_requests()
            .into_iter()
            .map(|inner| Py::new(py, PullRequest { inner }))
            .collect()
    }
}

/// A mutable pull request fixture handle.
#[pyclass]
struct PullRequest {
    inner: ninot_bitbucket::state::PullRequestHandle,
}

#[pymethods]
impl PullRequest {
    /// Pull request identifier.
    #[getter]
    fn id(&self) -> u64 {
        self.inner.id
    }

    /// Current pull request title.
    #[getter]
    fn title(&self) -> PyResult<String> {
        self.inner
            .get()
            .map(|pull| pull.title)
            .ok_or_else(|| PyLookupError::new_err("pull request no longer exists"))
    }

    /// Set the pull request title through property assignment.
    #[setter(title)]
    fn set_title_property(&self, title: &str) -> PyResult<()> {
        self.inner.set_title(title).map_err(PyLookupError::new_err)
    }

    /// Current pull request description.
    #[getter]
    fn description(&self) -> PyResult<String> {
        self.inner
            .get()
            .map(|pull| pull.description)
            .ok_or_else(|| PyLookupError::new_err("pull request no longer exists"))
    }

    /// Set the pull request description through property assignment.
    #[setter(description)]
    fn set_description_property(&self, description: &str) -> PyResult<()> {
        self.inner
            .set_description(description)
            .map_err(PyLookupError::new_err)
    }

    /// Set the pull request title.
    fn set_title(&self, title: &str) -> PyResult<()> {
        self.inner.set_title(title).map_err(PyLookupError::new_err)
    }

    /// Set the pull request description.
    fn set_description(&self, description: &str) -> PyResult<()> {
        self.inner
            .set_description(description)
            .map_err(PyLookupError::new_err)
    }

    /// Add a build status, newest first.
    #[pyo3(signature = (state, *, key="", name="", url=""))]
    fn add_status(&self, state: &str, key: &str, name: &str, url: &str) -> PyResult<()> {
        self.inner
            .add_status(RustBuildStatus {
                state: state.to_owned(),
                key: key.to_owned(),
                name: name.to_owned(),
                url: url.to_owned(),
                ..RustBuildStatus::default()
            })
            .map_err(PyLookupError::new_err)
    }
}

/// A running Bitbucket Cloud fake.
#[pyclass]
struct Server {
    inner: Option<RustServer>,
    state: State,
}

#[pymethods]
impl Server {
    /// Start a server, optionally using a previously constructed state.
    #[new]
    #[pyo3(signature = (state=None))]
    fn new(py: Python<'_>, state: Option<Py<State>>) -> PyResult<Self> {
        let state = state.map(|state| state.borrow(py).clone());
        let inner = match state.as_ref() {
            Some(state) => RustServer::start_with_handle(state.inner.clone()),
            None => RustServer::start(),
        }
        .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
        let fixture_state = state.unwrap_or_else(|| State {
            inner: inner.state(),
        });
        Ok(Self {
            inner: Some(inner),
            state: fixture_state,
        })
    }

    /// HTTP API base URL including the `/2.0` prefix.
    #[getter]
    fn url(&self) -> PyResult<String> {
        self.inner
            .as_ref()
            .map(RustServer::url)
            .ok_or_else(|| PyRuntimeError::new_err("server was stopped"))
    }

    /// Shared fixture state.
    #[getter]
    fn state(&self) -> State {
        self.state.clone()
    }

    /// Return this running server for use by Python context managers.
    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    /// Start request profiling and clear previous observations.
    fn start_profile(&self) -> PyResult<()> {
        self.inner
            .as_ref()
            .map(RustServer::start_profile)
            .ok_or_else(|| PyRuntimeError::new_err("server was stopped"))
    }

    /// Whether request profiling is currently enabled.
    #[getter]
    fn is_recording(&self) -> bool {
        self.inner.as_ref().is_some_and(RustServer::is_recording)
    }

    /// Stop request profiling and return ordered HTTP observations.
    fn stop_profile(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let profile = self
            .inner
            .as_ref()
            .ok_or_else(|| PyRuntimeError::new_err("server was stopped"))?
            .stop_profile();
        let hits: Vec<_> = profile
            .hits()
            .iter()
            .map(|hit| {
                serde_json::json!({
                    "method": hit.method,
                    "path": hit.path,
                    "query": hit.query,
                    "request": hit.request(),
                    "label": hit.label,
                    "status": hit.status,
                    "request_bytes": hit.request_bytes,
                    "response_bytes": hit.response_bytes,
                    "duration_ms": hit.duration.as_millis(),
                })
            })
            .collect();
        let labels: Vec<_> = profile
            .by_label()
            .iter()
            .map(|stats| {
                serde_json::json!({
                    "label": stats.label,
                    "count": stats.count,
                    "total_ms": stats.total.as_millis(),
                })
            })
            .collect();
        let repeats: Vec<_> = profile
            .repeats()
            .iter()
            .map(|repeat| serde_json::json!({"request": repeat.request, "count": repeat.count}))
            .collect();
        to_python(
            py,
            &serde_json::json!({
                "requests": hits.len(),
                "hits": hits,
                "labels": labels,
                "repeats": repeats,
            }),
        )
    }

    /// Stop the HTTP server.
    fn stop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.stop();
        }
    }

    /// Stop the server when leaving a Python context manager.
    fn __exit__(
        &mut self,
        _exception_type: Option<&Bound<'_, PyAny>>,
        _exception_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> bool {
        self.stop();
        false
    }
}

/// Register the public binding types in the extension module.
#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Server>()?;
    module.add_class::<State>()?;
    module.add_class::<Repository>()?;
    module.add_class::<PullRequest>()?;
    module.add_class::<Participant>()?;
    module.add_class::<FaultBuilder>()?;
    Ok(())
}
