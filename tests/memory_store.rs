#![cfg(feature = "memory")]

use std::{sync::Arc, thread, time::Duration};

use tower_rate_limiter::{MemoryStore, RateLimitError, Store, Usage};

fn increment(store: &MemoryStore, key: &str, window: Duration) -> Usage {
    store
        .increment(key, window)
        .into_inner()
        .expect("memory store increment")
}

#[test]
fn first_increment_starts_a_window_and_later_increments_preserve_it() {
    let store = MemoryStore::new();
    let first = increment(&store, "key", Duration::from_secs(60));
    let second = increment(&store, "key", Duration::from_secs(60));

    assert_eq!(first.used, 1);
    assert_eq!(second.used, 2);
    assert!(first.reset_after > Duration::ZERO);
    assert!(second.reset_after <= first.reset_after);
}

#[test]
fn independent_keys_do_not_share_usage() {
    let store = MemoryStore::new();

    assert_eq!(increment(&store, "a", Duration::from_secs(60)).used, 1);
    assert_eq!(increment(&store, "a", Duration::from_secs(60)).used, 2);
    assert_eq!(increment(&store, "b", Duration::from_secs(60)).used, 1);
}

#[test]
fn clones_share_usage() {
    let store = MemoryStore::new();
    let clone = store.clone();

    assert_eq!(increment(&store, "shared", Duration::from_secs(60)).used, 1);
    assert_eq!(increment(&clone, "shared", Duration::from_secs(60)).used, 2);
}

#[test]
fn concurrent_increments_are_not_lost() {
    let store = Arc::new(MemoryStore::new());
    let total = 32;

    thread::scope(|scope| {
        let handles = (0..total)
            .map(|_| {
                let store = Arc::clone(&store);
                scope.spawn(move || increment(&store, "concurrent", Duration::from_secs(60)))
            })
            .collect::<Vec<_>>();

        let usages = handles
            .into_iter()
            .map(|handle| handle.join().expect("increment thread"))
            .collect::<Vec<_>>();
        let final_usage = increment(&store, "concurrent", Duration::from_secs(60));

        assert_eq!(final_usage.used, total + 1);
        assert!(
            usages
                .iter()
                .all(|usage| final_usage.reset_after <= usage.reset_after)
        );
    });
}

#[test]
fn expired_usage_restarts_at_one_with_a_new_window() {
    let store = MemoryStore::new();
    let window = Duration::from_millis(5);
    let first = increment(&store, "expiring", window);
    thread::sleep(Duration::from_millis(15));
    let restarted = increment(&store, "expiring", window);

    assert_eq!(first.used, 1);
    assert_eq!(restarted.used, 1);
    assert!(restarted.reset_after > Duration::ZERO);
}

#[test]
fn different_scopes_remain_independent_in_one_store() {
    let store = MemoryStore::new();

    assert_eq!(
        increment(&store, "policy-a-window-60", Duration::from_secs(60)).used,
        1
    );
    assert_eq!(
        increment(&store, "policy-b-window-60", Duration::from_secs(60)).used,
        1
    );
    assert_eq!(
        increment(&store, "policy-a-window-30", Duration::from_secs(30)).used,
        1
    );
}

#[test]
fn an_unrepresentable_window_is_reported_as_a_store_error() {
    let result = MemoryStore::new()
        .increment("huge", Duration::MAX)
        .into_inner();

    assert!(matches!(
        result,
        Err(RateLimitError::StoreUnavailable(code, message))
            if code == "memory_window_out_of_range"
                && message == "window cannot be represented by the process clock"
    ));
}
