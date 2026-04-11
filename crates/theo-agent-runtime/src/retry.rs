use std::fmt::Display;
use std::future::Future;
use std::sync::Arc;

use theo_domain::event::{DomainEvent, EventType};
use theo_domain::retry_policy::RetryPolicy;

use crate::event_bus::EventBus;

/// Executes an async operation with retry policy, exponential backoff, and event publishing.
pub struct RetryExecutor;

impl RetryExecutor {
    /// Executes `f` with the given retry policy.
    ///
    /// - On success, returns Ok(T) immediately.
    /// - On failure, retries up to `policy.max_retries` times with exponential backoff.
    /// - Publishes a DomainEvent on each retry attempt.
    /// - If `is_retryable` returns false for an error, fails immediately without retry.
    /// - If all retries are exhausted, returns the last error.
    pub async fn with_retry<F, Fut, T, E>(
        policy: &RetryPolicy,
        operation_name: &str,
        event_bus: &Arc<EventBus>,
        mut f: F,
        is_retryable: fn(&E) -> bool,
    ) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
        E: Display,
    {
        let mut last_error: Option<E> = None;

        for attempt in 0..=policy.max_retries {
            match f().await {
                Ok(value) => return Ok(value),
                Err(e) => {
                    // Check if the error is retryable
                    if !is_retryable(&e) {
                        return Err(e);
                    }

                    // Last attempt — don't sleep, just return error
                    if attempt == policy.max_retries {
                        return Err(e);
                    }

                    // Publish retry event
                    event_bus.publish(DomainEvent::new(
                        EventType::Error,
                        operation_name,
                        serde_json::json!({
                            "type": "retry",
                            "attempt": attempt + 1,
                            "max_retries": policy.max_retries,
                            "error": format!("{}", e),
                        }),
                    ));

                    let delay = policy.delay_for_attempt(attempt);
                    tokio::time::sleep(delay).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.expect("retry loop should have returned"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::CapturingListener;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn no_delay_policy(max_retries: u32) -> RetryPolicy {
        RetryPolicy {
            max_retries,
            base_delay_ms: 0,
            max_delay_ms: 0,
            jitter: false,
        }
    }

    fn always_retryable(_: &String) -> bool {
        true
    }

    fn never_retryable(_: &String) -> bool {
        false
    }

    #[tokio::test]
    async fn succeeds_on_first_attempt_no_retry() {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());

        let result = RetryExecutor::with_retry(
            &no_delay_policy(3),
            "test-op",
            &bus,
            || async { Ok::<_, String>(42) },
            always_retryable,
        )
        .await;

        assert_eq!(result.unwrap(), 42);
        // No retry events — succeeded first time
        let retry_events: Vec<_> = listener
            .captured()
            .iter()
            .filter(|e| e.payload.get("type").and_then(|v| v.as_str()) == Some("retry"))
            .cloned()
            .collect();
        assert_eq!(retry_events.len(), 0);
    }

    #[tokio::test]
    async fn fails_then_succeeds_on_retry() {
        let bus = Arc::new(EventBus::new());
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();

        let result = RetryExecutor::with_retry(
            &no_delay_policy(3),
            "test-op",
            &bus,
            move || {
                let count = cc.fetch_add(1, Ordering::SeqCst);
                async move {
                    if count == 0 {
                        Err::<i32, String>("transient error".into())
                    } else {
                        Ok(99)
                    }
                }
            },
            always_retryable,
        )
        .await;

        assert_eq!(result.unwrap(), 99);
        assert_eq!(call_count.load(Ordering::SeqCst), 2); // called twice
    }

    #[tokio::test]
    async fn exhausts_max_retries_returns_last_error() {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());

        let result = RetryExecutor::with_retry(
            &no_delay_policy(2),
            "test-op",
            &bus,
            || async { Err::<i32, String>("permanent failure".into()) },
            always_retryable,
        )
        .await;

        assert_eq!(result.unwrap_err(), "permanent failure");
        // 2 retry events (attempt 1 and 2, the 3rd attempt is the final one with no event)
        let retry_events: Vec<_> = listener
            .captured()
            .iter()
            .filter(|e| e.payload.get("type").and_then(|v| v.as_str()) == Some("retry"))
            .cloned()
            .collect();
        assert_eq!(retry_events.len(), 2);
    }

    #[tokio::test]
    async fn non_retryable_error_fails_immediately() {
        let bus = Arc::new(EventBus::new());
        let call_count = Arc::new(AtomicU32::new(0));
        let cc = call_count.clone();

        let result = RetryExecutor::with_retry(
            &no_delay_policy(5),
            "test-op",
            &bus,
            move || {
                cc.fetch_add(1, Ordering::SeqCst);
                async { Err::<i32, String>("auth failed".into()) }
            },
            never_retryable,
        )
        .await;

        assert_eq!(result.unwrap_err(), "auth failed");
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "should only call once for non-retryable"
        );
    }

    #[tokio::test]
    async fn retry_events_contain_attempt_info() {
        let bus = Arc::new(EventBus::new());
        let listener = Arc::new(CapturingListener::new());
        bus.subscribe(listener.clone());

        let _ = RetryExecutor::with_retry(
            &no_delay_policy(2),
            "my-operation",
            &bus,
            || async { Err::<i32, String>("fail".into()) },
            always_retryable,
        )
        .await;

        let events = listener.captured();
        let retry_events: Vec<_> = events
            .iter()
            .filter(|e| e.payload.get("type").and_then(|v| v.as_str()) == Some("retry"))
            .collect();

        assert_eq!(retry_events.len(), 2);
        assert_eq!(retry_events[0].entity_id, "my-operation");
        assert_eq!(retry_events[0].payload["attempt"], 1);
        assert_eq!(retry_events[1].payload["attempt"], 2);
    }
}
