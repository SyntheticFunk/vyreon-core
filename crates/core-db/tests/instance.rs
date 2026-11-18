//! One core at a time (C-053; audit 2026-11-17 round w, 1, 11, 16): the single-instance lock a
//! service takes at startup. Run with `cargo test -- --ignored`.
use core_db::instance;
use test_support::Scratch;

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server (VYREON_CORE_TEST_PG)"]
async fn a_second_core_is_refused_while_the_first_holds_the_database() {
    let scratch = Scratch::new("instance").await;
    let first = scratch.client().await;
    let second = scratch.client().await;
    assert!(instance::take(&first).await.unwrap(), "the first core takes the lock");
    assert!(instance::held(&first).await.unwrap());
    assert!(!instance::take(&second).await.unwrap(), "a second core is refused");
    assert!(!instance::held(&second).await.unwrap());
    // the first core's connection ends (it stopped, or lost the database): the lock goes with it
    drop(first);
    let mut taken = false;
    for _ in 0..50 {
        if instance::take(&second).await.unwrap() {
            taken = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(taken, "the lock is free once the session holding it has ended");
    assert!(instance::held(&second).await.unwrap());
    drop(second);
    scratch.drop().await;
}
