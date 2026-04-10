//! TestClient - Request Builder Abstraction for Orchestrator Route Testing
//!
//! This module provides a high-level API for making requests to orchestrator routes in tests.

use axum::{
    body::Body,
    http::{Method, Request, Response, StatusCode},
};
use dto_lib::chelsea_server2::commits::ListCommitsResponse;
use dto_lib::chelsea_server2::vm::{
    VmCommitResponse, VmExecLogQuery, VmExecLogResponse, VmExecRequest, VmExecResponse,
    VmExecStreamAttachRequest, VmSshKeyResponse,
};
use dto_lib::orchestrator::commit_repository::{
    CreateRepoTagRequest, CreateRepoTagResponse, CreateRepositoryRequest, CreateRepositoryResponse,
    ForkRepositoryRequest, ForkRepositoryResponse, ListPublicRepositoriesResponse,
    ListRepoTagsResponse, ListRepositoriesResponse, PublicRepositoryInfo, RepoTagInfo,
    RepositoryInfo, SetRepositoryVisibilityRequest,
};
use orchestrator::{
    action::VM,
    inbound::routes::controlplane::vm::{FromCommitVmRequest, NewRootRequest, NewVmResponse},
};
use serde::de::DeserializeOwned;
use tower::ServiceExt;
use uuid::Uuid;

/// Error type for TestClient operations
#[derive(Debug, PartialEq)]
pub enum TestError {
    RequestFailed(String),
    DeserializationFailed {
        body: String,
        error: String,
    },
    SerializationFailed(String),
    FailedStatusCodeAssert {
        expected_status: StatusCode,
        got_status: StatusCode,
    },
    Unauthorized,
    ResourceOrRouteNotFound,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::RequestFailed(msg) => write!(f, "Request failed: {}", msg),
            TestError::DeserializationFailed { body, error } => {
                write!(f, "Deserialization failed: {} (body: {})", error, body)
            }
            TestError::SerializationFailed(msg) => write!(f, "Serialization failed: {}", msg),
            TestError::FailedStatusCodeAssert {
                expected_status,
                got_status,
            } => write!(
                f,
                "Failed status-code assert: Expected = {}, Got = {}",
                expected_status, got_status
            ),
            TestError::Unauthorized => write!(f, "TestError::Unauthorized"),
            TestError::ResourceOrRouteNotFound => {
                write!(f, "TestError::ResourceOrRouteNotFound")
            }
        }
    }
}

impl std::error::Error for TestError {}

/// TestClient provides a high-level API for making requests to orchestrator routes in tests
///
/// # Example
/// ```
/// let client = TestClient::new(routes)
///     .with_bearer(env.orch_apikey());
///
/// let (status, response) = client.new_root("cluster_id", vm_config).await?;
/// assert_eq!(status, StatusCode::CREATED);
/// ```
pub struct TestClient {
    routes: axum::Router,
    bearer_token: Option<String>,
}

impl TestClient {
    /// Create a new TestClient without authentication
    pub fn new(routes: axum::Router) -> Self {
        Self {
            routes,
            bearer_token: None,
        }
    }

    /// Set the bearer token for authentication (builder pattern)
    pub fn with_bearer(mut self, token: impl Into<String>) -> Self {
        self.bearer_token = Some(token.into());
        self
    }

    // ========================================================================
    // Internal Helper Methods
    // ========================================================================

    /// Build a request with appropriate headers
    fn build_request(
        &self,
        method: Method,
        uri: &str,
        body: impl Into<Body>,
        ct: Option<mime::Mime>,
    ) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);

        // Add Authorization header if bearer token is set
        if let Some(token) = &self.bearer_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        // Add Content-Type for JSON bodies when requested
        if let Some(_typ) = ct {
            builder = builder.header("Content-Type", _typ.to_string());
        }

        builder.body(body.into()).unwrap()
    }

    /// Execute a request and parse the response body
    async fn execute_and_parse<R: DeserializeOwned>(
        &self,
        response: Response<Body>,
    ) -> Result<R, TestError> {
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("Unknown error")
            .to_vec();

        let parsed =
            serde_json::from_slice(&body_bytes).map_err(|e| TestError::DeserializationFailed {
                body: String::from_utf8_lossy(&body_bytes).to_string(),
                error: e.to_string(),
            })?;

        Ok(parsed)
    }

    /// Execute a request without parsing the response body
    async fn execute_no_parse(&self, req: Request<Body>) -> Result<Response<Body>, TestError> {
        let response = self.routes.clone().oneshot(req).await.expect("infallible");

        Ok(response)
    }

    /// Assert that the status code matches expected value based on authentication
    fn assert_status(
        &self,
        actual: StatusCode,
        expected_when_authed: StatusCode,
    ) -> Result<(), TestError> {
        if actual == expected_when_authed {
            Ok(())
        } else if actual == StatusCode::NOT_FOUND {
            Err(TestError::ResourceOrRouteNotFound)
        } else if actual == StatusCode::UNAUTHORIZED {
            Err(TestError::Unauthorized)
        } else {
            Err(TestError::FailedStatusCodeAssert {
                expected_status: expected_when_authed,
                got_status: actual,
            })
        }
    }

    // ========================================================================
    // Route-Specific Methods
    // ========================================================================

    /// Create a new root VM
    ///
    /// POST /api/v1/vm/new_root
    pub async fn new_root(&self, request_body: NewRootRequest) -> Result<NewVmResponse, TestError> {
        let body_json = serde_json::to_string(&request_body)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;

        let req = self.build_request(
            Method::POST,
            "/api/v1/vm/new_root",
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::CREATED)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    /// List all VMs on a node
    ///
    /// GET /api/v1/node/{node_id}/vms
    pub async fn vm_list(&self) -> Result<Vec<VM>, TestError> {
        let req = self.build_request(Method::GET, "/api/v1/vms", Body::empty(), None);

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::OK)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    /// Delete a VM
    ///
    /// DELETE /api/v1/vm/{vm_id}
    pub async fn vm_delete(&self, vm_id: &str) -> Result<(), TestError> {
        let uri = format!("/api/v1/vm/{}", vm_id);
        let req = self.build_request(Method::DELETE, &uri, Body::empty(), None);

        let res = self.execute_no_parse(req).await?;
        self.assert_status(res.status(), StatusCode::OK)?;
        Ok(())
    }

    /// Get ssh key
    /// GET /api/v1/vm/{vm_id}/ssh_key
    pub async fn ssh_key(&self, vm_id: &str) -> Result<VmSshKeyResponse, TestError> {
        let uri = format!("/api/v1/vm/{}/ssh_key", vm_id);
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");
        self.assert_status(response.status(), StatusCode::OK)?;
        let res = self.execute_and_parse(response).await?;
        Ok(res)
    }

    /// Commit a VM
    ///
    /// POST /api/v1/vm/{vm_id}/commit
    pub async fn vm_commit(&self, vm_id: &str) -> Result<VmCommitResponse, TestError> {
        let uri = format!("/api/v1/vm/{}/commit", vm_id);
        let req = self.build_request(Method::POST, &uri, Body::empty(), None);

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::CREATED)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    /// Create a VM from a commit
    ///
    /// POST /api/v1/vm/from_commit
    pub async fn vm_from_commit(&self, commit_id: Uuid) -> Result<NewVmResponse, TestError> {
        let request_body = FromCommitVmRequest::CommitId(commit_id);

        let body_json = serde_json::to_string(&request_body)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;

        let req = self.build_request(
            Method::POST,
            "/api/v1/vm/from_commit",
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::CREATED)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    /// List commits
    ///
    /// GET /api/v1/commits
    pub async fn list_commits(
        &self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<ListCommitsResponse, TestError> {
        let mut uri = "/api/v1/commits".to_string();
        let mut params = Vec::new();
        if let Some(l) = limit {
            params.push(format!("limit={l}"));
        }
        if let Some(o) = offset {
            params.push(format!("offset={o}"));
        }
        if !params.is_empty() {
            uri = format!("{uri}?{}", params.join("&"));
        }

        let req = self.build_request(Method::GET, &uri, Body::empty(), None);

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::OK)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    /// Branch a VM
    ///
    /// POST /api/v1/vm/{vm_id}/branch
    pub async fn vm_branch(&self, vm_id: &str) -> Result<NewVmResponse, TestError> {
        let uri = format!("/api/v1/vm/{}/branch", vm_id);
        let req = self.build_request(Method::POST, &uri, Body::empty(), None);

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::CREATED)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    // ========================================================================
    // Exec Route Methods
    // ========================================================================

    /// Execute a command in a VM
    ///
    /// POST /api/v1/vm/{vm_id}/exec
    pub async fn vm_exec(
        &self,
        vm_id: &str,
        request: VmExecRequest,
    ) -> Result<VmExecResponse, TestError> {
        let body_json = serde_json::to_string(&request)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;
        let uri = format!("/api/v1/vm/{}/exec", vm_id);
        let req = self.build_request(
            Method::POST,
            &uri,
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Get exec logs for a VM
    ///
    /// GET /api/v1/vm/{vm_id}/logs
    pub async fn vm_logs(
        &self,
        vm_id: &str,
        query: &VmExecLogQuery,
    ) -> Result<VmExecLogResponse, TestError> {
        let mut uri = format!("/api/v1/vm/{}/logs", vm_id);
        let mut params = Vec::new();
        if let Some(offset) = query.offset {
            params.push(format!("offset={offset}"));
        }
        if let Some(max) = query.max_entries {
            params.push(format!("max_entries={max}"));
        }
        if !params.is_empty() {
            uri = format!("{uri}?{}", params.join("&"));
        }
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Start a streaming exec session (returns raw response for stream inspection)
    ///
    /// POST /api/v1/vm/{vm_id}/exec/stream
    pub async fn vm_exec_stream(
        &self,
        vm_id: &str,
        request: VmExecRequest,
    ) -> Result<Response<Body>, TestError> {
        let body_json = serde_json::to_string(&request)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;
        let uri = format!("/api/v1/vm/{}/exec/stream", vm_id);
        let req = self.build_request(
            Method::POST,
            &uri,
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );
        self.execute_no_parse(req).await
    }

    /// Reattach to a running exec stream (returns raw response for stream inspection)
    ///
    /// POST /api/v1/vm/{vm_id}/exec/stream/attach
    pub async fn vm_exec_stream_attach(
        &self,
        vm_id: &str,
        request: VmExecStreamAttachRequest,
    ) -> Result<Response<Body>, TestError> {
        let body_json = serde_json::to_string(&request)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;
        let uri = format!("/api/v1/vm/{}/exec/stream/attach", vm_id);
        let req = self.build_request(
            Method::POST,
            &uri,
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );
        self.execute_no_parse(req).await
    }

    // ========================================================================
    // Repository Route Methods
    // ========================================================================

    /// Create a repository
    ///
    /// POST /api/v1/repositories
    pub async fn create_repository(
        &self,
        name: &str,
        description: Option<&str>,
    ) -> Result<CreateRepositoryResponse, TestError> {
        let body = CreateRepositoryRequest {
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
        };
        let body_json = serde_json::to_string(&body)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;
        let req = self.build_request(
            Method::POST,
            "/api/v1/repositories",
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::CREATED)?;
        self.execute_and_parse(response).await
    }

    /// List repositories in the caller's org
    ///
    /// GET /api/v1/repositories
    pub async fn list_repositories(&self) -> Result<ListRepositoriesResponse, TestError> {
        let req = self.build_request(Method::GET, "/api/v1/repositories", Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Get a repository by name
    ///
    /// GET /api/v1/repositories/{repo_name}
    pub async fn get_repository(&self, repo_name: &str) -> Result<RepositoryInfo, TestError> {
        let uri = format!("/api/v1/repositories/{}", repo_name);
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Delete a repository
    ///
    /// DELETE /api/v1/repositories/{repo_name}
    pub async fn delete_repository(&self, repo_name: &str) -> Result<(), TestError> {
        let uri = format!("/api/v1/repositories/{}", repo_name);
        let req = self.build_request(Method::DELETE, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::NO_CONTENT)?;
        Ok(())
    }

    /// Set repository visibility
    ///
    /// PATCH /api/v1/repositories/{repo_name}/visibility
    pub async fn set_repository_visibility(
        &self,
        repo_name: &str,
        is_public: bool,
    ) -> Result<(), TestError> {
        let body = SetRepositoryVisibilityRequest { is_public };
        let body_json = serde_json::to_string(&body)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;
        let uri = format!("/api/v1/repositories/{}/visibility", repo_name);
        let req = self.build_request(
            Method::PATCH,
            &uri,
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::NO_CONTENT)?;
        Ok(())
    }

    /// Create a tag in a repository
    ///
    /// POST /api/v1/repositories/{repo_name}/tags
    pub async fn create_repo_tag(
        &self,
        repo_name: &str,
        tag_name: &str,
        commit_id: Uuid,
        description: Option<&str>,
    ) -> Result<CreateRepoTagResponse, TestError> {
        let body = CreateRepoTagRequest {
            tag_name: tag_name.to_string(),
            commit_id,
            description: description.map(|s| s.to_string()),
        };
        let body_json = serde_json::to_string(&body)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;
        let uri = format!("/api/v1/repositories/{}/tags", repo_name);
        let req = self.build_request(
            Method::POST,
            &uri,
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::CREATED)?;
        self.execute_and_parse(response).await
    }

    /// List tags in a repository
    ///
    /// GET /api/v1/repositories/{repo_name}/tags
    pub async fn list_repo_tags(&self, repo_name: &str) -> Result<ListRepoTagsResponse, TestError> {
        let uri = format!("/api/v1/repositories/{}/tags", repo_name);
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Get a specific tag in a repository
    ///
    /// GET /api/v1/repositories/{repo_name}/tags/{tag_name}
    pub async fn get_repo_tag(
        &self,
        repo_name: &str,
        tag_name: &str,
    ) -> Result<RepoTagInfo, TestError> {
        let uri = format!("/api/v1/repositories/{}/tags/{}", repo_name, tag_name);
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Delete a tag from a repository
    ///
    /// DELETE /api/v1/repositories/{repo_name}/tags/{tag_name}
    pub async fn delete_repo_tag(&self, repo_name: &str, tag_name: &str) -> Result<(), TestError> {
        let uri = format!("/api/v1/repositories/{}/tags/{}", repo_name, tag_name);
        let req = self.build_request(Method::DELETE, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::NO_CONTENT)?;
        Ok(())
    }

    // ── Public Repository Routes (no auth) ──────────────────────────────

    /// List all public repositories (no auth needed)
    ///
    /// GET /api/v1/public/repositories
    pub async fn list_public_repositories(
        &self,
    ) -> Result<ListPublicRepositoriesResponse, TestError> {
        let req = self.build_request(
            Method::GET,
            "/api/v1/public/repositories",
            Body::empty(),
            None,
        );
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Get a public repository (no auth needed)
    ///
    /// GET /api/v1/public/repositories/{org_name}/{repo_name}
    pub async fn get_public_repository(
        &self,
        org_name: &str,
        repo_name: &str,
    ) -> Result<PublicRepositoryInfo, TestError> {
        let uri = format!("/api/v1/public/repositories/{}/{}", org_name, repo_name);
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// List tags in a public repository (no auth needed)
    ///
    /// GET /api/v1/public/repositories/{org_name}/{repo_name}/tags
    pub async fn list_public_repo_tags(
        &self,
        org_name: &str,
        repo_name: &str,
    ) -> Result<ListRepoTagsResponse, TestError> {
        let uri = format!(
            "/api/v1/public/repositories/{}/{}/tags",
            org_name, repo_name
        );
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Get a specific tag in a public repository (no auth needed)
    ///
    /// GET /api/v1/public/repositories/{org_name}/{repo_name}/tags/{tag_name}
    pub async fn get_public_repo_tag(
        &self,
        org_name: &str,
        repo_name: &str,
        tag_name: &str,
    ) -> Result<RepoTagInfo, TestError> {
        let uri = format!(
            "/api/v1/public/repositories/{}/{}/tags/{}",
            org_name, repo_name, tag_name
        );
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Fork a public repository
    ///
    /// POST /api/v1/repositories/fork
    pub async fn fork_repository(
        &self,
        request: ForkRepositoryRequest,
    ) -> Result<ForkRepositoryResponse, TestError> {
        let body_json = serde_json::to_string(&request)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;
        let req = self.build_request(
            Method::POST,
            "/api/v1/repositories/fork",
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );
        let response = self.execute_no_parse(req).await?;
        self.assert_status(response.status(), StatusCode::CREATED)?;
        self.execute_and_parse(response).await
    }

    /// Raw request helper — returns status code + body for error testing
    pub async fn raw_request(
        &self,
        method: Method,
        uri: &str,
        body: Option<String>,
    ) -> Result<(StatusCode, String), TestError> {
        let body = match body {
            Some(b) => Body::from(b),
            None => Body::empty(),
        };
        let ct = if method == Method::POST || method == Method::PATCH || method == Method::PUT {
            Some(mime::APPLICATION_JSON)
        } else {
            None
        };
        let req = self.build_request(method, uri, body, ct);
        let response = self.execute_no_parse(req).await?;
        let status = response.status();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        Ok((status, String::from_utf8_lossy(&body_bytes).to_string()))
    }

    // ========================================================================
    // Domain Route Methods
    // ========================================================================

    /// Create a custom domain
    ///
    /// POST /api/v1/domains
    pub async fn create_domain(
        &self,
        vm_id: Uuid,
        domain: &str,
    ) -> Result<orchestrator::action::DomainResponse, TestError> {
        let body = serde_json::json!({
            "vm_id": vm_id,
            "domain": domain
        });

        let body_json = serde_json::to_string(&body)
            .map_err(|e| TestError::SerializationFailed(e.to_string()))?;

        let req = self.build_request(
            Method::POST,
            "/api/v1/domains",
            Body::from(body_json),
            Some(mime::APPLICATION_JSON),
        );

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::CREATED)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    /// Get a domain by ID
    ///
    /// GET /api/v1/domains/{domain_id}
    pub async fn get_domain(
        &self,
        domain_id: Uuid,
    ) -> Result<orchestrator::action::DomainResponse, TestError> {
        let uri = format!("/api/v1/domains/{}", domain_id);
        let req = self.build_request(Method::GET, &uri, Body::empty(), None);

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::OK)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    /// List domains
    ///
    /// GET /api/v1/domains
    pub async fn list_domains(
        &self,
        vm_id: Option<Uuid>,
    ) -> Result<Vec<orchestrator::action::DomainResponse>, TestError> {
        let mut uri = "/api/v1/domains".to_string();
        if let Some(vid) = vm_id {
            uri = format!("{uri}?vm_id={vid}");
        }

        let req = self.build_request(Method::GET, &uri, Body::empty(), None);

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::OK)?;
        let response = self.execute_and_parse(response).await?;
        Ok(response)
    }

    /// Delete a domain
    ///
    /// DELETE /api/v1/domains/{domain_id}
    pub async fn delete_domain(&self, domain_id: Uuid) -> Result<(), TestError> {
        let uri = format!("/api/v1/domains/{}", domain_id);
        let req = self.build_request(Method::DELETE, &uri, Body::empty(), None);

        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");

        self.assert_status(response.status(), StatusCode::OK)?;
        Ok(())
    }

    // ── Environment Variables ────────────────────────────────────────────

    /// List environment variables
    ///
    /// GET /api/v1/env_vars
    pub async fn list_env_vars(
        &self,
    ) -> Result<dto_lib::orchestrator::env_var::EnvVarsResponse, TestError> {
        let req = self.build_request(Method::GET, "/api/v1/env_vars", Body::empty(), None);
        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Set environment variables
    ///
    /// PUT /api/v1/env_vars
    pub async fn set_env_vars(
        &self,
        req_body: dto_lib::orchestrator::env_var::SetEnvVarsRequest,
    ) -> Result<dto_lib::orchestrator::env_var::EnvVarsResponse, TestError> {
        let body = serde_json::to_vec(&req_body).expect("serialize SetEnvVarsRequest");
        let req = self.build_request(
            Method::PUT,
            "/api/v1/env_vars",
            Body::from(body),
            Some(mime::APPLICATION_JSON),
        );
        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");
        self.assert_status(response.status(), StatusCode::OK)?;
        self.execute_and_parse(response).await
    }

    /// Delete an environment variable
    ///
    /// DELETE /api/v1/env_vars/{key}
    pub async fn delete_env_var(&self, key: &str) -> Result<(), TestError> {
        let uri = format!("/api/v1/env_vars/{}", key);
        let req = self.build_request(Method::DELETE, &uri, Body::empty(), None);
        let response = self
            .routes
            .clone()
            .oneshot(req)
            .await
            .expect("Is supposed to be infallible?");
        self.assert_status(response.status(), StatusCode::NO_CONTENT)?;
        Ok(())
    }
}
