use std::{
    future::Future,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::time::Instant;

/// Returns a tuple containing the time started (unix epoch time, in seconds), the duration elapsed (in ns), and the original output from the future
pub async fn time_future<Fut: Future<Output = R>, R>(future: Fut) -> (u64, u128, R) {
    let time_start = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before Unix epoch")
        .as_secs();
    let start = Instant::now();

    let result = future.await;

    let end = Instant::now();
    let duration_ns = end.duration_since(start).as_nanos();

    (time_start, duration_ns, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_future_result() {
        let (_start, _dur, result) = time_future(async { 42 }).await;
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn epoch_timestamp_is_reasonable() {
        let (start, _, _) = time_future(async {}).await;
        // Should be after 2024-01-01 (1_704_067_200) and before 2040-01-01 (2_208_988_800)
        assert!(start > 1_704_067_200, "epoch {start} is too small");
        assert!(start < 2_208_988_800, "epoch {start} is too large");
    }

    #[tokio::test]
    async fn duration_reflects_elapsed_time() {
        let (_start, duration_ns, _) = time_future(async {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        })
        .await;

        // Should be at least 40ms (some slack for scheduling)
        assert!(
            duration_ns >= 40_000_000,
            "duration {duration_ns}ns is too short"
        );
        // Should be less than 2s (generous upper bound)
        assert!(
            duration_ns < 2_000_000_000,
            "duration {duration_ns}ns is too long"
        );
    }

    #[tokio::test]
    async fn propagates_result_type() {
        let (_, _, result) = time_future(async { Result::<&str, &str>::Ok("hello") }).await;
        assert_eq!(result.unwrap(), "hello");
    }
}
