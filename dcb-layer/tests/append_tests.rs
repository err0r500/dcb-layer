mod common;

use dcb_layer::ReadOptions;

// TestAppendSingleEvent
#[tokio::test]
async fn test_append_single_event_read_all_returns_it_exactly() {
    let store = common::make_store("append");
    let ev = common::tagged("OrderPlaced", &["tenant:1"]);
    store.append(vec![ev.clone()], vec![]).await.unwrap();

    let all = store.read_all().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].event.type_name, "OrderPlaced");
    assert_eq!(all[0].event.tags, vec!["tenant:1"]);
}

// TestAppendMultipleEvents
#[tokio::test]
async fn test_append_multiple_events_all_returned_strictly_ordered() {
    let store = common::make_store("append");
    let events = vec![common::event("A"), common::event("B"), common::event("C")];
    store.append(events, vec![]).await.unwrap();

    let all = store.read_all().await.unwrap();
    assert_eq!(all.len(), 3);
    for w in all.windows(2) {
        assert!(w[0].position < w[1].position);
    }
}

// TestAppendEmptySlice
#[tokio::test]
async fn test_append_empty_slice_returns_error() {
    let store = common::make_store("append");
    let err = store.append(vec![], vec![]).await.unwrap_err();
    assert!(matches!(err, dcb_layer::Error::EmptyEvents));
}

// TestAppendEventWithTooManyTags
#[tokio::test]
async fn test_append_event_with_more_than_10_tags_returns_error() {
    let store = common::make_store("append");
    let tags: Vec<&str> = (0..11).map(|_| "t").collect();
    let ev = common::tagged("OrderPlaced", &tags);
    let err = store.append(vec![ev], vec![]).await.unwrap_err();
    assert!(matches!(err, dcb_layer::Error::TooManyTags));
}

// TestAppendEventWithNoTags
#[tokio::test]
async fn test_append_event_with_no_tags_round_trips() {
    let store = common::make_store("append");
    store.append(vec![common::event("Ping")], vec![]).await.unwrap();

    let all = store.read_all().await.unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].event.tags.is_empty());
}

// Batch-index ordering within a single append call
#[tokio::test]
async fn test_append_batch_versionstamps_strictly_increasing() {
    let store = common::make_store("append");
    let n = 5usize;
    let events: Vec<_> = (0..n)
        .map(|i| dcb_layer::Event {
            type_name: "E".into(),
            tags: vec![],
            data: vec![i as u8].into(),
        })
        .collect();
    store.append(events, vec![]).await.unwrap();

    let all = store.read_all().await.unwrap();
    assert_eq!(all.len(), n);
    for w in all.windows(2) {
        assert!(w[0].position < w[1].position);
    }
    // user-version (bytes 10-11) encodes the batch index
    for (i, ev) in all.iter().enumerate() {
        let user_ver = u16::from_be_bytes([ev.position[10], ev.position[11]]);
        assert_eq!(user_ver as usize, i);
    }
}

// read_all is consistent with type-indexed read
#[tokio::test]
async fn test_append_position_consistent_across_indexes() {
    let store = common::make_store("append");
    store.append(vec![common::event("T")], vec![]).await.unwrap();

    let all = store.read_all().await.unwrap();
    let indexed = store
        .read(
            common::type_query(&["T"]),
            Some(ReadOptions { limit: 0, after: None, reverse: false }),
        )
        .await
        .unwrap();

    assert_eq!(all[0].position, indexed[0].position);
}

// ---------------------------------------------------------------------------
// Return value: last-event versionstamp
// ---------------------------------------------------------------------------

// Single-event batch: returned position must match the stored event's position.
#[tokio::test]
async fn test_append_returns_last_event_position_single() {
    let store = common::make_store("append");
    let position = store.append(vec![common::event("T")], vec![]).await.unwrap();

    let all = store.read_all().await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(position, all[0].position, "returned position must match stored event");
}

// Multi-event batch: returned position must be the LAST event's position
// (user_version bytes 10-11 == n-1).
#[tokio::test]
async fn test_append_returns_last_event_position_batch() {
    let store = common::make_store("append");
    let n = 4usize;
    let events: Vec<_> = (0..n).map(|_| common::event("T")).collect();
    let last_position = store.append(events, vec![]).await.unwrap();

    let all = store.read_all().await.unwrap();
    assert_eq!(all.len(), n);
    assert_eq!(last_position, all[n - 1].position, "returned position must be the last event");

    // Verify bytes 10-11 encode the last batch index (n-1).
    let user_ver = u16::from_be_bytes([last_position[10], last_position[11]]);
    assert_eq!(user_ver as usize, n - 1);
}
