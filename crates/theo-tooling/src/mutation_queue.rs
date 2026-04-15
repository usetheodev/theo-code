use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// Error types for file mutation queue operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MutationQueueError {
    #[error("timeout acquiring lock for path: {path}")]
    Timeout { path: PathBuf },
}

/// Per-path locking mechanism to serialize concurrent file mutations.
///
/// When the agent runs multiple tools in parallel (batch tool), concurrent
/// writes to the same file can cause race conditions. `FileMutationQueue`
/// ensures that mutations to the same path are serialized while allowing
/// mutations to different paths to proceed concurrently.
#[derive(Debug, Default)]
pub struct FileMutationQueue {
    locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl FileMutationQueue {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }

    /// Execute `f` while holding an exclusive lock on `path`.
    ///
    /// - Same-path calls are serialized (one at a time).
    /// - Different-path calls proceed concurrently.
    /// - Returns `MutationQueueError::Timeout` if the lock cannot be acquired
    ///   within 5 seconds.
    pub async fn with_lock<F, Fut, T>(
        &self,
        path: &Path,
        f: F,
    ) -> Result<T, MutationQueueError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let path_lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(path.to_path_buf())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        let guard = tokio::time::timeout(LOCK_TIMEOUT, path_lock.lock())
            .await
            .map_err(|_| MutationQueueError::Timeout {
                path: path.to_path_buf(),
            })?;

        let result = f().await;
        drop(guard);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use tokio::sync::mpsc;
    use tokio::time::{self, Duration};

    #[tokio::test]
    async fn test_concurrent_writes_to_same_file_are_serialized() {
        // Arrange
        let queue = Arc::new(FileMutationQueue::new());
        let path = PathBuf::from("/tmp/test_file.rs");
        let (tx, mut rx) = mpsc::channel::<u32>(10);

        // Act — spawn two tasks that write to the same path.
        // Task 1 holds the lock for 100ms, then sends 1.
        // Task 2 acquires after task 1 releases, then sends 2.
        let q1 = queue.clone();
        let p1 = path.clone();
        let tx1 = tx.clone();
        let handle1 = tokio::spawn(async move {
            q1.with_lock(&p1, || async {
                time::sleep(Duration::from_millis(100)).await;
                tx1.send(1).await.unwrap();
            })
            .await
            .unwrap();
        });

        // Small yield so task 1 acquires first.
        tokio::task::yield_now().await;

        let q2 = queue.clone();
        let p2 = path.clone();
        let tx2 = tx.clone();
        let handle2 = tokio::spawn(async move {
            q2.with_lock(&p2, || async {
                tx2.send(2).await.unwrap();
            })
            .await
            .unwrap();
        });

        handle1.await.unwrap();
        handle2.await.unwrap();
        drop(tx);

        // Assert — task 1 must complete before task 2.
        let first = rx.recv().await.unwrap();
        let second = rx.recv().await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 2);
    }

    #[tokio::test]
    async fn test_different_files_proceed_in_parallel() {
        // Arrange
        let queue = Arc::new(FileMutationQueue::new());
        let path_a = PathBuf::from("/tmp/file_a.rs");
        let path_b = PathBuf::from("/tmp/file_b.rs");
        let (tx, mut rx) = mpsc::channel::<&str>(10);

        // Act — both tasks hold their lock for 100ms.
        // If they were serialized, total time would be ~200ms.
        // If parallel, ~100ms.
        let q1 = queue.clone();
        let tx1 = tx.clone();
        let handle1 = tokio::spawn(async move {
            q1.with_lock(&path_a, || async {
                time::sleep(Duration::from_millis(100)).await;
                tx1.send("a").await.unwrap();
            })
            .await
            .unwrap();
        });

        let q2 = queue.clone();
        let tx2 = tx.clone();
        let handle2 = tokio::spawn(async move {
            q2.with_lock(&path_b, || async {
                time::sleep(Duration::from_millis(100)).await;
                tx2.send("b").await.unwrap();
            })
            .await
            .unwrap();
        });

        let start = tokio::time::Instant::now();
        handle1.await.unwrap();
        handle2.await.unwrap();
        let elapsed = start.elapsed();
        drop(tx);

        // Assert — both completed, and total time is well under 200ms
        // (proving they ran in parallel).
        let mut results = vec![];
        while let Some(v) = rx.recv().await {
            results.push(v);
        }
        assert_eq!(results.len(), 2);
        assert!(
            elapsed < Duration::from_millis(180),
            "Expected parallel execution, but took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn test_timeout_when_lock_held_too_long() {
        // Arrange
        let queue = Arc::new(FileMutationQueue::new());
        let path = PathBuf::from("/tmp/locked_file.rs");

        // Acquire the per-path lock directly to simulate a long-held lock.
        let path_lock = {
            let mut locks = queue.locks.lock().await;
            locks
                .entry(path.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = path_lock.lock().await;

        // Act — try to acquire through with_lock; it should timeout.
        // Use a shorter timeout by calling the lock directly to avoid waiting 5s.
        let q = queue.clone();
        let p = path.clone();
        let result = tokio::time::timeout(Duration::from_millis(200), async {
            // We override the internal timeout by racing externally.
            q.with_lock(&p, || async { 42 }).await
        })
        .await;

        // Assert — the outer timeout fires because the lock is held.
        assert!(result.is_err(), "Expected timeout, but lock was acquired");
    }
}
