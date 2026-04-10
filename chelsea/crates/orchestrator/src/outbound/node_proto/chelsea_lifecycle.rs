use std::time::Duration;

use dto_lib::chelsea_server2::images::{ImageStatusResponse, ListImagesResponse};
use dto_lib::chelsea_server2::vm::{
    VmCommitRequest, VmCommitResponse, VmCreateRequest, VmExecLogQuery, VmExecLogResponse,
    VmExecRequest, VmExecResponse, VmExecStreamAttachRequest, VmFromCommitRequest,
    VmReadFileResponse, VmResizeDiskRequest, VmSshKeyResponse, VmUpdateStateEnum,
    VmUpdateStateRequest, VmWakeRequest, VmWriteFileRequest,
};
use reqwest::StatusCode;
use tokio::time::timeout;
use uuid::Uuid;
use vers_config::VersConfig;

use crate::{db::NodeEntity, outbound::node_proto::HttpError};

use super::{ChelseaProto, RequestBuilderExt};

impl ChelseaProto {
    #[tracing::instrument(skip_all, fields(node_id = %node.id()))]
    pub async fn new_vm(
        &self,
        node: &NodeEntity,
        request: VmCreateRequest,
        wait_boot: bool,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        {
            let _span = tracing::info_span!("proto.ensure_node_wg").entered();
            self.ensure_node_wg(node)?;
        }
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/vm/new"))
            .query(&[("wait_boot", &wait_boot.to_string())])
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        let res = match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            // no timeout and good request
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => Ok(()),
                _ => {
                    let status = response.status();
                    let body = response.text().await;
                    tracing::error!(?status, ?body, "invalid chelsea response");
                    Err(HttpError::from_status_and_body_result(&status, body))
                }
            },

            // no timeout and error request
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, "new_root_vm, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            // timeout
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, "timeout 'new_root_vm' chelsea");
                Err(HttpError::Timeout)
            }
        };

        res
    }

    pub async fn vm_commit(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        commit_id: Uuid,
        keep_paused: bool,
        skip_wait_boot: bool,
        request_id: Option<&str>,
    ) -> Result<VmCommitResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/vm/{vm_id}/commit"))
            .query(&[
                ("keep_paused", &keep_paused.to_string()),
                ("skip_wait_boot", &skip_wait_boot.to_string()),
            ])
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&VmCommitRequest {
                commit_id: Some(commit_id),
                name: None,
                description: None,
            })
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            // no timeout and good request
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => match response.json::<VmCommitResponse>().await {
                    Ok(body) => Ok(body),
                    Err(_err) => Err(HttpError::BodyUnparsable),
                },
                _ => Err(HttpError::from_response(response).await),
            },

            // no timeout and error request
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_commit, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            // timeout
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_commit' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn vm_from_commit(
        &self,
        node: &NodeEntity,
        request: VmFromCommitRequest,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/vm/from_commit"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            // no timeout and good request
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => Ok(()),
                _ => Err(HttpError::from_response(response).await),
            },

            // no timeout and error request
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, ?request, "vm_from_commit, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            // timeout
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, ?request, "timeout 'vm_from_commit' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn vm_update_state(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        state: VmUpdateStateEnum,
        skip_wait_boot: bool,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let request = VmUpdateStateRequest { state };
        let http_fut = self
            .http
            .patch(&format!("http://{endpoint}/api/vm/{vm_id}/state"))
            .query(&[("skip_wait_boot", &skip_wait_boot.to_string())])
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            // no timeout and good request
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => Ok(()),
                _ => Err(HttpError::from_response(response).await),
            },

            // no timeout and error request
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_update_state, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            // timeout
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_update_state' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn vm_resize_disk(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        request: VmResizeDiskRequest,
        skip_wait_boot: bool,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .patch(&format!("http://{endpoint}/api/vm/{vm_id}/disk"))
            .query(&[("skip_wait_boot", &skip_wait_boot.to_string())])
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => Ok(()),
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_resize_disk, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_resize_disk' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn ssh_key(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        request_id: Option<&str>,
    ) -> Result<VmSshKeyResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .get(&format!("http://{endpoint}/api/vm/{vm_id}/ssh_key"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            // no timeout and good request
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK | StatusCode::INTERNAL_SERVER_ERROR => {
                    match response.json::<VmSshKeyResponse>().await {
                        Ok(body) => Ok(body),
                        Err(_err) => Err(HttpError::BodyUnparsable),
                    }
                }
                _ => Err(HttpError::from_response(response).await),
            },

            // no timeout and error request
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "ssh_key, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            // timeout
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'ssh_key' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn vm_status(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        request_id: Option<&str>,
    ) -> Result<dto_lib::chelsea_server2::vm::VmStatusResponse, HttpError> {
        // Check for test mock first
        #[cfg(any(test, feature = "integration-tests"))]
        if let Some(result) = super::mock::try_mock_vm_status(vm_id) {
            return result;
        }

        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .get(&format!("http://{endpoint}/api/vm/{vm_id}"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            // no timeout and good request
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => {
                    match response
                        .json::<dto_lib::chelsea_server2::vm::VmStatusResponse>()
                        .await
                    {
                        Ok(body) => Ok(body),
                        Err(_err) => Err(HttpError::BodyUnparsable),
                    }
                }
                _ => Err(HttpError::from_response(response).await),
            },

            // no timeout and error request
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_status, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            // timeout
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_status' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn vm_exec(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        request: VmExecRequest,
        request_id: Option<&str>,
    ) -> Result<VmExecResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/vm/{vm_id}/exec"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => match response.json::<VmExecResponse>().await {
                    Ok(body) => Ok(body),
                    Err(_err) => Err(HttpError::BodyUnparsable),
                },
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_exec, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_exec' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn vm_write_file(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        request: VmWriteFileRequest,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .put(&format!("http://{endpoint}/api/vm/{vm_id}/files"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => Ok(()),
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_write_file, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_write_file' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn vm_read_file(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        path: &str,
        request_id: Option<&str>,
    ) -> Result<VmReadFileResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .get(&format!("http://{endpoint}/api/vm/{vm_id}/files"))
            .query(&[("path", path)])
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => match response.json::<VmReadFileResponse>().await {
                    Ok(body) => Ok(body),
                    Err(_err) => Err(HttpError::BodyUnparsable),
                },
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_read_file, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_read_file' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    pub async fn vm_exec_logs(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        query: &VmExecLogQuery,
        request_id: Option<&str>,
    ) -> Result<VmExecLogResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();

        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(offset) = query.offset {
            params.push(("offset", offset.to_string()));
        }
        if let Some(max) = query.max_entries {
            params.push(("max_entries", max.to_string()));
        }
        if let Some(ref stream) = query.stream {
            let s = serde_json::to_value(stream)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            params.push(("stream", s));
        }

        let http_fut = self
            .http
            .get(&format!("http://{endpoint}/api/vm/{vm_id}/exec/logs"))
            .query(&params)
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => match response.json::<VmExecLogResponse>().await {
                    Ok(body) => Ok(body),
                    Err(_err) => Err(HttpError::BodyUnparsable),
                },
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_exec_logs, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_exec_logs' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    /// Start a streaming exec session. Returns the raw reqwest::Response whose
    /// body is an NDJSON byte stream of `VmExecStreamEvent` lines.
    pub async fn vm_exec_stream(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        request: VmExecRequest,
        request_id: Option<&str>,
    ) -> Result<reqwest::Response, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/vm/{vm_id}/exec/stream"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        // No timeout — streaming exec is long-lived by design
        match http_fut.await {
            Ok(response) => match response.status() {
                StatusCode::OK => Ok(response),
                _ => Err(HttpError::from_response(response).await),
            },
            Err(err) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_exec_stream, error");
                Err(HttpError::ConnectionRefused)
            }
        }
    }

    /// Reattach to a running exec stream. Returns the raw reqwest::Response whose
    /// body is an NDJSON byte stream replaying from the given cursor.
    pub async fn vm_exec_stream_attach(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        request: VmExecStreamAttachRequest,
        request_id: Option<&str>,
    ) -> Result<reqwest::Response, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!(
                "http://{endpoint}/api/vm/{vm_id}/exec/stream/attach"
            ))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        // No timeout — streaming reattach is long-lived by design
        match http_fut.await {
            Ok(response) => match response.status() {
                StatusCode::OK => Ok(response),
                _ => Err(HttpError::from_response(response).await),
            },
            Err(err) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_exec_stream_attach, error");
                Err(HttpError::ConnectionRefused)
            }
        }
    }

    pub async fn delete_vm(
        &self,
        node: &NodeEntity,
        vm_id: &Uuid,
        skip_wait_boot: bool,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .delete(&format!("http://{endpoint}/api/vm/{vm_id}"))
            .query(&[("skip_wait_boot", &skip_wait_boot.to_string())])
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            // no timeout and good request
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => Ok(()),
                _ => Err(HttpError::from_response(response).await),
            },

            // no timeout and error request
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "delete_vm, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            // timeout
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'delete_vm' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    /// List all base images on a Chelsea node
    pub async fn list_images(
        &self,
        node: &NodeEntity,
        request_id: Option<&str>,
    ) -> Result<ListImagesResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .get(&format!("http://{endpoint}/api/images"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => match response.json::<ListImagesResponse>().await {
                    Ok(body) => Ok(body),
                    Err(_err) => Err(HttpError::BodyUnparsable),
                },
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, "list_images, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, "timeout 'list_images' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    /// Create a base image on a Chelsea node
    pub async fn create_image(
        &self,
        node: &NodeEntity,
        request: dto_lib::chelsea_server2::images::CreateBaseImageRequest,
        request_id: Option<&str>,
    ) -> Result<dto_lib::chelsea_server2::images::CreateBaseImageResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/images/create"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => {
                    match response
                        .json::<dto_lib::chelsea_server2::images::CreateBaseImageResponse>()
                        .await
                    {
                        Ok(body) => Ok(body),
                        Err(_err) => Err(HttpError::BodyUnparsable),
                    }
                }
                StatusCode::CONFLICT => Err(HttpError::NonSuccessStatusCode(
                    409,
                    "Image already exists".to_string(),
                )),
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, image_name = %request.image_name, "create_image, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, image_name = %request.image_name, "timeout 'create_image' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    /// Get the status of a base image on a Chelsea node
    pub async fn image_status(
        &self,
        node: &NodeEntity,
        image_name: &str,
        request_id: Option<&str>,
    ) -> Result<ImageStatusResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        // URL-encode the image name since it may contain slashes (namespace/image format)
        let encoded_image_name = urlencoding::encode(image_name);
        let http_fut = self
            .http
            .get(&format!(
                "http://{endpoint}/api/images/{encoded_image_name}/status"
            ))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => match response.json::<ImageStatusResponse>().await {
                    Ok(body) => Ok(body),
                    Err(_err) => Err(HttpError::BodyUnparsable),
                },
                StatusCode::NOT_FOUND => Err(HttpError::NonSuccessStatusCode(
                    404,
                    "Image not found".to_string(),
                )),
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, image_name = %image_name, "image_status, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, image_name = %image_name, "timeout 'image_status' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    /// Upload a tarball to create a base image on a Chelsea node.
    /// This streams the file to Chelsea's upload endpoint.
    pub async fn upload_image(
        &self,
        node: &NodeEntity,
        image_name: &str,
        size_mib: Option<u32>,
        body: reqwest::Body,
        content_length: u64,
        request_id: Option<&str>,
    ) -> Result<dto_lib::chelsea_server2::images::CreateBaseImageResponse, HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();

        // Build query params
        let mut query = vec![("image_name", image_name.to_string())];
        if let Some(size) = size_mib {
            query.push(("size_mib", size.to_string()));
        }

        // Use a longer timeout for uploads (10 minutes max)
        let upload_timeout =
            std::time::Duration::from_secs(VersConfig::orchestrator().file_upload_timeout_secs);

        // Create the multipart form
        let part = reqwest::multipart::Part::stream_with_length(body, content_length)
            .file_name("upload.tar")
            .mime_str("application/x-tar")
            .map_err(|e| {
                HttpError::NonSuccessStatusCode(500, format!("Failed to set mime type: {}", e))
            })?;

        let form = reqwest::multipart::Form::new().part("file", part);

        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/images/upload"))
            .query(&query)
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .multipart(form)
            .send();

        match timeout(upload_timeout, http_fut).await {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => {
                    match response
                        .json::<dto_lib::chelsea_server2::images::CreateBaseImageResponse>()
                        .await
                    {
                        Ok(body) => Ok(body),
                        Err(_err) => Err(HttpError::BodyUnparsable),
                    }
                }
                StatusCode::CONFLICT => Err(HttpError::NonSuccessStatusCode(
                    409,
                    "Image already exists".to_string(),
                )),
                StatusCode::PAYLOAD_TOO_LARGE => Err(HttpError::NonSuccessStatusCode(
                    413,
                    "Upload too large".to_string(),
                )),
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, image_name = %image_name, "upload_image, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, image_name = %image_name, "timeout 'upload_image' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    /// Sleep a VM: snapshots it and kills its process on the node.
    ///
    /// After this call succeeds, Chelsea has created a `sleep_snapshot` for the VM.
    /// The orchestrator is responsible for setting `node_id = NULL` in the database.
    pub async fn vm_sleep(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        skip_wait_boot: bool,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/vm/{vm_id}/sleep"))
            .query(&[("skip_wait_boot", &skip_wait_boot.to_string())])
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => Ok(()),
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_sleep, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_sleep' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    /// Wake a sleeping VM: restores it from its sleep snapshot on the given node.
    ///
    /// The caller must supply a `VmWakeRequest` with the WireGuard configuration to use.
    /// After this call succeeds, the orchestrator is responsible for setting `node_id` in the
    /// database to the node this VM was woken on.
    pub async fn vm_wake(
        &self,
        node: &NodeEntity,
        vm_id: Uuid,
        request: VmWakeRequest,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        let http_fut = self
            .http
            .post(&format!("http://{endpoint}/api/vm/{vm_id}/wake"))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .json(&request)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::OK => Ok(()),
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, vm_id = %vm_id, "vm_wake, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, vm_id = %vm_id, "timeout 'vm_wake' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }

    /// Delete a base image from a Chelsea node.
    ///
    /// Returns Ok(()) on success (204 No Content from Chelsea).
    /// Returns Err with appropriate error codes for:
    /// - 404: Image not found
    /// - 409: Image has child clones or is in use
    /// - Other errors
    pub async fn delete_image(
        &self,
        node: &NodeEntity,
        image_name: &str,
        request_id: Option<&str>,
    ) -> Result<(), HttpError> {
        self.ensure_node_wg(node)?;
        let endpoint = node.server_addr();
        // URL-encode the image name since it may contain slashes (namespace/image format)
        let encoded_image_name = urlencoding::encode(image_name);
        let http_fut = self
            .http
            .delete(&format!(
                "http://{endpoint}/api/images/{encoded_image_name}"
            ))
            .header("User-Agent", Self::USER_AGENT)
            .maybe_request_id(request_id)
            .send();

        match timeout(
            Duration::from_secs(VersConfig::orchestrator().node_proto_request_timeout_secs),
            http_fut,
        )
        .await
        {
            Ok(Ok(response)) => match response.status() {
                StatusCode::NO_CONTENT => Ok(()),
                StatusCode::NOT_FOUND => Err(HttpError::NonSuccessStatusCode(
                    404,
                    "Image not found".to_string(),
                )),
                StatusCode::CONFLICT => {
                    let body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Image is in use".to_string());
                    Err(HttpError::NonSuccessStatusCode(409, body))
                }
                _ => Err(HttpError::from_response(response).await),
            },
            Ok(Err(err)) => {
                tracing::error!(err = ?err, chelsea_ip = ?endpoint, image_name = %image_name, "delete_image, non-timeout error");
                Err(HttpError::ConnectionRefused)
            }
            Err(_) => {
                tracing::warn!(chelsea_ip = ?endpoint, image_name = %image_name, "timeout 'delete_image' chelsea");
                Err(HttpError::Timeout)
            }
        }
    }
}
