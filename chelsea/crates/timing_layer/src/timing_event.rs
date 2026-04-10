use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct TimingEvent {
    pub span_id: u64,
    pub operation_id: Option<String>,
    pub span_name: Option<String>,
    pub span_fullname: Option<String>,
    pub return_value: Option<String>,
    pub elapsed_micros: Option<u128>,
    /// If the return value was specifically a Result::Err, this will be true
    pub is_error: Option<bool>,
    pub operation_start_time: Option<u64>,
}

impl TimingEvent {
    pub fn new<S: Into<String>>(span_id: u64, operation_id: Option<S>) -> Self {
        Self {
            span_id,
            operation_id: operation_id.map(|id| id.into()),
            span_name: None,
            span_fullname: None,
            return_value: None,
            elapsed_micros: None,
            is_error: None,
            operation_start_time: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct PartialTimingEvent {
    pub span_name: Option<Option<String>>,
    pub span_fullname: Option<Option<String>>,
    pub return_value: Option<Option<String>>,
    pub elapsed_micros: Option<Option<u128>>,
    pub is_error: Option<Option<bool>>,
    pub operation_start_time: Option<Option<u64>>,
}
