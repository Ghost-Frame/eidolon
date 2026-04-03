use std::collections::VecDeque;
use std::time::Instant;

use dashmap::DashMap;

pub struct RateLimiter {
    windows: DashMap<String, VecDeque<Instant>>,
    requests_per_minute: u32,
    burst: u32,
}

pub struct RateLimitInfo {
    pub remaining: u32,
    pub limit: u32,
    pub reset_secs: u64,
}

pub struct RateLimitExceeded {
    pub retry_after_secs: u64,
    pub limit: u32,
}

impl RateLimiter {
    pub fn new(requests_per_minute: u32, burst: u32) -> Self {
        RateLimiter {
            windows: DashMap::new(),
            requests_per_minute,
            burst,
        }
    }

    /// Check if a request from `user` is allowed.
    /// Returns Ok(info) with remaining quota, or Err(exceeded) with retry-after.
    pub fn check(&self, user: &str) -> Result<RateLimitInfo, RateLimitExceeded> {
        let now = Instant::now();
        let window = std::time::Duration::from_secs(60);
        let max_requests = self.requests_per_minute + self.burst;

        let mut entry = self.windows.entry(user.to_string()).or_insert_with(VecDeque::new);
        let deque = entry.value_mut();

        // Prune timestamps older than the window
        while let Some(&front) = deque.front() {
            if now.duration_since(front) > window {
                deque.pop_front();
            } else {
                break;
            }
        }

        let count = deque.len() as u32;

        if count >= max_requests {
            // Calculate when the oldest entry in the window expires
            let oldest = deque.front().unwrap();
            let expires_in = window.saturating_sub(now.duration_since(*oldest));
            Err(RateLimitExceeded {
                retry_after_secs: expires_in.as_secs().max(1),
                limit: max_requests,
            })
        } else {
            deque.push_back(now);
            let remaining = max_requests - count - 1;
            let reset_secs = if let Some(&oldest) = deque.front() {
                window.saturating_sub(now.duration_since(oldest)).as_secs()
            } else {
                60
            };
            Ok(RateLimitInfo {
                remaining,
                limit: max_requests,
                reset_secs,
            })
        }
    }

    /// Prune all expired entries across all users. Call periodically to bound memory.
    pub fn prune_expired(&self) {
        let now = Instant::now();
        let window = std::time::Duration::from_secs(60);
        let mut empty_keys = Vec::new();

        for mut entry in self.windows.iter_mut() {
            let deque = entry.value_mut();
            while let Some(&front) = deque.front() {
                if now.duration_since(front) > window {
                    deque.pop_front();
                } else {
                    break;
                }
            }
            if deque.is_empty() {
                empty_keys.push(entry.key().clone());
            }
        }

        for key in empty_keys {
            self.windows.remove(&key);
        }
    }
}
