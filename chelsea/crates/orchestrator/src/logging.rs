use std::fs::{self, OpenOptions};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use vers_config::VersConfig;

pub fn init_logging() -> Option<WorkerGuard> {
    let base = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE);
    if VersConfig::orchestrator().log_to_disk {
        let (file_writer, guard) = build_file_writer();
        base.with_ansi(false).with_writer(file_writer).init();
        Some(guard)
    } else {
        base.init();
        None
    }
}

fn build_file_writer() -> (tracing_appender::non_blocking::NonBlocking, WorkerGuard) {
    let log_dir = &VersConfig::orchestrator().log_dir;
    if let Err(err) = fs::create_dir_all(&log_dir) {
        panic!(
            "failed to create orchestrator log file {}: {err}",
            log_dir.display()
        );
    }

    println!("Writing logs to {:?}", log_dir);
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let file_path = log_dir.join(format!("{}-orch.log", timestamp));

    match OpenOptions::new()
        .create(true)
        .append(true)
        .write(true)
        .open(&file_path)
    {
        Ok(file) => tracing_appender::non_blocking(file),
        Err(err) => {
            panic!(
                "failed to create orchestrator log file {}: {err}",
                file_path.display()
            );
        }
    }
}
