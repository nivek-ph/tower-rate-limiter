#![cfg(all(feature = "redis", any(feature = "runtime-tokio", feature = "runtime-smol")))]

use std::{env, time::Duration};

use tower_rate_limiter::{RedisStore, Store};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redis_store_uses_one_atomic_fixed_window() {
    let url = env::var("REDIS_URL").expect("REDIS_URL must point to the test Redis server");
    let client = redis::Client::open(url).expect("valid Redis URL");
    let connection = client
        .get_multiplexed_async_connection()
        .await
        .expect("connect to test Redis");
    let store = RedisStore::new(connection).with_namespace(format!(
        "tower-rate-limiter-test:{}:{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
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
        maximum = maximum.max(increment.await.expect("join increment task").used);
    }
    assert_eq!(maximum, 18);
}
