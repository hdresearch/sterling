//! Minimal types — just what we need for spend tracking extraction.
//! Request/response bodies pass through as raw bytes/JSON, never deserialized into domain types.

/// Token usage info extracted from provider responses for spend tracking.
#[derive(Debug, Clone, Default)]
pub struct UsageInfo {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
