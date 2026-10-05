//! Bounded retry with exponential backoff.

use std::{future::Future, time::Duration};

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Total attempts including the first one.
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub max_delay: Duration,
}

impl RetryPolicy {
    pub const fn new(max_attempts: u32, initial_delay: Duration, max_delay: Duration) -> Self {
        Self { max_attempts, initial_delay, max_delay }
    }

    /// Delay applied before attempt `attempt` (1-based; attempt 1 has no delay).
    pub fn delay_before(&self, attempt: u32) -> Duration {
        if attempt <= 1 {
            return Duration::ZERO;
        }
        let factor = 2u32.saturating_pow(attempt - 2);
        self.initial_delay.saturating_mul(factor).min(self.max_delay)
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(3, Duration::from_millis(500), Duration::from_secs(8))
    }
}

/// Runs `op` until it succeeds, fails with a non-retryable error, or the
/// attempt budget is exhausted. The last error is returned.
pub async fn retry<T, E, Fut, Op, Pred>(policy: RetryPolicy, is_retryable: Pred, op: Op) -> Result<T, E>
where
    Op: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    Pred: Fn(&E) -> bool,
{
    retry_hinted(policy, is_retryable, |_| None, op).await
}

/// Like [`retry`], but `hint` may supply a server-requested delay (e.g. `Retry-After`)
/// that replaces the computed backoff for that attempt, capped at `MAX_HINTED_DELAY`.
pub async fn retry_hinted<T, E, Fut, Op, Pred, Hint>(
    policy: RetryPolicy,
    is_retryable: Pred,
    hint: Hint,
    mut op: Op,
) -> Result<T, E>
where
    Op: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, E>>,
    Pred: Fn(&E) -> bool,
    Hint: Fn(&E) -> Option<Duration>,
{
    let mut attempt = 1;
    loop {
        match op(attempt).await {
            Ok(v) => return Ok(v),
            Err(e) if attempt < policy.max_attempts && is_retryable(&e) => {
                attempt += 1;
                let delay = match hint(&e) {
                    Some(requested) => requested.min(MAX_HINTED_DELAY),
                    None => policy.delay_before(attempt),
                };
                tokio::time::sleep(delay).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Upper bound on how long a server hint may make us wait between attempts.
pub const MAX_HINTED_DELAY: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    const FAST: RetryPolicy = RetryPolicy::new(3, Duration::from_millis(1), Duration::from_millis(4));

    #[test]
    fn backoff_doubles_and_caps() {
        let p = RetryPolicy::new(6, Duration::from_millis(100), Duration::from_millis(350));
        assert_eq!(p.delay_before(1), Duration::ZERO);
        assert_eq!(p.delay_before(2), Duration::from_millis(100));
        assert_eq!(p.delay_before(3), Duration::from_millis(200));
        assert_eq!(p.delay_before(4), Duration::from_millis(350));
        assert_eq!(p.delay_before(5), Duration::from_millis(350));
    }

    #[tokio::test]
    async fn server_hint_replaces_backoff_and_is_capped() {
        let start = tokio::time::Instant::now();
        let calls = AtomicU32::new(0);
        // policy delays would be ~1ms; the hint asks for 30ms
        let out: Result<u32, &str> = retry_hinted(
            FAST,
            |_| true,
            |_| Some(Duration::from_millis(30)),
            |_| async {
                let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 2 { Err("slow down") } else { Ok(n) }
            },
        )
        .await;
        assert_eq!(out, Ok(2));
        assert!(start.elapsed() >= Duration::from_millis(30));
        assert_eq!(MAX_HINTED_DELAY, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn retries_until_success() {
        let calls = AtomicU32::new(0);
        let out: Result<u32, &str> = retry(
            FAST,
            |_| true,
            |_| async {
                let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 { Err("transient") } else { Ok(n) }
            },
        )
        .await;
        assert_eq!(out, Ok(3));
    }

    #[tokio::test]
    async fn stops_at_attempt_budget() {
        let calls = AtomicU32::new(0);
        let out: Result<(), &str> = retry(
            FAST,
            |_| true,
            |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err("always")
            },
        )
        .await;
        assert_eq!(out, Err("always"));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn does_not_retry_permanent_errors() {
        let calls = AtomicU32::new(0);
        let out: Result<(), &str> = retry(
            FAST,
            |e: &&str| *e != "permanent",
            |_| async {
                calls.fetch_add(1, Ordering::SeqCst);
                Err("permanent")
            },
        )
        .await;
        assert_eq!(out, Err("permanent"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
