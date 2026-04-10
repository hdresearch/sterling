use lru::LruCache;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::new(1, 0);
const CACHE_SIZE: NonZeroUsize = NonZeroUsize::new(1000).unwrap();

#[derive(Clone)]
pub struct Limiter {
    cache: Arc<Mutex<LruCache<IpAddr, Instant>>>,
}

// We are defending against programatic attacks on our API. Denial of
// service/flooding attacks, and API key guessing attacks. We want to
// do that with minimal impact on good clients — minimal lockouts,
// slowdowns, errors when people are working on setting up etc.
impl Limiter {
    pub fn new() -> Self {
        tracing::debug!(
            cache_size = CACHE_SIZE.get(),
            timeout_secs = TIMEOUT.as_secs(),
            "Creating rate limiter"
        );
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(CACHE_SIZE))),
        }
    }

    pub fn is_rate_limited(&self, key: SocketAddr) -> bool {
        // Limit to 1 failed login per x seconds. No reset, to longer
        // timeout, that is it.

        // We are only passed IP addresses that have failed auth. So
        // all we are doing is checking that we haven't seen this IP
        // in the last second.

        // Given an ip address, see if it is in the cache:
        //
        // If the IP address isn't in the cache, return false
        // If it is in the cache, but it is older than TIMEOUT seconds, return false
        // If it is in the cache, and is younger than TIMEOUT seconds return true

        tracing::trace!(ip = %key.ip(), "Checking rate limit for IP");

        // Always insert into the cache with the current timestamp
        let mut cache = self.cache.lock().unwrap();
        let is_limited = match cache.put(key.ip(), Instant::now()) {
            Some(when) => {
                let elapsed = Instant::now() - when;
                let is_within_timeout = when > (Instant::now() - TIMEOUT);

                if is_within_timeout {
                    tracing::warn!(
                        ip = %key.ip(),
                        elapsed_ms = elapsed.as_millis(),
                        timeout_ms = TIMEOUT.as_millis(),
                        "IP is rate limited (too many recent failed attempts)"
                    );
                } else {
                    tracing::debug!(
                        ip = %key.ip(),
                        elapsed_ms = elapsed.as_millis(),
                        "IP was in cache but timeout expired, allowing request"
                    );
                }

                is_within_timeout
            }
            None => {
                tracing::trace!(
                    ip = %key.ip(),
                    "IP not in rate limit cache, allowing request"
                );
                false
            }
        };

        is_limited
    }
}
