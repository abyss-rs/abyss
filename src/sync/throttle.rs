//! Bandwidth throttling for sync transfers.
//!
//! Provides rate limiting using a token bucket algorithm.

use anyhow::Result;
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

/// Bandwidth limit configuration.
#[derive(Debug, Clone, Copy)]
pub struct BandwidthLimit {
    /// Bytes per second limit.
    pub bytes_per_second: u64,
}

impl BandwidthLimit {
    /// Create a new bandwidth limit.
    pub fn new(bytes_per_second: u64) -> Self {
        Self { bytes_per_second }
    }

    /// No limit.
    pub fn unlimited() -> Self {
        Self { bytes_per_second: 0 }
    }

    /// 1 MB/s limit.
    pub fn slow() -> Self {
        Self::new(1_000_000)
    }

    /// 10 MB/s limit.
    pub fn medium() -> Self {
        Self::new(10_000_000)
    }

    /// 100 MB/s limit.
    pub fn fast() -> Self {
        Self::new(100_000_000)
    }

    /// Check if there's a limit.
    pub fn is_limited(&self) -> bool {
        self.bytes_per_second > 0
    }

    /// Format as human-readable string.
    pub fn display(&self) -> String {
        if !self.is_limited() {
            return "unlimited".to_string();
        }
        
        let bps = self.bytes_per_second;
        if bps >= 1_000_000_000 {
            format!("{:.1} GB/s", bps as f64 / 1_000_000_000.0)
        } else if bps >= 1_000_000 {
            format!("{:.1} MB/s", bps as f64 / 1_000_000.0)
        } else if bps >= 1_000 {
            format!("{:.1} KB/s", bps as f64 / 1_000.0)
        } else {
            format!("{} B/s", bps)
        }
    }
}

impl Default for BandwidthLimit {
    fn default() -> Self {
        Self::unlimited()
    }
}

/// Bandwidth limiter using token bucket algorithm.
#[derive(Clone)]
pub struct BandwidthLimiter {
    limiter: Option<Arc<RateLimiter<governor::state::NotKeyed, governor::state::InMemoryState, governor::clock::DefaultClock>>>,
    limit: BandwidthLimit,
}

impl BandwidthLimiter {
    /// Create a new bandwidth limiter.
    pub fn new(limit: BandwidthLimit) -> Self {
        let limiter = if limit.is_limited() {
            // Create rate limiter
            // We use chunks of 1KB for smoother limiting
            let chunk_size = 1024u32;
            let chunks_per_second = (limit.bytes_per_second / chunk_size as u64).max(1) as u32;
            
            if let Some(rate) = NonZeroU32::new(chunks_per_second) {
                let quota = Quota::per_second(rate);
                Some(Arc::new(RateLimiter::direct(quota)))
            } else {
                None
            }
        } else {
            None
        };
        
        Self { limiter, limit }
    }

    /// Create an unlimited limiter.
    pub fn unlimited() -> Self {
        Self::new(BandwidthLimit::unlimited())
    }

    /// Get the current limit.
    pub fn limit(&self) -> BandwidthLimit {
        self.limit
    }

    /// Wait for permission to transfer `bytes` bytes.
    /// This is a no-op if no limit is set.
    pub async fn acquire(&self, bytes: usize) {
        if let Some(limiter) = &self.limiter {
            // Request tokens for chunks of 1KB
            let chunks = ((bytes + 1023) / 1024).max(1);
            
            for _ in 0..chunks {
                limiter.until_ready().await;
            }
        }
    }

    /// Wait for permission to transfer `bytes` bytes (blocking version).
    pub fn acquire_blocking(&self, bytes: usize) {
        if let Some(limiter) = &self.limiter {
            let chunks = ((bytes + 1023) / 1024).max(1);
            
            for _ in 0..chunks {
                while limiter.check().is_err() {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    /// Check if we can transfer `bytes` bytes without waiting.
    pub fn try_acquire(&self, bytes: usize) -> bool {
        if let Some(limiter) = &self.limiter {
            let chunks = ((bytes + 1023) / 1024).max(1);
            
            for _ in 0..chunks {
                if limiter.check().is_err() {
                    return false;
                }
            }
        }
        true
    }

    /// Update the bandwidth limit.
    pub fn set_limit(&mut self, limit: BandwidthLimit) {
        *self = Self::new(limit);
    }
}

impl Default for BandwidthLimiter {
    fn default() -> Self {
        Self::unlimited()
    }
}

/// Transfer statistics for bandwidth monitoring.
#[derive(Debug, Clone, Default)]
pub struct TransferStats {
    /// Total bytes transferred.
    pub bytes_transferred: u64,
    /// Transfer start time.
    pub start_time: Option<std::time::Instant>,
    /// Transfer end time.
    pub end_time: Option<std::time::Instant>,
}

impl TransferStats {
    /// Create new transfer stats.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start tracking.
    pub fn start(&mut self) {
        self.start_time = Some(std::time::Instant::now());
        self.bytes_transferred = 0;
    }

    /// Record bytes transferred.
    pub fn record(&mut self, bytes: u64) {
        self.bytes_transferred += bytes;
    }

    /// Stop tracking.
    pub fn stop(&mut self) {
        self.end_time = Some(std::time::Instant::now());
    }

    /// Get elapsed duration.
    pub fn elapsed(&self) -> Duration {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => end.duration_since(start),
            (Some(start), None) => start.elapsed(),
            _ => Duration::ZERO,
        }
    }

    /// Get average transfer rate in bytes per second.
    pub fn rate(&self) -> f64 {
        let elapsed = self.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.bytes_transferred as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Format rate as human-readable string.
    pub fn rate_display(&self) -> String {
        let rate = self.rate();
        if rate >= 1_000_000_000.0 {
            format!("{:.1} GB/s", rate / 1_000_000_000.0)
        } else if rate >= 1_000_000.0 {
            format!("{:.1} MB/s", rate / 1_000_000.0)
        } else if rate >= 1_000.0 {
            format!("{:.1} KB/s", rate / 1_000.0)
        } else {
            format!("{:.0} B/s", rate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bandwidth_limit_display() {
        assert_eq!(BandwidthLimit::unlimited().display(), "unlimited");
        assert_eq!(BandwidthLimit::new(1000).display(), "1.0 KB/s");
        assert_eq!(BandwidthLimit::new(1_500_000).display(), "1.5 MB/s");
        assert_eq!(BandwidthLimit::new(2_500_000_000).display(), "2.5 GB/s");
    }

    #[test]
    fn test_unlimited_limiter() {
        let limiter = BandwidthLimiter::unlimited();
        
        // Should always return true for unlimited
        assert!(limiter.try_acquire(1_000_000_000));
    }

    #[test]
    fn test_transfer_stats() {
        let mut stats = TransferStats::new();
        stats.start();
        std::thread::sleep(Duration::from_millis(1)); // Ensure elapsed > 0
        stats.record(1000);
        stats.record(2000);
        
        assert_eq!(stats.bytes_transferred, 3000);
        assert!(stats.elapsed() >= Duration::from_millis(1));
    }
}
