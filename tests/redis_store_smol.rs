#![cfg(all(feature = "redis", feature = "runtime-smol"))]

use std::{env, time::Duration};

use tower_rate_limiter::{RedisStore, Store};

async fn test_store(test_name: &str) -> RedisStore {
    let url = env::var("REDIS_URL").expect("REDIS_URL must point to the test Redis server");
    let client = redis::Client::open(url).expect("valid Redis URL");
    let connection = client
        .get_multiplexed_async_connection()
        .await
        .expect("connect to test Redis");

    RedisStore::new(connection).with_namespace(format!(
        "tower-rate-limiter-smol-test:{}:{}:{}",
        test_name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos()
    ))
}

#[test]
fn increments_are_atomic() {
    smol::block_on(async {
        let store = test_store("atomic-increment").await;
        let window = Duration::from_secs(5);

        let first = store.increment("policy:client", window).await.expect("first increment");
        let second = store
            .increment("policy:client", window)
            .await
            .expect("second increment");

        assert_eq!(first.used, 1);
        assert_eq!(second.used, 2);
    });
}

#[test]
fn ttl_decreases_within_the_same_window() {
    smol::block_on(async {
        let store = test_store("decreasing-ttl").await;
        let window = Duration::from_secs(5);

        let first = store.increment("policy:client", window).await.expect("first increment");
        smol::Timer::after(Duration::from_millis(50)).await;
        let second = store
            .increment("policy:client", window)
            .await
            .expect("second increment");

        assert!(second.reset_after < first.reset_after);
    });
}

#[test]
fn counter_resets_after_the_window_expires() {
    smol::block_on(async {
        let store = test_store("window-expiration").await;
        let window = Duration::from_millis(200);

        let first = store.increment("policy:client", window).await.expect("first increment");
        assert_eq!(first.used, 1);

        smol::Timer::after(Duration::from_millis(300)).await;

        let after_expiration = store
            .increment("policy:client", window)
            .await
            .expect("increment after expiration");
        assert_eq!(after_expiration.used, 1);
    });
}

#[test]
fn concurrent_increments_each_receive_a_unique_count() {
    smol::block_on(async {
        let store = test_store("concurrent-increments").await;
        let window = Duration::from_secs(5);

        let increments = (0..16)
            .map(|_| {
                let store = store.clone();
                smol::spawn(async move {
                    store
                        .increment("policy:client", window)
                        .await
                        .expect("concurrent increment")
                        .used
                })
            })
            .collect::<Vec<_>>();

        let mut counts = Vec::with_capacity(increments.len());
        for increment in increments {
            counts.push(increment.await);
        }
        counts.sort_unstable();

        assert_eq!(counts, (1_u64..=16).collect::<Vec<_>>());
    });
}
