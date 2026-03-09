use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, SemaphorePermit};
use tokio::time::timeout;

#[derive(Debug)]
pub enum RateLimitError {
    Timeout,
    Overloaded,
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "Rate limit: request timed out waiting for a slot"),
            Self::Overloaded => write!(f, "Rate limit: server overloaded"),
        }
    }
}

#[derive(Clone)]
pub struct QueueStats {
    pub total_processed: u64,
    pub total_rejected: u64,
    pub current_pending: usize,
    pub avg_wait_us: u64,
}

struct LimiterInner {
    concurrent: Semaphore,
    max_per_minute: u32,
    minute_count: AtomicU64,
    minute_start: std::sync::Mutex<Instant>,
    total_processed: AtomicU64,
    total_rejected: AtomicU64,
    total_wait_us: AtomicU64,
    pending: AtomicU64,
}

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<LimiterInner>,
}

pub struct RateGuard<'a> {
    _permit: SemaphorePermit<'a>,
    limiter: &'a RateLimiter,
    start: Instant,
}

impl<'a> Drop for RateGuard<'a> {
    fn drop(&mut self) {
        let wait = self.start.elapsed().as_micros() as u64;
        self.limiter
            .inner
            .total_wait_us
            .fetch_add(wait, Ordering::Relaxed);
        self.limiter
            .inner
            .total_processed
            .fetch_add(1, Ordering::Relaxed);
    }
}

impl RateLimiter {
    pub fn new(max_requests_per_minute: u32, max_concurrent: u32) -> Self {
        Self {
            inner: Arc::new(LimiterInner {
                concurrent: Semaphore::new(max_concurrent as usize),
                max_per_minute: max_requests_per_minute,
                minute_count: AtomicU64::new(0),
                minute_start: std::sync::Mutex::new(Instant::now()),
                total_processed: AtomicU64::new(0),
                total_rejected: AtomicU64::new(0),
                total_wait_us: AtomicU64::new(0),
                pending: AtomicU64::new(0),
            }),
        }
    }

    pub async fn acquire(&self) -> Result<RateGuard<'_>, RateLimitError> {
        self.inner.pending.fetch_add(1, Ordering::Relaxed);

        self.check_minute_window();

        let current = self.inner.minute_count.fetch_add(1, Ordering::Relaxed);
        if current >= self.inner.max_per_minute as u64 {
            self.inner.minute_count.fetch_sub(1, Ordering::Relaxed);
            self.inner.pending.fetch_sub(1, Ordering::Relaxed);
            self.inner.total_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(RateLimitError::Overloaded);
        }

        let start = Instant::now();
        let permit = timeout(Duration::from_secs(30), self.inner.concurrent.acquire())
            .await
            .map_err(|_| {
                self.inner.pending.fetch_sub(1, Ordering::Relaxed);
                self.inner.total_rejected.fetch_add(1, Ordering::Relaxed);
                RateLimitError::Timeout
            })?
            .map_err(|_| {
                self.inner.pending.fetch_sub(1, Ordering::Relaxed);
                RateLimitError::Overloaded
            })?;

        self.inner.pending.fetch_sub(1, Ordering::Relaxed);

        Ok(RateGuard {
            _permit: permit,
            limiter: self,
            start,
        })
    }

    pub fn pending(&self) -> usize {
        self.inner.pending.load(Ordering::Relaxed) as usize
    }

    pub fn stats(&self) -> QueueStats {
        let processed = self.inner.total_processed.load(Ordering::Relaxed);
        let total_wait = self.inner.total_wait_us.load(Ordering::Relaxed);
        QueueStats {
            total_processed: processed,
            total_rejected: self.inner.total_rejected.load(Ordering::Relaxed),
            current_pending: self.pending(),
            avg_wait_us: if processed > 0 {
                total_wait / processed
            } else {
                0
            },
        }
    }

    fn check_minute_window(&self) {
        let mut start = self.inner.minute_start.lock().unwrap();
        if start.elapsed() >= Duration::from_secs(60) {
            *start = Instant::now();
            self.inner.minute_count.store(0, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acquire_within_limits() {
        let limiter = RateLimiter::new(100, 10);
        let guard = limiter.acquire().await;
        assert!(guard.is_ok());
        drop(guard);
        let stats = limiter.stats();
        assert_eq!(stats.total_processed, 1);
        assert_eq!(stats.total_rejected, 0);
    }

    #[tokio::test]
    async fn concurrent_limit_respected() {
        let limiter = RateLimiter::new(100, 2);
        let g1 = limiter.acquire().await.unwrap();
        let g2 = limiter.acquire().await.unwrap();
        assert_eq!(limiter.inner.concurrent.available_permits(), 0);
        drop(g1);
        assert_eq!(limiter.inner.concurrent.available_permits(), 1);
        drop(g2);
    }

    #[tokio::test]
    async fn rate_limit_rejects_over_max() {
        let limiter = RateLimiter::new(2, 10);
        let _g1 = limiter.acquire().await.unwrap();
        let _g2 = limiter.acquire().await.unwrap();
        let result = limiter.acquire().await;
        assert!(matches!(result, Err(RateLimitError::Overloaded)));
        let stats = limiter.stats();
        assert_eq!(stats.total_rejected, 1);
    }

    #[tokio::test]
    async fn stats_tracked() {
        let limiter = RateLimiter::new(100, 10);
        let g = limiter.acquire().await.unwrap();
        drop(g);
        let g = limiter.acquire().await.unwrap();
        drop(g);
        let stats = limiter.stats();
        assert_eq!(stats.total_processed, 2);
        assert_eq!(stats.total_rejected, 0);
    }

    #[tokio::test]
    async fn pending_count() {
        let limiter = RateLimiter::new(100, 1);
        let _g = limiter.acquire().await.unwrap();
        assert_eq!(limiter.pending(), 0);
    }
}
