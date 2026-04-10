use crate::timing_event::{PartialTimingEvent, TimingEvent};

/// Trait for writing timing data to various backends (DB, file, log aggregator, etc.)
pub trait TimingWriter: Send + Sync {
    /// Insert a new timing event
    fn insert_event(&self, event: TimingEvent);

    /// Update an existing event with partial data
    fn update_event(
        &self,
        span_id: u64,
        partial: PartialTimingEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Finalize and persist a completed timing event
    fn finalize_record(&self, span_id: u64)
    -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Remove/cleanup a timing event (for error cases)
    /// This is a best-effort cleanup and should not fail
    fn remove_event(&self, span_id: u64);
}
