//! Ordinary Bitbucket Cloud HTTP routes and their typed read models.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::State as AxumState;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, RETRY_AFTER};
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, Uri};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use ninot_common::profile::{Label, Profiler};
use ninot_common::state::StateHandle;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::form_urlencoded;

use crate::model::{
    CreatePullRequestRequest, PatchField, PullRequest, RepositoryResponse, UpdatePullRequestRequest,
};
use crate::state::{RequestRecord, State};

/// Maximum ordinary Bitbucket request body accepted by the fake.
const BODY_LIMIT: usize = 16 * 1024 * 1024;

/// State and observation shared by all requests to one server.
#[derive(Clone)]
pub(crate) struct ApiContext {
    /// Bitbucket fixture state.
    pub state: StateHandle<State>,
}

/// Builds the catch-all router for Bitbucket's small supported protocol surface.
pub(crate) fn router(context: ApiContext, profiler: Profiler) -> Router {
    profiler.layer(Router::new().fallback(dispatch).with_state(context))
}

/// Records, authenticates and routes one ordinary HTTP request.
async fn dispatch(
    AxumState(context): AxumState<ApiContext>,
    request: Request<Body>,
) -> Response<Body> {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    let body = match to_bytes(request.into_body(), BODY_LIMIT).await {
        Ok(body) => body,
        Err(_) => {
            return json_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                &ErrorMessage {
                    message: "request body too large".to_owned(),
                },
                "unmatched",
            );
        }
    };
    record_request(&context.state, &method, &uri, &headers, &body);

    if !authorized(&context.state, &headers) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            &ErrorMessage {
                message: "Unauthorized".to_owned(),
            },
            "auth",
        );
    }
    if let Some(response) = apply_faults(&context.state, &method, uri.path()).await {
        return response;
    }

    let path = provider_path(uri.path());
    let query = query_map(uri.query());
    match (method.as_str(), path.as_str()) {
        ("GET", "/user") => {
            let user = context.state.read(|state| state.user.clone());
            json_response(StatusCode::OK, &user, "GET /user")
        }
        ("GET", "/repositories") => {
            let repos = context.state.read(|state| {
                state
                    .repositories
                    .values()
                    .map(RepositoryResponse::from)
                    .collect::<Vec<_>>()
            });
            page_response(&uri, &headers, &query, repos, "GET /repositories")
        }
        _ => dispatch_repository(
            &context.state,
            method.as_str(),
            &uri,
            &headers,
            &path,
            &query,
            body,
        ),
    }
}

/// Routes requests whose path is rooted in repositories or workspaces.
fn dispatch_repository(
    state: &StateHandle<State>,
    method: &str,
    uri: &Uri,
    headers: &HeaderMap,
    path: &str,
    query: &BTreeMap<String, String>,
    body: Bytes,
) -> Response<Body> {
    let parts = path.trim_start_matches('/').split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["repositories", workspace] if method == "GET" => {
            let mut repositories = state.read(|state| {
                state
                    .repositories
                    .iter()
                    .filter(|(key, _)| {
                        key.split_once('/')
                            .is_some_and(|(owner, _)| owner == *workspace)
                    })
                    .map(|(_, repository)| RepositoryResponse::from(repository))
                    .collect::<Vec<_>>()
            });
            if let Some(name) = query.get("q").and_then(|q| extract_query(q, "name~")) {
                let needle = strip_quotes(name).to_lowercase();
                repositories.retain(|repository| repository.slug.to_lowercase().contains(&needle));
            }
            page_response(
                uri,
                headers,
                query,
                repositories,
                "GET /repositories/{workspace}",
            )
        }
        ["repositories", workspace, slug, ..] => {
            repo_operation(state, method, uri, headers, workspace, slug, query, body)
        }
        ["workspaces", workspace, "pullrequests", selected_user] if method == "GET" => {
            list_workspace_pulls(state, uri, headers, query, workspace, selected_user)
        }
        _ => json_response(
            StatusCode::NOT_IMPLEMENTED,
            &ErrorMessage {
                message: format!("Not implemented: {method} {}", uri.path()),
            },
            "unmatched",
        ),
    }
}

/// Runs the operation selected for one repository resource path.
#[allow(clippy::too_many_arguments)]
fn repo_operation(
    state: &StateHandle<State>,
    method: &str,
    uri: &Uri,
    headers: &HeaderMap,
    workspace: &str,
    slug: &str,
    query: &BTreeMap<String, String>,
    body: Bytes,
) -> Response<Body> {
    let key = format!("{workspace}/{slug}");
    let subpath = provider_path(uri.path())
        .trim_start_matches(&format!("/repositories/{workspace}/{slug}"))
        .to_owned();
    match (method, subpath.as_str()) {
        ("GET", "") => state
            .read(|state| state.repositories.get(&key).map(RepositoryResponse::from))
            .map_or_else(
                || {
                    not_found(
                        &format!("no such repository: {key}"),
                        "GET /repositories/{workspace}/{repo_slug}",
                    )
                },
                |repository| {
                    json_response(
                        StatusCode::OK,
                        &repository,
                        "GET /repositories/{workspace}/{repo_slug}",
                    )
                },
            ),
        ("GET", "/pullrequests") => {
            let pulls = state.read(|state| {
                state
                    .repositories
                    .get(&key)
                    .map(|repo| repo.pull_requests.clone())
                    .unwrap_or_default()
            });
            let pulls = filter_pull_requests(pulls, query);
            page_response(
                uri,
                headers,
                query,
                pulls,
                "GET /repositories/{workspace}/{repo_slug}/pullrequests",
            )
        }
        ("POST", "/pullrequests") => create_pull_request(state, &key, body),
        ("GET", suffix) if pull_request_status_id(suffix).is_some() => {
            let id = pull_request_status_id(suffix);
            let Some(id) = id else {
                return json_response(
                    StatusCode::NOT_FOUND,
                    &ErrorMessage {
                        message: "no such pull request".to_owned(),
                    },
                    "GET /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}/statuses",
                );
            };
            state.read(|state| {
                let Some(pull) = state.repositories.get(&key).and_then(|repo| repo.pull_requests.iter().find(|pull| pull.id == id)) else {
                    return not_found(&format!("no such PR: {key}#{id}"), "GET /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}/statuses");
                };
                page_response(uri, headers, query, pull.statuses.clone(), "GET /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}/statuses")
            })
        }
        ("GET", suffix) if pull_request_resource_id(suffix).is_some() => {
            let Some(id) = pull_request_resource_id(suffix) else {
                unreachable!("the match guard validated the pull request id")
            };
            state.read(|state| {
                state.repositories.get(&key)
                    .and_then(|repo| repo.pull_requests.iter().find(|pull| pull.id == id))
                    .map_or_else(
                        || not_found(&format!("no such PR: {key}#{id}"), "GET /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}"),
                        |pull| json_response(StatusCode::OK, pull, "GET /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}"),
                    )
            })
        }
        ("PUT", suffix) if pull_request_resource_id(suffix).is_some() => {
            let Some(id) = pull_request_resource_id(suffix) else {
                unreachable!("the match guard validated the pull request id")
            };
            update_pull_request(state, &key, id, body)
        }
        ("GET", suffix) if suffix.starts_with("/src/") => {
            file_contents(state, uri, &key, &suffix["/src/".len()..], query)
        }
        _ => json_response(
            StatusCode::NOT_IMPLEMENTED,
            &ErrorMessage {
                message: format!("Not implemented: {method} {}", uri.path()),
            },
            "unmatched",
        ),
    }
}

/// Validates and stores a pull request creation request.
fn create_pull_request(state: &StateHandle<State>, key: &str, body: Bytes) -> Response<Body> {
    if !state.read(|state| state.repositories.contains_key(key)) {
        return not_found(
            &format!("no such repository: {key}"),
            "POST /repositories/{workspace}/{repo_slug}/pullrequests",
        );
    }
    let input: CreatePullRequestRequest = match decode_json(&body) {
        Ok(input) => input,
        Err(message) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                &InputError { error: message },
                "POST /repositories/{workspace}/{repo_slug}/pullrequests",
            );
        }
    };
    let mut state = state.lock();
    let Some(repository) = state.repositories.get_mut(key) else {
        return not_found(
            &format!("no such repository: {key}"),
            "POST /repositories/{workspace}/{repo_slug}/pullrequests",
        );
    };
    let id = repository
        .pull_requests
        .iter()
        .map(|pull| pull.id)
        .max()
        .unwrap_or(0)
        + 1;
    let source = input.source.unwrap_or_default();
    let destination = input.destination.unwrap_or_default();
    let pull = PullRequest {
        id,
        title: input.title,
        description: input.description,
        state: "OPEN".to_owned(),
        draft: input.draft.is_some_and(|draft| draft.enabled),
        source,
        destination,
        ..PullRequest::default()
    };
    repository.pull_requests.push(pull.clone());
    json_response(
        StatusCode::CREATED,
        &pull,
        "POST /repositories/{workspace}/{repo_slug}/pullrequests",
    )
}

/// Applies the supported partial update fields to an existing pull request.
fn update_pull_request(
    state: &StateHandle<State>,
    key: &str,
    id: u64,
    body: Bytes,
) -> Response<Body> {
    if !state.read(|state| {
        state
            .repositories
            .get(key)
            .is_some_and(|repo| repo.pull_requests.iter().any(|pull| pull.id == id))
    }) {
        return not_found(
            &format!("no such PR: {key}#{id}"),
            "PUT /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}",
        );
    }
    let input: UpdatePullRequestRequest = match decode_json(&body) {
        Ok(input) => input,
        Err(message) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                &InputError { error: message },
                "PUT /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}",
            );
        }
    };
    let invalid_patch =
        matches!(&input.title, PatchField::Null) || matches!(&input.description, PatchField::Null);
    if invalid_patch {
        return json_response(
            StatusCode::BAD_REQUEST,
            &InputError {
                error: "title and description must be strings".to_owned(),
            },
            "PUT /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}",
        );
    }
    let mut state = state.lock();
    let Some(pull) = state
        .repositories
        .get_mut(key)
        .and_then(|repo| repo.pull_requests.iter_mut().find(|pull| pull.id == id))
    else {
        return not_found(
            &format!("no such PR: {key}#{id}"),
            "PUT /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}",
        );
    };
    if let PatchField::Value(value) = input.title {
        pull.title = value;
    }
    if let PatchField::Value(value) = input.description {
        pull.description = value;
    }
    json_response(
        StatusCode::OK,
        pull,
        "PUT /repositories/{workspace}/{repo_slug}/pullrequests/{pull_request_id}",
    )
}

/// Serves raw file bytes from the repository's main branch.
fn file_contents(
    state: &StateHandle<State>,
    uri: &Uri,
    key: &str,
    commit_and_path: &str,
    query: &BTreeMap<String, String>,
) -> Response<Body> {
    const LABEL: &str = "GET /repositories/{workspace}/{repo_slug}/src/{commit}/{path}";
    if query.contains_key("format")
        || !commit_and_path.contains('/')
        || commit_and_path.ends_with('/')
    {
        return json_response(
            StatusCode::NOT_IMPLEMENTED,
            &ErrorMessage {
                message: format!("Not implemented: GET {}", uri.path()),
            },
            "unmatched",
        );
    }
    state.read(|state| {
        let Some(repo) = state.repositories.get(key) else {
            return src_error(
                "You may not have access to this repository or it no longer exists in this workspace. If you think this repository exists and you have access, make sure you are authenticated.",
                None,
            );
        };
        let Some(path) = commit_and_path.strip_prefix(&format!("{}/", repo.mainbranch.name))
        else {
            let commit = commit_and_path.split('/').next().unwrap_or_default();
            return src_error("Commit not found", Some(commit));
        };
        let Some(content) = repo.files.get(path) else {
            return src_error(&format!("No such file or directory: {path}"), None);
        };
        let content_type = if std::str::from_utf8(content).is_ok() {
            "text/plain"
        } else {
            "application/octet-stream"
        };
        labeled_response(StatusCode::OK, content_type, content.clone(), LABEL)
    })
}

/// Builds Bitbucket's JSON error envelope for a 404 from the source endpoint.
fn src_error(message: &str, commit: Option<&str>) -> Response<Body> {
    let data = commit.map(|commit| serde_json::json!({"shas": [commit]}));
    let mut error = serde_json::json!({"message": message});
    let mut body = serde_json::json!({"type": "error"});
    if let Some(data) = data {
        error["data"] = data.clone();
        body["data"] = data;
    }
    body["error"] = error;
    json_response(
        StatusCode::NOT_FOUND,
        &body,
        "GET /repositories/{workspace}/{repo_slug}/src/{commit}/{path}",
    )
}

/// Lists workspace pull requests authored by the selected account.
fn list_workspace_pulls(
    state: &StateHandle<State>,
    uri: &Uri,
    headers: &HeaderMap,
    query: &BTreeMap<String, String>,
    workspace: &str,
    selected_user: &str,
) -> Response<Body> {
    let pulls = state.read(|state| {
        state
            .repositories
            .values()
            .filter(|repo| repo.full_name.starts_with(&format!("{workspace}/")))
            .flat_map(|repo| repo.pull_requests.iter())
            .filter(|pull| pull.author.account_id == selected_user)
            .cloned()
            .collect::<Vec<_>>()
    });
    page_response(
        uri,
        headers,
        query,
        filter_pull_requests(pulls, query),
        "GET /workspaces/{workspace}/pullrequests/{selected_user}",
    )
}

/// Applies Bitbucket `q` terms and the supported state query parameter.
fn filter_pull_requests(
    mut pulls: Vec<PullRequest>,
    query: &BTreeMap<String, String>,
) -> Vec<PullRequest> {
    let q = query.get("q").map(String::as_str).unwrap_or("");
    if let Some(value) = extract_query(q, "state=") {
        let value = strip_quotes(value);
        pulls.retain(|pull| pull.state == value);
    } else if let Some(value) = query.get("state") {
        pulls.retain(|pull| &pull.state == value);
    }
    if let Some(value) = extract_query(q, "source.branch.name=") {
        let value = strip_quotes(value);
        pulls.retain(|pull| pull.source.branch.name == value);
    }
    if let Some(value) = extract_query(q, "author.account_id=") {
        let value = strip_quotes(value);
        pulls.retain(|pull| pull.author.account_id == value);
    }
    if let Some(value) = extract_query(q, "reviewers.account_id=") {
        let value = strip_quotes(value);
        pulls.retain(|pull| {
            pull.participants.iter().any(|participant| {
                participant.role == "REVIEWER" && participant.account_id == value
            })
        });
    }
    if let Some(value) = extract_query(q, "updated_on>=") {
        let value = strip_quotes(value);
        pulls.retain(|pull| pull.updated_on.as_str() >= value);
    }
    pulls
}

/// Builds the Bitbucket values envelope and absolute next-page URL.
fn page_response<T: Serialize>(
    uri: &Uri,
    headers: &HeaderMap,
    query: &BTreeMap<String, String>,
    values: Vec<T>,
    label: &str,
) -> Response<Body> {
    let page = query
        .get("page")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    let pagelen = query
        .get("pagelen")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100);
    let start = page.saturating_sub(1).saturating_mul(pagelen);
    let end = start.saturating_add(pagelen).min(values.len());
    let page_values = if start < values.len() {
        &values[start..end]
    } else {
        &values[0..0]
    };
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1");
    let base = format!("http://{host}{}", uri.path());
    let next = (start.saturating_add(pagelen) < values.len())
        .then(|| absolute_page_url(&base, query, page + 1));
    json_response(
        StatusCode::OK,
        &PageResponse {
            values: page_values,
            page,
            size: values.len(),
            pagelen,
            next,
        },
        label,
    )
}

/// Builds an absolute next-page URL while preserving the original query fields.
fn absolute_page_url(base: &str, query: &BTreeMap<String, String>, page: usize) -> String {
    let mut query = query.clone();
    query.insert("page".to_owned(), page.to_string());
    let query = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(query.iter())
        .finish();
    format!("{base}?{query}")
}

/// Returns one supported `q` expression value.
fn extract_query<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    let start = query.find(key)? + key.len();
    let rest = query[start..].trim_start();
    let len = rest.find(" AND ").unwrap_or(rest.len());
    Some(rest[..len].trim())
}

/// Removes a matching pair of double quotes from a query value.
fn strip_quotes(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

/// Parses a route PR number, rejecting nested path components.
fn pull_request_resource_id(suffix: &str) -> Option<u64> {
    let value = suffix.strip_prefix("/pullrequests/")?;
    (!value.contains('/')).then(|| value.parse().ok()).flatten()
}

/// Recognizes only the status resource path, leaving unknown nested paths for the 501 fallback.
fn pull_request_status_id(suffix: &str) -> Option<u64> {
    let value = suffix.strip_prefix("/pullrequests/")?;
    let (id, resource) = value.split_once('/')?;
    (resource == "statuses").then(|| id.parse().ok()).flatten()
}

/// Parses query parameters into an owned map.
fn query_map(query: Option<&str>) -> BTreeMap<String, String> {
    form_urlencoded::parse(query.unwrap_or_default().as_bytes())
        .into_owned()
        .collect()
}

/// Removes the provider API prefix before routing.
fn provider_path(path: &str) -> String {
    path.strip_prefix("/2.0").unwrap_or(path).to_owned()
}

/// Decodes a request body into its typed provider model.
fn decode_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|error| error.to_string())
}

/// Saves an ordered observation of the real HTTP request.
fn record_request(
    state: &StateHandle<State>,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: &[u8],
) {
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1");
    let url = format!("http://{host}{uri}");
    let headers = headers
        .iter()
        .filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_owned())))
        .collect();
    state.update(|state| {
        state.requests.push(RequestRecord {
            method: method.to_string(),
            url,
            headers,
            body: body.to_vec(),
        })
    });
}

/// Checks configured Basic credentials before handling provider operations.
fn authorized(state: &StateHandle<State>, headers: &HeaderMap) -> bool {
    state.read(|state| {
        let Some((username, password)) = &state.credentials else {
            return true;
        };
        let encoded = STANDARD.encode(format!("{username}:{password}"));
        headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            == Some(format!("Basic {encoded}").as_str())
    })
}

/// Applies the first matching response or transport fault rule.
async fn apply_faults(
    state: &StateHandle<State>,
    method: &Method,
    path: &str,
) -> Option<Response<Body>> {
    let mut action = None;
    let mut latency = Duration::ZERO;
    state.update(|state| {
        for fault in &mut state.faults {
            if fault
                .method
                .as_deref()
                .is_some_and(|expected| expected != method.as_str())
                || fault
                    .path
                    .as_deref()
                    .is_some_and(|expected| !path.contains(expected))
                || fault.remaining == 0
            {
                continue;
            }
            fault.matched += 1;
            if fault.nth.is_some_and(|nth| nth != fault.matched) {
                continue;
            }
            fault.remaining -= 1;
            state.applied_faults.push(crate::state::AppliedFault {
                method: method.to_string(),
                path: path.to_owned(),
                kind: fault.kind.clone(),
            });
            match fault.kind.as_str() {
                "timeout" => {
                    latency += Duration::from_secs_f64(
                        fault
                            .payload
                            .get("delay")
                            .and_then(Value::as_f64)
                            .unwrap_or(1.0)
                            .max(0.0),
                    )
                }
                "response" | "corrupt" | "drop" => {
                    action = Some(fault.clone());
                    break;
                }
                _ => {}
            }
        }
    });
    if !latency.is_zero() {
        tokio::time::sleep(latency).await;
    }
    action.and_then(|fault| match fault.kind.as_str() {
        "drop" => ninot_common::faults::drop_connection(),
        "corrupt" => {
            let mode = fault
                .payload
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("truncate");
            let body: &[u8] = match mode {
                "empty" => b"",
                "not-json" => b"<html>",
                "null" => b"null",
                _ => b"{\"partial\": tru",
            };
            Some(labeled_response(
                StatusCode::OK,
                "application/json",
                body.to_vec(),
                "fault/corrupt",
            ))
        }
        "response" => {
            let status = fault
                .payload
                .get("status_code")
                .and_then(Value::as_u64)
                .and_then(|status| u16::try_from(status).ok())
                .unwrap_or(500);
            let body = if let Some(value) = fault.payload.get("json") {
                serde_json::to_vec(value).unwrap_or_default()
            } else {
                fault
                    .payload
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("injected failure")
                    .as_bytes()
                    .to_vec()
            };
            let mut response = labeled_response(
                StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                "application/json",
                body,
                "fault/response",
            );
            if let Some(retry_after) = fault
                .payload
                .get("headers")
                .and_then(Value::as_object)
                .and_then(|headers| headers.get("Retry-After"))
                .and_then(Value::as_str)
                .and_then(|value| HeaderValue::from_str(value).ok())
            {
                response.headers_mut().insert(RETRY_AFTER, retry_after);
            }
            Some(response)
        }
        _ => None,
    })
}

/// Builds the provider error response body.
#[derive(Serialize)]
/// Bitbucket's provider-shaped error message response.
struct ErrorMessage {
    /// Human-readable error description.
    message: String,
}

/// Invalid JSON bodies use Bitbucket's `error` response field.
#[derive(Serialize)]
struct InputError {
    /// Input validation error.
    error: String,
}

/// A typed page envelope for list endpoints.
#[derive(Serialize)]
struct PageResponse<'a, T> {
    /// Items included in this page.
    values: &'a [T],
    /// One-based page number.
    page: usize,
    /// Total matching item count.
    size: usize,
    /// Requested maximum page length.
    pagelen: usize,
    /// Absolute URL of the next page when more results remain.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<String>,
}

/// Builds a labeled HTTP 404 response.
fn not_found(message: &str, label: &str) -> Response<Body> {
    labeled_response(
        StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        message.as_bytes().to_vec(),
        label,
    )
}

/// Serializes a JSON model into a labeled response.
fn json_response<T: Serialize>(status: StatusCode, value: &T, label: &str) -> Response<Body> {
    let body = serde_json::to_vec(value).expect("typed Bitbucket responses serialize");
    labeled_response(status, "application/json", body, label)
}

/// Attaches the route label and content type to a response.
fn labeled_response(
    status: StatusCode,
    content_type: &str,
    body: Vec<u8>,
    label: &str,
) -> Response<Body> {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    if let Ok(content_type) = HeaderValue::from_str(content_type) {
        response.headers_mut().insert(CONTENT_TYPE, content_type);
    }
    response.extensions_mut().insert(Label(label.to_owned()));
    response
}
