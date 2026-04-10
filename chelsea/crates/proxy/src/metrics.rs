//! Metrics collection for proxy connections
//!
//! This module tracks:
//! - Total SSH connections
//! - Active SSH connections
//! - Connection errors by type
//! - Total HTTP connections
//! - Total WebSocket connections
//!
//! Metrics are exposed via a simple in-memory counter system.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global metrics collector
#[derive(Clone)]
pub struct Metrics {
    inner: Arc<MetricsInner>,
}

struct MetricsInner {
    // SSH connection metrics
    ssh_connections_total: AtomicU64,
    ssh_connections_active: AtomicU64,
    ssh_errors_tls_handshake: AtomicU64,
    ssh_errors_backend_connection: AtomicU64,
    ssh_errors_vm_not_found: AtomicU64,
    ssh_errors_other: AtomicU64,

    // HTTP connection metrics
    http_connections_total: AtomicU64,
    http_connections_active: AtomicU64,

    // WebSocket connection metrics
    websocket_connections_total: AtomicU64,
    websocket_connections_active: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(MetricsInner {
                ssh_connections_total: AtomicU64::new(0),
                ssh_connections_active: AtomicU64::new(0),
                ssh_errors_tls_handshake: AtomicU64::new(0),
                ssh_errors_backend_connection: AtomicU64::new(0),
                ssh_errors_vm_not_found: AtomicU64::new(0),
                ssh_errors_other: AtomicU64::new(0),
                http_connections_total: AtomicU64::new(0),
                http_connections_active: AtomicU64::new(0),
                websocket_connections_total: AtomicU64::new(0),
                websocket_connections_active: AtomicU64::new(0),
            }),
        }
    }

    // SSH connection tracking
    pub fn ssh_connection_started(&self) {
        self.inner
            .ssh_connections_total
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .ssh_connections_active
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn ssh_connection_ended(&self) {
        self.inner
            .ssh_connections_active
            .fetch_sub(1, Ordering::Relaxed);
    }

    // HTTP connection tracking
    pub fn http_connection_started(&self) {
        self.inner
            .http_connections_total
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .http_connections_active
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn http_connection_ended(&self) {
        self.inner
            .http_connections_active
            .fetch_sub(1, Ordering::Relaxed);
    }

    // WebSocket connection tracking
    pub fn websocket_connection_started(&self) {
        self.inner
            .websocket_connections_total
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .websocket_connections_active
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn websocket_connection_ended(&self) {
        self.inner
            .websocket_connections_active
            .fetch_sub(1, Ordering::Relaxed);
    }

    // Get current metrics
    pub fn ssh_connections_total(&self) -> u64 {
        self.inner.ssh_connections_total.load(Ordering::Relaxed)
    }

    pub fn ssh_connections_active(&self) -> u64 {
        self.inner.ssh_connections_active.load(Ordering::Relaxed)
    }

    pub fn http_connections_total(&self) -> u64 {
        self.inner.http_connections_total.load(Ordering::Relaxed)
    }

    pub fn http_connections_active(&self) -> u64 {
        self.inner.http_connections_active.load(Ordering::Relaxed)
    }

    pub fn websocket_connections_total(&self) -> u64 {
        self.inner
            .websocket_connections_total
            .load(Ordering::Relaxed)
    }

    pub fn websocket_connections_active(&self) -> u64 {
        self.inner
            .websocket_connections_active
            .load(Ordering::Relaxed)
    }

    /// Get detailed metrics as JSON-like string
    pub fn detailed(&self) -> String {
        format!(
            r#"{{
  "ssh": {{
    "connections_total": {},
    "connections_active": {},
    "errors": {{
      "tls_handshake": {},
      "backend_connection": {},
      "vm_not_found": {},
      "other": {}
    }}
  }},
  "http": {{
    "connections_total": {},
    "connections_active": {}
  }},
  "websocket": {{
    "connections_total": {},
    "connections_active": {}
  }}
}}"#,
            self.ssh_connections_total(),
            self.ssh_connections_active(),
            self.inner.ssh_errors_tls_handshake.load(Ordering::Relaxed),
            self.inner
                .ssh_errors_backend_connection
                .load(Ordering::Relaxed),
            self.inner.ssh_errors_vm_not_found.load(Ordering::Relaxed),
            self.inner.ssh_errors_other.load(Ordering::Relaxed),
            self.http_connections_total(),
            self.http_connections_active(),
            self.websocket_connections_total(),
            self.websocket_connections_active(),
        )
    }
}

/// RAII guard for tracking active connections
///
/// Automatically increments active count on creation and decrements on drop
pub struct ConnectionGuard {
    metrics: Metrics,
    connection_type: ConnectionType,
}

pub enum ConnectionType {
    Ssh,
    Http,
    WebSocket,
}

impl ConnectionGuard {
    pub fn new(metrics: Metrics, connection_type: ConnectionType) -> Self {
        match connection_type {
            ConnectionType::Ssh => metrics.ssh_connection_started(),
            ConnectionType::Http => metrics.http_connection_started(),
            ConnectionType::WebSocket => metrics.websocket_connection_started(),
        }
        Self {
            metrics,
            connection_type,
        }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        match self.connection_type {
            ConnectionType::Ssh => self.metrics.ssh_connection_ended(),
            ConnectionType::Http => self.metrics.http_connection_ended(),
            ConnectionType::WebSocket => self.metrics.websocket_connection_ended(),
        }
    }
}
