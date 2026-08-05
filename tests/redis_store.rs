#![cfg(feature = "redis")]

use std::{env, time::Duration};

use tower_rate_limiter::{RateLimitError, RedisStore, Store, Usage};

#[test]
fn script_result_requires_positive_usage_and_ttl() {
    let usage =
        RedisStore::usage_from_script_result((4, 1_500)).expect("valid Redis script result");
    assert_eq!(
        usage,
        Usage {
            used: 4,
            reset_after: Duration::from_millis(1_500),
        }
    );

    assert!(matches!(
        RedisStore::usage_from_script_result((0, 1_500)),
        Err(RateLimitError::StoreUnavailable(code, _)) if code == "redis_invalid_count"
    ));
    assert!(matches!(
        RedisStore::usage_from_script_result((4, 0)),
        Err(RateLimitError::StoreUnavailable(code, _)) if code == "redis_invalid_pttl"
    ));
    assert!(matches!(
        RedisStore::usage_from_script_result((4, -1)),
        Err(RateLimitError::StoreUnavailable(code, _)) if code == "redis_invalid_pttl"
    ));
}

#[test]
fn redis_store_implements_the_common_store_seam() {
    fn assert_store<T: Store>() {}

    assert_store::<RedisStore>();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an explicit REDIS_URL and a running Redis server"]
async fn redis_store_uses_one_atomic_fixed_window() {
    let url = env::var("REDIS_URL").expect("REDIS_URL must be set for this ignored test");
    let client = redis::Client::open(url).expect("Redis URL");
    let connection = client
        .get_multiplexed_async_connection()
        .await
        .expect("Redis connection");
    let store = RedisStore::new(connection).with_namespace(format!(
        "tower-rate-limiter-test:{}:{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ));

    let first = store
        .increment("opaque:policy:window:client", Duration::from_secs(5))
        .await
        .expect("first increment");
    tokio::time::sleep(Duration::from_millis(25)).await;
    let second = store
        .increment("opaque:policy:window:client", Duration::from_secs(5))
        .await
        .expect("second increment");

    assert_eq!(first.used, 1);
    assert_eq!(second.used, 2);
    assert!(second.reset_after < first.reset_after);

    let increments = (0..16)
        .map(|_| {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .increment("opaque:policy:window:client", Duration::from_secs(5))
                    .await
                    .expect("concurrent increment")
            })
        })
        .collect::<Vec<_>>();
    let mut maximum = second.used;
    for increment in increments {
        maximum = maximum.max(increment.await.expect("join").used);
    }
    assert_eq!(maximum, 18);
}
