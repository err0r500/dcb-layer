mod common;

use std::time::Duration;

// ---------------------------------------------------------------------------
// Cursor — get / set
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_get_cursor_returns_none_initially() {
    let store = common::make_store("subscribe");
    let cursor = store.get_cursor("my-sub").await.unwrap();
    assert!(cursor.is_none(), "fresh subscription must have no cursor");
}

#[tokio::test]
async fn test_set_cursor_then_get_returns_same_value() {
    let store = common::make_store("subscribe");
    // Use the position from a real append so the versionstamp is valid.
    let pos = store.append(vec![common::event("E")], vec![]).await.unwrap();

    store.set_cursor("my-sub", pos).await.unwrap();
    let got = store.get_cursor("my-sub").await.unwrap();
    assert_eq!(got, Some(pos), "cursor must round-trip exactly");
}

#[tokio::test]
async fn test_set_cursor_can_be_advanced() {
    let store = common::make_store("subscribe");
    let pos1 = store.append(vec![common::event("E1")], vec![]).await.unwrap();
    let pos2 = store.append(vec![common::event("E2")], vec![]).await.unwrap();

    store.set_cursor("my-sub", pos1).await.unwrap();
    assert_eq!(store.get_cursor("my-sub").await.unwrap(), Some(pos1));

    store.set_cursor("my-sub", pos2).await.unwrap();
    assert_eq!(store.get_cursor("my-sub").await.unwrap(), Some(pos2));
}

#[tokio::test]
async fn test_cursors_are_isolated_by_name() {
    let store = common::make_store("subscribe");
    let pos = store.append(vec![common::event("E")], vec![]).await.unwrap();

    store.set_cursor("sub-a", pos).await.unwrap();

    // sub-b was never written
    assert!(store.get_cursor("sub-b").await.unwrap().is_none());
    assert_eq!(store.get_cursor("sub-a").await.unwrap(), Some(pos));
}

#[tokio::test]
async fn test_cursors_are_isolated_by_namespace() {
    let store_a = common::make_store("subscribe");
    let store_b = common::make_store("subscribe");
    let pos = store_a.append(vec![common::event("E")], vec![]).await.unwrap();

    store_a.set_cursor("sub", pos).await.unwrap();

    // Same subscription name in a different namespace must be independent.
    assert!(store_b.get_cursor("sub").await.unwrap().is_none());
}

// ---------------------------------------------------------------------------
// Sentinel — written on append, triggers watch
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_sentinel_fires_after_append() {
    let store = common::make_store("subscribe");
    let store_for_watch = store.clone();

    // Arm the watch before the append (mirrors the GenServer ordering).
    let watch_task = tokio::spawn(async move {
        store_for_watch.wait_for_sentinel_change().await;
    });

    // Give the watch task time to register and commit the watch transaction.
    tokio::time::sleep(Duration::from_millis(200)).await;

    store.append(vec![common::event("Ping")], vec![]).await.unwrap();

    tokio::time::timeout(Duration::from_secs(5), watch_task)
        .await
        .expect("sentinel watch did not fire within 5 s after append")
        .expect("watch task panicked");
}

#[tokio::test]
async fn test_sentinel_fires_on_each_append() {
    let store = common::make_store("subscribe");

    for round in 1..=3 {
        let store_for_watch = store.clone();
        let watch_task = tokio::spawn(async move {
            store_for_watch.wait_for_sentinel_change().await;
        });
        tokio::time::sleep(Duration::from_millis(200)).await;

        store
            .append(vec![common::event(&format!("E{round}"))], vec![])
            .await
            .unwrap();

        tokio::time::timeout(Duration::from_secs(5), watch_task)
            .await
            .unwrap_or_else(|_| panic!("round {round}: sentinel did not fire"))
            .expect("watch task panicked");
    }
}

#[tokio::test]
async fn test_sentinel_value_changes_between_appends() {
    // Two sequential appends must produce two distinct sentinel values —
    // confirming that SetVersionstampedValue writes a unique tx versionstamp
    // each time rather than a constant empty value.
    let store = common::make_store("subscribe");

    let pos1 = store.append(vec![common::event("A")], vec![]).await.unwrap();
    let pos2 = store.append(vec![common::event("B")], vec![]).await.unwrap();

    // Positions come from the same tx versionstamp source as the sentinel.
    // Two distinct appends must have different tx versions (bytes 0..10).
    let tx1 = &pos1[..10];
    let tx2 = &pos2[..10];
    assert_ne!(tx1, tx2, "two appends must produce distinct tx versionstamps");
}
