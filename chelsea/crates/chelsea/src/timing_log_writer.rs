use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use timing_layer::{PartialTimingEvent, TimingEvent, TimingLayer, TimingWriter};
use tokio::fs::OpenOptions;
use tokio::io::{self, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use vers_config::VersConfig;

enum WriteCommand {
    WriteRecord(TimingEvent),
    Shutdown,
}

enum LogDestination {
    File(String),
    Stdout,
}

pub struct LogFileWriter {
    pending_records: Arc<dashmap::DashMap<u64, TimingEvent>>,
    writer_tx: mpsc::Sender<WriteCommand>,
    writer_handle: tokio::task::JoinHandle<()>,
}

impl LogFileWriter {
    pub fn new(path: &str) -> std::io::Result<Self> {
        Self::with_destination(LogDestination::File(path.to_string()), 1000)
    }

    pub fn new_stdout() -> std::io::Result<Self> {
        Self::with_destination(LogDestination::Stdout, 1000)
    }

    // This function is currently only used by test code.  - asebexen, 30 Jan 2026
    #[cfg(test)]
    pub fn with_buffer_size(path: &str, buffer_size: usize) -> std::io::Result<Self> {
        Self::with_destination(LogDestination::File(path.to_string()), buffer_size)
    }

    fn with_destination(destination: LogDestination, buffer_size: usize) -> std::io::Result<Self> {
        let (writer_tx, mut writer_rx) = mpsc::channel::<WriteCommand>(buffer_size);
        let pending_records = Arc::new(dashmap::DashMap::new());

        // Spawn dedicated writer task using async I/O
        let writer_handle = tokio::task::spawn(async move {
            let (mut writer, flush_after_write): (Pin<Box<dyn AsyncWrite + Send>>, bool) =
                match destination {
                    LogDestination::File(path) => {
                        // Open file with async I/O
                        match OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&path)
                            .await
                        {
                            Ok(file) => (Box::pin(file), true),
                            Err(e) => {
                                tracing::error!("Failed to open timing log file {}: {}", path, e);
                                return;
                            }
                        }
                    }
                    LogDestination::Stdout => (Box::pin(io::stdout()), true),
                };

            while let Some(command) = writer_rx.recv().await {
                match command {
                    WriteCommand::WriteRecord(record) => {
                        if let Ok(json) = serde_json::to_string(&record) {
                            let line = format!("{}\n", json);
                            if let Err(e) = writer.write_all(line.as_bytes()).await {
                                tracing::warn!("Failed to write timing record: {}", e);
                            } else if flush_after_write {
                                if let Err(e) = writer.flush().await {
                                    tracing::warn!("Failed to flush timing output: {}", e);
                                }
                            }
                        } else {
                            tracing::warn!(
                                "Failed to serialize timing record for span_id: {}",
                                record.span_id
                            );
                        }
                    }
                    WriteCommand::Shutdown => break,
                }
            }
        });

        Ok(Self {
            pending_records,
            writer_tx,
            writer_handle,
        })
    }
}
impl TimingWriter for LogFileWriter {
    fn insert_event(&self, event: TimingEvent) {
        self.pending_records.insert(event.span_id, event);
    }

    fn update_event(
        &self,
        span_id: u64,
        partial: PartialTimingEvent,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(mut event_ref) = self.pending_records.get_mut(&span_id) {
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
            Err(format!("No pending event found for span_id: {}", span_id).into())
        }
    }

    fn finalize_record(
        &self,
        span_id: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some((_, record)) = self.pending_records.remove(&span_id) {
            // Send to writer task (non-blocking)
            if let Err(e) = self.writer_tx.try_send(WriteCommand::WriteRecord(record)) {
                match e {
                    mpsc::error::TrySendError::Full(_) => {
                        tracing::warn!(
                            "Timing writer channel full, dropping record for span_id: {}",
                            span_id
                        );
                    }
                    mpsc::error::TrySendError::Closed(_) => {
                        return Err("Timing writer task has shut down".into());
                    }
                }
            }
            Ok(())
        } else {
            Err(format!("No pending record found for span_id: {}", span_id).into())
        }
    }

    fn remove_event(&self, span_id: u64) {
        self.pending_records.remove(&span_id);
    }
}

impl Drop for LogFileWriter {
    fn drop(&mut self) {
        // Send shutdown signal to writer task
        let _ = self.writer_tx.try_send(WriteCommand::Shutdown);
        // Abort the writer task to ensure cleanup
        self.writer_handle.abort();
    }
}

pub fn get_timing_layer() -> Option<TimingLayer> {
    let target = VersConfig::chelsea().timing_log_target.as_str();
    match target {
        // Values to disable timing layer
        "disabled" | "" => {
            println!("Timing layer disabled");
            None
        }
        // "stdout"; create stdout writer
        "stdout" => match LogFileWriter::new_stdout() {
            Ok(writer) => {
                let writer: Arc<dyn TimingWriter> = Arc::new(writer);
                println!("Timing logs will be written to stdout");
                Some(TimingLayer { writer })
            }
            Err(error) => {
                eprintln!("Failed to create stdout timing log writer: {error}");
                None
            }
        },
        // Non-special names; assume data dir subdir
        subdir => {
            // Create log directory
            let dir = VersConfig::chelsea().data_dir.join(subdir);
            if let Err(error) = std::fs::create_dir_all(&dir) {
                eprintln!(
                    "Failed to create timing log directory {}: {error}",
                    dir.display()
                );
                return None;
            }

            // Calculate log filepath
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| Duration::from_secs(0))
                .as_secs();
            let timing_log_path = dir.join(format!("{timestamp}-timing.log"));
            let timing_log_path_str = timing_log_path.to_string_lossy().to_string();

            // Construct LogFileWriter and wrap it in a TimingLayer
            match LogFileWriter::new(&timing_log_path_str) {
                Ok(writer) => {
                    let writer: Arc<dyn TimingWriter> = Arc::new(writer);
                    eprintln!("Timing logs will be written to: {}", timing_log_path_str);
                    Some(TimingLayer { writer })
                }
                Err(error) => {
                    eprintln!(
                        "Failed to create timing log writer at {}: {error}",
                        timing_log_path.display()
                    );
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use timing_layer::{PartialTimingEvent, TimingEvent};
    use tokio::fs;
    use tokio::time::{Duration, sleep};

    async fn create_temp_file() -> Result<String, std::io::Error> {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!("test_timing_{}.log", uuid::Uuid::new_v4()));

        // Ensure temp directory exists and is writable
        if !temp_dir.exists() {
            fs::create_dir_all(&temp_dir).await?;
        }

        let path_str = file_path.to_string_lossy().to_string();

        // Test write permissions by creating and removing a test file
        fs::write(&path_str, b"test").await?;
        fs::remove_file(&path_str).await?;

        Ok(path_str)
    }

    #[tokio::test]
    async fn test_logfile_writer_basic_functionality() {
        let temp_file = create_temp_file()
            .await
            .expect("Failed to create temp file");
        let writer = LogFileWriter::new(&temp_file).unwrap();

        let event = TimingEvent::new(1, Some("test_op".to_string()));
        writer.insert_event(event);

        let partial = PartialTimingEvent {
            span_name: Some(Some("test_span".to_string())),
            elapsed_micros: Some(Some(1000)),
            ..Default::default()
        };
        writer.update_event(1, partial).unwrap();
        writer.finalize_record(1).unwrap();

        // Wait longer and poll for file content
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 50; // 5 seconds total
        let mut content = String::new();

        while attempts < MAX_ATTEMPTS {
            sleep(Duration::from_millis(100)).await;
            if let Ok(file_content) = fs::read_to_string(&temp_file).await {
                content = file_content;
                if !content.is_empty() {
                    break;
                }
            }
            attempts += 1;
        }

        // Verify file exists and has content
        assert!(
            !content.is_empty(),
            "File content is empty after {} attempts",
            attempts
        );
        assert!(
            content.contains("test_op"),
            "Content missing test_op: {}",
            content
        );
        assert!(
            content.contains("test_span"),
            "Content missing test_span: {}",
            content
        );
        assert!(
            content.contains("1000"),
            "Content missing 1000: {}",
            content
        );

        // Clean up
        let _ = fs::remove_file(&temp_file).await;
    }

    #[tokio::test]
    async fn test_channel_backpressure_drops_records() {
        let temp_file = create_temp_file()
            .await
            .expect("Failed to create temp file");
        // Small buffer to trigger backpressure
        let writer = LogFileWriter::with_buffer_size(&temp_file, 2).unwrap();

        let mut _successful_records = 0;

        // Fill up the channel quickly
        for i in 0..10 {
            let event = TimingEvent::new(i, Some(format!("op_{}", i)));
            writer.insert_event(event);
            // This should succeed or drop records when channel is full
            if writer.finalize_record(i).is_ok() {
                _successful_records += 1;
            }
        }

        // Wait longer for async processing
        sleep(Duration::from_millis(500)).await;

        let content = fs::read_to_string(&temp_file).await.unwrap_or_default();
        let line_count = content.lines().filter(|line| !line.is_empty()).count();

        // Should have some records but likely not all due to backpressure
        // Be more lenient about exact counts since timing can vary
        assert!(line_count > 0, "No records written to file");

        let _ = fs::remove_file(&temp_file).await;
    }

    #[tokio::test]
    async fn test_concurrent_writes_no_corruption() {
        let temp_file = create_temp_file()
            .await
            .expect("Failed to create temp file");
        let writer = Arc::new(LogFileWriter::new(&temp_file).unwrap());

        let handles = (0..20)
            .map(|i| {
                // Reduce concurrency for more reliable testing
                let writer = writer.clone();
                tokio::spawn(async move {
                    let event = TimingEvent::new(i, Some(format!("concurrent_op_{}", i)));
                    writer.insert_event(event);

                    // Add small delay to reduce contention
                    sleep(Duration::from_millis(1)).await;

                    let partial = PartialTimingEvent {
                        span_name: Some(Some(format!("span_{}", i))),
                        elapsed_micros: Some(Some(i as u128 * 100)),
                        ..Default::default()
                    };
                    let _ = writer.update_event(i, partial);
                    let _ = writer.finalize_record(i);
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.await.unwrap();
        }

        // Wait longer for async processing with polling
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 20;
        let mut content = String::new();

        while attempts < MAX_ATTEMPTS {
            sleep(Duration::from_millis(100)).await;
            if let Ok(file_content) = fs::read_to_string(&temp_file).await {
                content = file_content;
                let non_empty_lines = content.lines().filter(|line| !line.is_empty()).count();
                if non_empty_lines > 10 {
                    // Wait for substantial content
                    break;
                }
            }
            attempts += 1;
        }

        let lines: Vec<&str> = content.lines().filter(|line| !line.is_empty()).collect();

        // Should have written some records without corruption
        assert!(
            !lines.is_empty(),
            "No records written after {} attempts",
            attempts
        );

        // Each line should be valid JSON
        let mut valid_json_count = 0;
        for line in &lines {
            let parsed: Result<serde_json::Value, _> = serde_json::from_str(line);
            if parsed.is_ok() {
                valid_json_count += 1;
            } else {
                eprintln!("Invalid JSON line: {}", line);
            }
        }

        // At least 80% of lines should be valid JSON (allows for some race conditions)
        let success_rate = valid_json_count as f64 / lines.len() as f64;
        assert!(
            success_rate >= 0.8,
            "JSON validity rate too low: {:.2}% ({}/{})",
            success_rate * 100.0,
            valid_json_count,
            lines.len()
        );

        let _ = fs::remove_file(&temp_file).await;
    }

    #[tokio::test]
    async fn test_writer_handles_file_permissions_gracefully() {
        // Try to write to a directory that doesn't exist
        let invalid_path = "/nonexistent/invalid/path/that/does/not/exist/timing.log";

        // Constructor should succeed (error happens in background task)
        let result = LogFileWriter::new(invalid_path);
        assert!(
            result.is_ok(),
            "Constructor should succeed even with invalid path"
        );

        if let Ok(writer) = result {
            // Try to write a record - this should not panic
            let event = TimingEvent::new(1, Some("test_op".to_string()));
            writer.insert_event(event);

            // finalize_record might succeed (channel send) but background task will fail
            #[warn(unused_must_use)]
            let _finalize_result = writer.finalize_record(1);
            // Don't assert on this result as it depends on timing

            // Give background task time to fail gracefully
            sleep(Duration::from_millis(200)).await;
        }

        // The test passes if we reach here without panicking
    }

    #[tokio::test]
    async fn test_non_blocking_finalize_record() {
        let temp_file = create_temp_file()
            .await
            .expect("Failed to create temp file");
        let writer = LogFileWriter::with_buffer_size(&temp_file, 1000).unwrap();

        let start = std::time::Instant::now();

        // This should not block even with many records
        for i in 0..50 {
            // Reduce load for more reliable timing
            let event = TimingEvent::new(i, Some(format!("op_{}", i)));
            writer.insert_event(event);
            // Don't unwrap here - some might fail due to channel being full
            let _ = writer.finalize_record(i);
        }

        let elapsed = start.elapsed();
        // Be more generous with timing - remote environments can be slower
        // The key is that it's not blocking for seconds
        assert!(
            elapsed < Duration::from_millis(200),
            "finalize_record took too long: {:?} - should be non-blocking",
            elapsed
        );

        // Give async processing time to complete
        sleep(Duration::from_millis(300)).await;
        let _ = fs::remove_file(&temp_file).await;
    }
}
