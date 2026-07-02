mod common;

use dcb_layer::{Query, QueryItem, ReadOptions};

/// A watch registered via `register_sentinel_watch` must fire for an append
/// that commits strictly after registration returned (no wake-loss window).
#[tokio::test(flavor = "multi_thread")]
async fn test_watch_registered_before_return_fires_on_next_append() {
    let store = common::make_store("watchreg");

    // Registration is durable once this returns Ok.
    let watch = store.register_sentinel_watch().await.unwrap();

    store.append(vec![common::event("Ping")], vec![]).await.unwrap();

    tokio::time::timeout(std::time::Duration::from_secs(5), watch)
        .await
        .expect("watch did not fire within 5s")
        .expect("watch resolved with error");
}

/// Seek-based type discovery must return the same events as before: compare a
/// tags-only query against a read_all filtered in-test, across multiple types
/// sharing the tag, plus `after`/`reverse`/`limit` combinations.
#[tokio::test(flavor = "multi_thread")]
async fn test_tags_only_discovery_matches_full_scan() {
    let store = common::make_store("seek");

    // 4 distinct types on the same tag, interleaved with noise.
    for i in 0..3 {
        for t in ["Alpha", "Beta", "Gamma", "Delta"] {
            store
                .append(vec![common::tagged(t, &["k:1", "extra:x"])], vec![])
                .await
                .unwrap();
            store
                .append(vec![common::tagged(&format!("Noise{i}"), &["k:2"])], vec![])
                .await
                .unwrap();
        }
    }

    let all = store.read_all().await.unwrap();
    let expected: Vec<_> = all
        .iter()
        .filter(|se| se.event.tags.contains(&"k:1".to_string()))
        .collect();
    assert_eq!(expected.len(), 12);

    let query = || Query {
        items: vec![QueryItem { types: vec![], tags: vec!["k:1".into()] }],
    };

    // Plain forward read.
    let got = store.read(query(), None).await.unwrap();
    assert_eq!(
        got.iter().map(|se| se.position).collect::<Vec<_>>(),
        expected.iter().map(|se| se.position).collect::<Vec<_>>()
    );

    // Reverse with limit.
    let got = store
        .read(
            query(),
            Some(ReadOptions { limit: 5, reverse: true, after: None }),
        )
        .await
        .unwrap();
    let mut want: Vec<_> = expected.iter().rev().take(5).map(|se| se.position).collect();
    assert_eq!(got.iter().map(|se| se.position).collect::<Vec<_>>(), want);

    // Forward with after = 4th matching event.
    let after = expected[3].position;
    let got = store
        .read(
            query(),
            Some(ReadOptions { limit: 0, reverse: false, after: Some(after) }),
        )
        .await
        .unwrap();
    want = expected.iter().skip(4).map(|se| se.position).collect();
    assert_eq!(got.iter().map(|se| se.position).collect::<Vec<_>>(), want);
}

/// Tags-only append conditions (which use type discovery inside the write
/// transaction) must still catch matching events of any type.
#[tokio::test(flavor = "multi_thread")]
async fn test_tags_only_condition_still_enforced_after_seek_discovery() {
    let store = common::make_store("seekcond");

    store
        .append(vec![common::tagged("SomeType", &["u:9"])], vec![])
        .await
        .unwrap();

    let cond = dcb_layer::AppendCondition {
        query: Query { items: vec![QueryItem { types: vec![], tags: vec!["u:9".into()] }] },
        after: None,
    };
    let err = store
        .append(vec![common::tagged("Other", &["u:9"])], vec![cond])
        .await
        .unwrap_err();
    assert!(matches!(err, dcb_layer::Error::AppendConditionFailed));
}
