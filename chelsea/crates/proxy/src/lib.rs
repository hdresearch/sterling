//! Proxy library - exposes SSH-over-TLS proxy functionality

pub mod hostname_validation;
pub mod idle_copy;

pub mod metrics;
pub mod pg;
pub mod protocol;

// Re-export main types for convenience
pub use metrics::Metrics;
pub use protocol::{BufferedStream, Protocol, detect_protocol, detect_protocol_from_bytes};

// used by testing stuff.
pub const PROXY_PRV_IP: &'static str = "fd00:fe11:deed:0::0";
pub const ORCHESTRATOR_PRV_IP: &'static str = "fd00:fe11:deed:0::ffff";
