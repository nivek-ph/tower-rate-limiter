//! Memory fixed-window rate-limit store.

use std::{
    collections::{HashMap, hash_map::Entry as HashMapEntry},
    future::{Ready, ready},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{RateLimitError, Store, Usage};

const CLEANUP_EVERY_INCREMENTS: u64 = 64;

#[derive(Debug)]
struct Entry {
    used: u64,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct State {
    entries: HashMap<String, Entry>,
    operations: u64,
}

/// A process-local, runtime-independent fixed-window rate-limit store.
///
/// Clones share the same usage map. Expired entries are removed for the current key on access,
/// and other expired entries are swept periodically when increments provide an opportunity.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore {
    state: Arc<Mutex<State>>,
}

impl MemoryStore {
    /// Construct an empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

fn expiration(now: Instant, window: Duration) -> Result<Instant, RateLimitError> {
    now.checked_add(window).ok_or_else(|| {
        RateLimitError::StoreUnavailable(
            String::from("memory_window_out_of_range"),
            String::from("window cannot be represented by the process clock"),
        )
    })
}

impl Store for MemoryStore {
    type Future = Ready<Result<Usage, RateLimitError>>;

    fn increment(&self, key: &str, window: Duration) -> Self::Future {
        let now = Instant::now();
        let result = self
            .state
            .lock()
            .map_err(|_| {
                RateLimitError::StoreUnavailable(
                    String::from("memory_store_poisoned"),
                    String::from("memory store state is poisoned"),
                )
            })
            .and_then(|mut state| {
                state.operations = state.operations.wrapping_add(1);
                if state.operations % CLEANUP_EVERY_INCREMENTS == 0 {
                    state.entries.retain(|_, entry| now < entry.expires_at);
                }

                let entry = match state.entries.entry(key.to_owned()) {
                    HashMapEntry::Occupied(mut occupied) => {
                        let entry = occupied.get_mut();
                        if now >= entry.expires_at {
                            entry.used = 0;
                            entry.expires_at = expiration(now, window)?;
                        }
                        occupied.into_mut()
                    }
                    HashMapEntry::Vacant(vacant) => vacant.insert(Entry {
                        used: 0,
                        expires_at: expiration(now, window)?,
                    }),
                };
                entry.used = entry.used.saturating_add(1);

                Ok(Usage {
                    used: entry.used,
                    reset_after: entry.expires_at.saturating_duration_since(now),
                })
            });
        ready(result)
    }
}
