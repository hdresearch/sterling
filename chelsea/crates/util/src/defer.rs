use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::Mutex;
use tracing::warn;

/// A struct that keeps a list of cleanup closures to run on Drop. Assists in creating "atomic" operations; if any one
/// fails, all previous ones can roll back. Call Defer::clear() to "commit" these changes and deregister all closures.
pub struct Defer {
    to_run: Vec<Box<dyn FnOnce()>>,
}

impl Defer {
    pub fn new() -> Self {
        Self { to_run: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.to_run.clear();
    }

    pub fn defer(&mut self, f: impl FnOnce() + 'static) {
        self.to_run.push(Box::new(f));
    }
}

impl Drop for Defer {
    fn drop(&mut self) {
        for f in self.to_run.drain(..).rev() {
            f();
        }
    }
}

/// A struct that holds a list of futures to run on Drop. Register cleanup tasks with `defer()`,
/// then call `commit()` on success to skip cleanup, or let it drop on error to run cleanup.
pub struct DeferAsync {
    to_run: Vec<Pin<Box<dyn Future<Output = ()> + 'static + Send>>>,
}

impl DeferAsync {
    pub fn new() -> Self {
        Self { to_run: Vec::new() }
    }

    /// Append a cleanup task.
    pub fn defer(&mut self, f: impl Future<Output = ()> + 'static + Send) {
        self.to_run.push(Box::pin(f));
    }

    /// Commit the operation - clear cleanup tasks without running them.
    pub fn commit(&mut self) {
        self.to_run.clear();
    }

    /// Run cleanup tasks in reverse order.
    pub async fn cleanup(&mut self) {
        for f in self.to_run.drain(..).rev() {
            f.await;
        }
    }
}

impl Drop for DeferAsync {
    fn drop(&mut self) {
        if !self.to_run.is_empty() {
            warn!("DeferAsync dropped with pending async cleanup tasks. Use commit() or cleanup().await; spawning bg task now");
            let tasks = self.to_run.drain(..).rev().collect::<Vec<_>>();
            tokio::spawn(async move {
                for task in tasks {
                    task.await;
                }
            });
        }
    }
}

/// Convenience wrapper that runs a closure with automatic cleanup on error.
/// The DeferAsync is wrapped in Arc<Mutex<_>> so it can be shared/moved.
pub async fn run_with_cleanup<F, Fut, T, E>(f: F) -> Result<T, E>
where
    F: FnOnce(Arc<Mutex<DeferAsync>>) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let defer = Arc::new(Mutex::new(DeferAsync::new()));
    let result = f(Arc::clone(&defer)).await;

    let mut defer = defer.lock().await;
    if result.is_err() {
        defer.cleanup().await;
    } else {
        defer.commit();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ── Sync Defer tests ──

    #[test]
    fn sync_defer_runs_closures_on_drop() {
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let mut d = Defer::new();
            let c = counter.clone();
            d.defer(move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
        }
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn sync_defer_runs_in_reverse_order() {
        let order = Arc::new(Mutex::new(Vec::new()));
        {
            let mut d = Defer::new();
            for i in 0..3 {
                let o = order.clone();
                d.defer(move || {
                    // Mutex::lock() returns a Result; unwrap is fine in tests
                    o.blocking_lock().push(i);
                });
            }
        }
        assert_eq!(*order.blocking_lock(), vec![2, 1, 0]);
    }

    #[test]
    fn sync_defer_clear_prevents_execution() {
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let mut d = Defer::new();
            let c = counter.clone();
            d.defer(move || {
                c.fetch_add(1, Ordering::SeqCst);
            });
            d.clear();
        }
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sync_defer_multiple_defers_all_run() {
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let mut d = Defer::new();
            for _ in 0..5 {
                let c = counter.clone();
                d.defer(move || {
                    c.fetch_add(1, Ordering::SeqCst);
                });
            }
        }
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    // ── Async Defer tests ──

    #[tokio::test]
    async fn manual_usage_success() {
        let cleanup_calls = Arc::new(AtomicUsize::new(0));
        let mut defer = DeferAsync::new();

        let flag = cleanup_calls.clone();
        defer.defer(async move {
            flag.fetch_add(1, Ordering::SeqCst);
        });

        // Success path - commit
        defer.commit();

        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn manual_usage_cleanup() {
        let cleanup_calls = Arc::new(AtomicUsize::new(0));
        let mut defer = DeferAsync::new();

        let flag = cleanup_calls.clone();
        defer.defer(async move {
            flag.fetch_add(1, Ordering::SeqCst);
        });

        // Error path - manually cleanup
        defer.cleanup().await;

        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn run_with_cleanup_success() {
        let cleanup_calls = Arc::new(AtomicUsize::new(0));

        let flag = cleanup_calls.clone();
        run_with_cleanup(|defer| async move {
            defer.lock().await.defer(async move {
                flag.fetch_add(1, Ordering::SeqCst);
            });

            Result::<(), ()>::Ok(())
        })
        .await
        .unwrap();

        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_with_cleanup_error() {
        let cleanup_calls = Arc::new(AtomicUsize::new(0));

        let flag = cleanup_calls.clone();
        let result = run_with_cleanup(|defer| async move {
            defer.lock().await.defer(async move {
                flag.fetch_add(1, Ordering::SeqCst);
            });

            Result::<(), ()>::Err(())
        })
        .await;

        assert!(result.is_err());
        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn can_move_into_spawn() {
        let cleanup_calls = Arc::new(AtomicUsize::new(0));

        let flag = cleanup_calls.clone();
        run_with_cleanup(|defer| async move {
            defer.lock().await.defer(async move {
                flag.fetch_add(1, Ordering::SeqCst);
            });

            // Can move defer into spawned task!
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                defer.lock().await.commit();
            })
            .await
            .unwrap();

            Result::<(), ()>::Ok(())
        })
        .await
        .unwrap();

        assert_eq!(cleanup_calls.load(Ordering::SeqCst), 0);
    }
}
