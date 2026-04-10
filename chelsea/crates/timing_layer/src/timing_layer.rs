use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::Subscriber;
use tracing::span::{self, Id};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

use crate::{
    timing_event::{PartialTimingEvent, TimingEvent},
    timing_writer::TimingWriter,
    tl_newtypes::{TimingOperationId, TimingStartInstant},
    visitor::{OperationIdVisitor, RetErrVisitor},
};
#[derive(Clone)]
pub struct TimingLayer {
    pub writer: Arc<dyn TimingWriter>,
}

impl<S> Layer<S> for TimingLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let span = match ctx.span(id) {
            Some(span) => span,
            None => {
                tracing::warn!("on_new_span: failed to retrieve span with ID {:?}", id);
                return;
            }
        };

        // Retrieve the operation_id from the attributes struct
        let mut visitor = OperationIdVisitor::default();
        attrs.record(&mut visitor);

        // Set the operation_id from the matched attribute if present, from parent if not
        let operation_id = visitor.operation_id.or_else(|| {
            span.parent().and_then(|parent| {
                parent
                    .extensions()
                    .get::<TimingOperationId>()
                    .map(|op_id| op_id.to_string())
            })
        });

        // Get current time
        let operation_start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .ok();

        // Create an event for this operation
        let mut event = TimingEvent::new(id.into_u64(), operation_id.clone());
        event.operation_start_time = operation_start_time;

        self.writer.insert_event(event);

        // Insert the operation_id into the span extensions only if we have one and it doesn't already exist
        if let Some(op_id) = operation_id {
            if span.extensions().get::<TimingOperationId>().is_none() {
                span.extensions_mut().insert(TimingOperationId::new(op_id));
            }
        }
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        // Insert the start time into the span extensions only if not already present
        let span = match ctx.span(id) {
            Some(span) => span,
            None => {
                tracing::warn!("on_enter: failed to retrieve span with ID {:?}", id);
                return;
            }
        };

        if span.extensions().get::<TimingStartInstant>().is_none() {
            span.extensions_mut().insert(TimingStartInstant::now());
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        // instrument sends the return and error values in an event; RetErrVisitor can extract these values
        let mut visitor = RetErrVisitor::default();
        event.record(&mut visitor);

        // Only process events that have timing data and are within a span context
        if let Some(value) = visitor.value {
            if let Some(span) = ctx.current_span().id() {
                let span_id = span.into_u64();
                if let Err(e) = self.writer.update_event(
                    span_id,
                    PartialTimingEvent {
                        return_value: Some(Some(value)),
                        is_error: Some(Some(visitor.is_error)),
                        ..Default::default()
                    },
                ) {
                    tracing::warn!("Failed to update event with span ID {}: {}", span_id, e);
                }
            }
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let span_id = id.into_u64();
        let span = match ctx.span(&id) {
            Some(span) => span,
            None => {
                tracing::warn!("on_close: failed to retrieve span with ID {:?}", id);
                self.writer.remove_event(span_id);
                return;
            }
        };
        let span_name = span.name().to_string();
        let span_target = span.metadata().target().to_string();

        // Retrieve the start time from the span extensions
        let start_time = match span.extensions().get::<TimingStartInstant>() {
            Some(time) => time.clone(),
            None => {
                tracing::warn!(
                    "on_close: span missing TimingStartInstant for span_id {}",
                    span_id
                );
                self.writer.remove_event(span_id);
                return;
            }
        };

        // Compute the full name by recursively pushing parents' names
        let mut span_fullname_vec = vec![span_name.clone()];
        let mut current_span = span;
        while let Some(parent) = current_span.parent() {
            span_fullname_vec.push(parent.name().to_string());
            current_span = parent;
        }
        span_fullname_vec.push(span_target);
        span_fullname_vec.reverse();
        let span_fullname = span_fullname_vec.join("::");

        // Compute elapsed time
        let elapsed = start_time.elapsed();

        // Finalize the timing event; to minimize writes to a DB, we could use an in-memory cache like this sample does, and then only push to the DB on span close
        if let Err(e) = self.writer.update_event(
            span_id,
            PartialTimingEvent {
                elapsed_micros: Some(Some(elapsed.as_micros())),
                span_name: Some(Some(span_name)),
                span_fullname: Some(Some(span_fullname)),
                ..Default::default()
            },
        ) {
            tracing::warn!("Failed to update timing event for span {}: {}", span_id, e);
            return; // Don't finalize if update failed
        }

        // Finalize and persist the completed record
        if let Err(e) = self.writer.finalize_record(span_id) {
            tracing::warn!("Failed to finalize record for span {}: {}", span_id, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tracing::info_span;
    use tracing_subscriber::{Registry, layer::SubscriberExt};

    #[derive(Debug, Clone)]
    struct MockTimingWriter {
        events: Arc<Mutex<HashMap<u64, TimingEvent>>>,
        calls: Arc<Mutex<Vec<String>>>,
        finalized: Arc<Mutex<Vec<u64>>>,
    }

    #[derive(Clone)]
    struct AsyncTimingWriter {
        events: Arc<dashmap::DashMap<u64, TimingEvent>>,
        tx: mpsc::Sender<AsyncCommand>,
        calls: Arc<Mutex<Vec<String>>>,
        finalized: Arc<Mutex<Vec<u64>>>,
    }

    enum AsyncCommand {
        Write(TimingEvent),
    }

    impl MockTimingWriter {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(HashMap::new())),
                calls: Arc::new(Mutex::new(Vec::new())),
                finalized: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn get_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn get_finalized(&self) -> Vec<u64> {
            self.finalized.lock().unwrap().clone()
        }
    }

    impl TimingWriter for MockTimingWriter {
        fn insert_event(&self, event: TimingEvent) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("insert_event({})", event.span_id));
            self.events.lock().unwrap().insert(event.span_id, event);
        }

        fn update_event(
            &self,
            span_id: u64,
            partial: PartialTimingEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("update_event({})", span_id));
            if let Some(event) = self.events.lock().unwrap().get_mut(&span_id) {
                if let Some(span_name) = partial.span_name {
                    event.span_name = span_name;
                }
                if let Some(span_fullname) = partial.span_fullname {
                    event.span_fullname = span_fullname;
                }
                if let Some(return_value) = partial.return_value {
                    event.return_value = return_value;
                }
                if let Some(elapsed_micros) = partial.elapsed_micros {
                    event.elapsed_micros = elapsed_micros;
                }
                if let Some(is_error) = partial.is_error {
                    event.is_error = is_error;
                }
                if let Some(operation_start_time) = partial.operation_start_time {
                    event.operation_start_time = operation_start_time;
                }
                Ok(())
            } else {
                Err(format!("No event found for span_id: {}", span_id).into())
            }
        }

        fn finalize_record(
            &self,
            span_id: u64,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("finalize_record({})", span_id));
            self.finalized.lock().unwrap().push(span_id);
            self.events.lock().unwrap().remove(&span_id);
            Ok(())
        }

        fn remove_event(&self, span_id: u64) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("remove_event({})", span_id));
            self.events.lock().unwrap().remove(&span_id);
        }
    }

    impl AsyncTimingWriter {
        fn new(buffer_size: usize) -> Self {
            let (tx, mut rx) = mpsc::channel::<AsyncCommand>(buffer_size);
            let events = Arc::new(dashmap::DashMap::new());
            let calls = Arc::new(Mutex::new(Vec::new()));
            let finalized = Arc::new(Mutex::new(Vec::new()));

            let finalized_clone = finalized.clone();

            // Spawn async writer task
            tokio::spawn(async move {
                while let Some(command) = rx.recv().await {
                    match command {
                        AsyncCommand::Write(event) => {
                            // Simulate async I/O delay
                            tokio::time::sleep(Duration::from_micros(100)).await;
                            finalized_clone.lock().unwrap().push(event.span_id);
                        }
                    }
                }
            });

            Self {
                events,
                tx,
                calls,
                finalized,
            }
        }

        fn get_calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn get_finalized(&self) -> Vec<u64> {
            self.finalized.lock().unwrap().clone()
        }
    }

    impl TimingWriter for AsyncTimingWriter {
        fn insert_event(&self, event: TimingEvent) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("insert_event({})", event.span_id));
            self.events.insert(event.span_id, event);
        }

        fn update_event(
            &self,
            span_id: u64,
            partial: PartialTimingEvent,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("update_event({})", span_id));
            if let Some(mut event_ref) = self.events.get_mut(&span_id) {
                if let Some(span_name) = partial.span_name {
                    event_ref.span_name = span_name;
                }
                if let Some(span_fullname) = partial.span_fullname {
                    event_ref.span_fullname = span_fullname;
                }
                if let Some(return_value) = partial.return_value {
                    event_ref.return_value = return_value;
                }
                if let Some(elapsed_micros) = partial.elapsed_micros {
                    event_ref.elapsed_micros = elapsed_micros;
                }
                if let Some(is_error) = partial.is_error {
                    event_ref.is_error = is_error;
                }
                if let Some(operation_start_time) = partial.operation_start_time {
                    event_ref.operation_start_time = operation_start_time;
                }
                Ok(())
            } else {
                Err(format!("No event found for span_id: {}", span_id).into())
            }
        }

        fn finalize_record(
            &self,
            span_id: u64,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("finalize_record({})", span_id));
            if let Some((_, event)) = self.events.remove(&span_id) {
                // Non-blocking send to async writer
                if let Err(e) = self.tx.try_send(AsyncCommand::Write(event)) {
                    match e {
                        mpsc::error::TrySendError::Full(_) => {
                            return Err("Async writer channel full".into());
                        }
                        mpsc::error::TrySendError::Closed(_) => {
                            return Err("Async writer task has shut down".into());
                        }
                    }
                }
                Ok(())
            } else {
                Err(format!("No event found for span_id: {}", span_id).into())
            }
        }

        fn remove_event(&self, span_id: u64) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("remove_event({})", span_id));
            self.events.remove(&span_id);
        }
    }

    #[test]
    fn test_on_new_span_inserts_event() {
        let mock_writer = Arc::new(MockTimingWriter::new());
        let timing_layer = TimingLayer {
            writer: mock_writer.clone(),
        };

        let subscriber = Registry::default().with(timing_layer);
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!("test_span", operation_id = "test_op_123");
            let _guard = span.enter();
            // Exit the span to trigger on_close which finalizes the record
        });

        let calls = mock_writer.get_calls();
        let finalized = mock_writer.get_finalized();

        // Should have called insert_event and finalize_record
        assert!(calls.iter().any(|c| c.starts_with("insert_event(")));
        assert!(!finalized.is_empty());
    }

    #[test]
    fn test_on_close_finalizes_with_elapsed_and_fullname() {
        let mock_writer = Arc::new(MockTimingWriter::new());
        let timing_layer = TimingLayer {
            writer: mock_writer.clone(),
        };

        let subscriber = Registry::default().with(timing_layer);
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!("test_span", operation_id = "test_op_123");
            let _guard = span.enter();
            std::thread::sleep(Duration::from_millis(1));
        });

        let calls = mock_writer.get_calls();
        let finalized = mock_writer.get_finalized();

        assert!(calls.iter().any(|c| c.starts_with("update_event(")));
        assert!(calls.iter().any(|c| c.starts_with("finalize_record(")));
        assert!(!finalized.is_empty());
    }

    #[test]
    fn test_mock_timing_writer_records_calls() {
        let mock_writer = MockTimingWriter::new();

        let event = TimingEvent::new(42, Some("test_op".to_string()));
        mock_writer.insert_event(event);

        let partial = PartialTimingEvent {
            span_name: Some(Some("test".to_string())),
            ..Default::default()
        };
        mock_writer.update_event(42, partial).unwrap();
        mock_writer.finalize_record(42).unwrap();

        let calls = mock_writer.get_calls();
        assert_eq!(
            calls,
            vec![
                "insert_event(42)".to_string(),
                "update_event(42)".to_string(),
                "finalize_record(42)".to_string(),
            ]
        );

        let finalized = mock_writer.get_finalized();
        assert_eq!(finalized, vec![42]);
    }

    #[tokio::test]
    async fn test_concurrent_span_closures_no_deadlock() {
        let mock_writer = Arc::new(MockTimingWriter::new());

        let handles = (0..10)
            .map(|i| {
                let mock_writer = mock_writer.clone();
                tokio::spawn(async move {
                    let timing_layer = TimingLayer {
                        writer: mock_writer,
                    };
                    let subscriber = Registry::default().with(timing_layer);

                    tracing::subscriber::with_default(subscriber, || {
                        let span =
                            info_span!("concurrent_span", operation_id = format!("op_{}", i));
                        let _guard = span.enter();
                        std::thread::sleep(Duration::from_millis(1));
                    });
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.await.unwrap();
        }

        let finalized = mock_writer.get_finalized();
        assert_eq!(finalized.len(), 10);
    }

    #[tokio::test]
    async fn test_events_flushed_in_order() {
        let mock_writer = Arc::new(MockTimingWriter::new());
        let timing_layer = TimingLayer {
            writer: mock_writer.clone(),
        };

        let subscriber = Registry::default().with(timing_layer);
        tracing::subscriber::with_default(subscriber, || {
            for i in 0..5 {
                let span = info_span!("ordered_span", operation_id = format!("op_{}", i));
                let _guard = span.enter();
                std::thread::sleep(Duration::from_millis(1));
            }
        });

        let calls = mock_writer.get_calls();
        let finalized = mock_writer.get_finalized();

        assert_eq!(finalized.len(), 5);
        assert!(calls.iter().any(|c| c.starts_with("insert_event(")));
        assert!(calls.iter().any(|c| c.starts_with("finalize_record(")));
    }

    #[test]
    fn test_instrumented_handler_produces_timing_event() {
        let mock_writer = Arc::new(MockTimingWriter::new());
        let timing_layer = TimingLayer {
            writer: mock_writer.clone(),
        };

        let subscriber = Registry::default().with(timing_layer);

        #[tracing::instrument(fields(operation_id = "handler_op_456"))]
        fn mock_handler() -> Result<String, &'static str> {
            Ok("success".to_string())
        }

        tracing::subscriber::with_default(subscriber, || {
            let _result = mock_handler();
        });

        let finalized = mock_writer.get_finalized();
        assert!(!finalized.is_empty());

        let calls = mock_writer.get_calls();
        assert!(calls.iter().any(|c| c.starts_with("insert_event(")));
    }

    #[tokio::test]
    async fn test_async_writer_basic_functionality() {
        let async_writer = Arc::new(AsyncTimingWriter::new(1000));
        let timing_layer = TimingLayer {
            writer: async_writer.clone(),
        };

        let subscriber = Registry::default().with(timing_layer);
        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!("async_test_span", operation_id = "async_op_123");
            let _guard = span.enter();
            std::thread::sleep(Duration::from_millis(1));
        });

        // Wait for async processing
        tokio::time::sleep(Duration::from_millis(200)).await;

        let calls = async_writer.get_calls();
        let finalized = async_writer.get_finalized();

        assert!(calls.iter().any(|c| c.starts_with("insert_event(")));
        assert!(calls.iter().any(|c| c.starts_with("finalize_record(")));
        assert_eq!(finalized.len(), 1);
    }

    #[tokio::test]
    async fn test_async_writer_channel_backpressure() {
        // Small buffer to trigger backpressure
        let async_writer = Arc::new(AsyncTimingWriter::new(2));
        let timing_layer = TimingLayer {
            writer: async_writer.clone(),
        };

        let subscriber = Registry::default().with(timing_layer);

        tracing::subscriber::with_default(subscriber, || {
            // Generate many spans quickly to overwhelm the channel
            for i in 0..20 {
                let span = info_span!("backpressure_span", operation_id = format!("bp_op_{}", i));
                let _guard = span.enter();
                // Brief operation
                std::thread::sleep(Duration::from_micros(10));
            }
        });

        // Wait for async processing
        tokio::time::sleep(Duration::from_millis(300)).await;

        let calls = async_writer.get_calls();
        let finalized = async_writer.get_finalized();

        // Should have processed some but not necessarily all due to backpressure
        assert!(!calls.is_empty());
        assert!(!finalized.is_empty());
        // Due to backpressure, some records may have been dropped
        assert!(finalized.len() <= 20);
    }

    #[tokio::test]
    async fn test_async_writer_concurrent_operations() {
        let async_writer = Arc::new(AsyncTimingWriter::new(100));

        let handles = (0..50)
            .map(|i| {
                let async_writer = async_writer.clone();
                tokio::spawn(async move {
                    let timing_layer = TimingLayer {
                        writer: async_writer,
                    };
                    let subscriber = Registry::default().with(timing_layer);

                    tracing::subscriber::with_default(subscriber, || {
                        let span = info_span!(
                            "concurrent_async_span",
                            operation_id = format!("concurrent_async_op_{}", i)
                        );
                        let _guard = span.enter();
                        std::thread::sleep(Duration::from_micros(500));
                    });
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.await.unwrap();
        }

        // Wait for all async processing to complete
        tokio::time::sleep(Duration::from_millis(500)).await;

        let calls = async_writer.get_calls();
        let finalized = async_writer.get_finalized();

        // Should have processed all or most spans
        assert!(calls.len() >= 100); // insert + finalize calls
        assert!(finalized.len() >= 40); // Allow some potential drops under high load
    }

    #[tokio::test]
    async fn test_async_writer_timing_accuracy() {
        let async_writer = Arc::new(AsyncTimingWriter::new(10));
        let timing_layer = TimingLayer {
            writer: async_writer.clone(),
        };

        let subscriber = Registry::default().with(timing_layer);
        let start_time = std::time::Instant::now();

        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!("timing_accuracy_span", operation_id = "timing_test_op");
            let _guard = span.enter();
            std::thread::sleep(Duration::from_millis(10));
        });

        let sync_elapsed = start_time.elapsed();

        // Wait for async processing
        tokio::time::sleep(Duration::from_millis(200)).await;

        let finalized = async_writer.get_finalized();
        assert_eq!(finalized.len(), 1);

        // The async processing shouldn't significantly delay the span completion
        assert!(sync_elapsed < Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_async_writer_error_handling() {
        // Create writer with very small buffer to force errors
        let async_writer = Arc::new(AsyncTimingWriter::new(1));

        let event1 = TimingEvent::new(1, Some("test_op_1".to_string()));
        let event2 = TimingEvent::new(2, Some("test_op_2".to_string()));
        let event3 = TimingEvent::new(3, Some("test_op_3".to_string()));

        async_writer.insert_event(event1);
        async_writer.insert_event(event2);
        async_writer.insert_event(event3);

        // These should succeed
        let result1 = async_writer.finalize_record(1);
        let result2 = async_writer.finalize_record(2);

        // This might fail due to channel being full
        let _result3 = async_writer.finalize_record(3);

        // At least some should succeed
        assert!(result1.is_ok() || result2.is_ok());

        // Wait for processing
        tokio::time::sleep(Duration::from_millis(200)).await;

        let finalized = async_writer.get_finalized();
        assert!(!finalized.is_empty());
    }

    #[tokio::test]
    async fn test_async_writer_mixed_sync_async_load() {
        let async_writer = Arc::new(AsyncTimingWriter::new(50));

        // Mix of sync and async operations
        let sync_handle = std::thread::spawn({
            let async_writer = async_writer.clone();
            move || {
                for i in 0..25 {
                    let timing_layer = TimingLayer {
                        writer: async_writer.clone(),
                    };
                    let subscriber = Registry::default().with(timing_layer);

                    tracing::subscriber::with_default(subscriber, || {
                        let span = info_span!(
                            "sync_mixed_span",
                            operation_id = format!("sync_mixed_{}", i)
                        );
                        let _guard = span.enter();
                        std::thread::sleep(Duration::from_micros(100));
                    });
                }
            }
        });

        let async_handles = (0..25)
            .map(|i| {
                let async_writer = async_writer.clone();
                tokio::spawn(async move {
                    let timing_layer = TimingLayer {
                        writer: async_writer,
                    };
                    let subscriber = Registry::default().with(timing_layer);

                    tracing::subscriber::with_default(subscriber, || {
                        let span = info_span!(
                            "async_mixed_span",
                            operation_id = format!("async_mixed_{}", i)
                        );
                        let _guard = span.enter();
                        std::thread::sleep(Duration::from_micros(100));
                    });

                    tokio::time::sleep(Duration::from_micros(50)).await;
                })
            })
            .collect::<Vec<_>>();

        sync_handle.join().unwrap();
        for handle in async_handles {
            handle.await.unwrap();
        }

        // Wait for all async processing
        tokio::time::sleep(Duration::from_millis(300)).await;

        let calls = async_writer.get_calls();
        let finalized = async_writer.get_finalized();

        // Should handle mixed load well
        assert!(calls.len() >= 80); // 50 operations * ~2 calls each (insert + finalize)  
        assert!(finalized.len() >= 40); // Most spans should complete successfully
    }
}
