use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BURST: f64 = 10.0;
const REFILL_PER_SECOND: f64 = 0.1;
const IDLE_EVICTION: Duration = Duration::from_secs(15 * 60);
const EVICTION_CHECK_LEN: usize = 1024;

struct Bucket {
    tokens: f64,
    updated_at: Instant,
}

/// Per-IP token bucket for the unauthenticated endpoints: bursts of 10
/// attempts, refilling one every 10 seconds.
#[derive(Clone, Default)]
pub struct RateLimiter {
    buckets: Arc<Mutex<HashMap<IpAddr, Bucket>>>,
}

impl RateLimiter {
    /// Whether this attempt may proceed. Unknown addresses are allowed; they
    /// only occur off-network, such as in-process tests.
    pub fn allow(&self, ip: Option<IpAddr>) -> bool {
        let Some(ip) = ip else {
            return true;
        };
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("rate limiter lock poisoned");

        if buckets.len() > EVICTION_CHECK_LEN {
            buckets.retain(|_, bucket| now.duration_since(bucket.updated_at) < IDLE_EVICTION);
        }

        let bucket = buckets.entry(ip).or_insert(Bucket {
            tokens: BURST,
            updated_at: now,
        });
        let elapsed = now.duration_since(bucket.updated_at).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * REFILL_PER_SECOND).min(BURST);
        bucket.updated_at = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}
