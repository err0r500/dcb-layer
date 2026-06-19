mod common;

use dcb_layer::ReadOptions;

// ---------------------------------------------------------------------------
// Filtering by type
// ---------------------------------------------------------------------------

// TestReadByType
#[tokio::test]
async fn test_read_by_type_returns_only_matching_type() {
    let store = common::make_store("read");
    store
        .append(vec![common::event("T1"), common::event("T2")], vec![])
        .await
        .unwrap();

    let results = store.read(common::type_query(&["T1"]), None).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].event.type_name, "T1");
}

// TestReadByMultipleTypes
#[tokio::test]
async fn test_read_by_multiple_types_excludes_others() {
    let store = common::make_store("read");
    store
        .append(
            vec![common::event("T1"), common::event("T2"), common::event("T3")],
            vec![],
        )
        .await
        .unwrap();

    let results = store.read(common::type_query(&["T1", "T3"]), None).await.unwrap();
    assert_eq!(results.len(), 2);
    let types: Vec<&str> = results.iter().map(|e| e.event.type_name.as_str()).collect();
    assert!(types.contains(&"T1"));
    assert!(types.contains(&"T3"));
    assert!(!types.contains(&"T2"));
    // strictly ordered
    assert!(results[0].position < results[1].position);
}

// TestReadCountsEventsCorrectly
#[tokio::test]
async fn test_read_counts_events_correctly() {
    let store = common::make_store("read");
    let n = 4usize;
    let events: Vec<_> = (0..n).map(|_| common::event("T")).collect();
    store.append(events, vec![]).await.unwrap();

    let results = store.read(common::type_query(&["T"]), None).await.unwrap();
    assert_eq!(results.len(), n);
}

// ---------------------------------------------------------------------------
// Filtering by tags
// ---------------------------------------------------------------------------

// TestReadByTags
#[tokio::test]
async fn test_read_by_tags_returns_only_tagged_events() {
    let store = common::make_store("read");
    store
        .append(
            vec![common::tagged("T", &["tagA"]), common::tagged("T", &["tagB"])],
            vec![],
        )
        .await
        .unwrap();

    let results = store.read(common::tag_query(&["tagA"]), None).await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].event.tags.iter().any(|t| t =="tagA"));
}

// TestReadByMultipleTags — AND semantics
#[tokio::test]
async fn test_read_by_multiple_tags_requires_all_tags() {
    let store = common::make_store("read");
    store
        .append(
            vec![
                common::tagged("T", &["tagA", "tagB"]),
                common::tagged("T", &["tagA"]),
            ],
            vec![],
        )
        .await
        .unwrap();

    let results = store.read(common::tag_query(&["tagA", "tagB"]), None).await.unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].event.tags.iter().any(|t| t =="tagB"));
}

// TestReadByTypeAndTags
#[tokio::test]
async fn test_read_by_type_and_tags_returns_intersection() {
    let store = common::make_store("read");
    // E1(T1,tagA), E2(T1,tagB), E3(T2,tagA)
    store
        .append(
            vec![
                common::tagged("T1", &["tagA"]),
                common::tagged("T1", &["tagB"]),
                common::tagged("T2", &["tagA"]),
            ],
            vec![],
        )
        .await
        .unwrap();

    let results = store
        .read(common::type_tag_query(&["T1"], &["tagA"]), None)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].event.type_name, "T1");
    assert!(results[0].event.tags.iter().any(|t| t =="tagA"));
}

// ---------------------------------------------------------------------------
// OR semantics across query items
// ---------------------------------------------------------------------------

// TestReadMultipleQueryItems
#[tokio::test]
async fn test_read_multiple_query_items_or_semantics() {
    let store = common::make_store("read");
    // E1(T1), E2(tag:A), E3(other)
    store
        .append(
            vec![
                common::event("T1"),
                common::tagged("Other", &["A"]),
                common::event("Unrelated"),
            ],
            vec![],
        )
        .await
        .unwrap();

    // OR: {types:[T1]} | {tags:[A]} → E1 and E2, not E3
    let results = store
        .read(common::or_query(&[(&["T1"], &[]), (&[], &["A"])]), None)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
    // strictly ordered
    assert!(results[0].position < results[1].position);
}

// TestEventOrderingWithMultipleRanges
#[tokio::test]
async fn test_event_ordering_with_multiple_ranges() {
    let store = common::make_store("read");
    // T1 and T2 in same batch → two ranges in the type index, merged
    store
        .append(vec![common::event("T1"), common::event("T2")], vec![])
        .await
        .unwrap();

    let results = store.read(common::type_query(&["T1", "T2"]), None).await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results[0].position < results[1].position);
}

// ---------------------------------------------------------------------------
// Empty result
// ---------------------------------------------------------------------------

// TestReadEmptyResult
#[tokio::test]
async fn test_read_returns_empty_for_nonexistent_type() {
    let store = common::make_store("read");
    store.append(vec![common::event("T1")], vec![]).await.unwrap();

    let results = store.read(common::type_query(&["T2"]), None).await.unwrap();
    assert!(results.is_empty());
}

// ---------------------------------------------------------------------------
// Limit and After
// ---------------------------------------------------------------------------

// TestReadWithLimit
#[tokio::test]
async fn test_read_with_limit() {
    let store = common::make_store("read");
    let events: Vec<_> = (0..5).map(|_| common::event("T")).collect();
    store.append(events, vec![]).await.unwrap();

    let results = store
        .read(
            common::type_query(&["T"]),
            Some(ReadOptions { limit: 3, after: None, reverse: false }),
        )
        .await
        .unwrap();
    assert!(results.len() <= 3);
    assert_eq!(results.len(), 3);
}

// TestReadWithAfter
#[tokio::test]
async fn test_read_with_after_returns_only_later_events() {
    let store = common::make_store("read");
    // Three separate batches so we have distinct positions.
    for _ in 0..3 {
        store.append(vec![common::event("T")], vec![]).await.unwrap();
    }

    let all = store.read(common::type_query(&["T"]), None).await.unwrap();
    assert_eq!(all.len(), 3);
    let midpoint = all[1].position;

    let after = store
        .read(
            common::type_query(&["T"]),
            Some(ReadOptions { limit: 0, after: Some(midpoint), reverse: false }),
        )
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert!(after[0].position > midpoint);
}

// TestReadWithLimitAndAfter
#[tokio::test]
async fn test_read_with_limit_and_after() {
    let store = common::make_store("read");
    for _ in 0..5 {
        store.append(vec![common::event("T")], vec![]).await.unwrap();
    }

    let all = store.read(common::type_query(&["T"]), None).await.unwrap();
    let midpoint = all[2].position;

    let results = store
        .read(
            common::type_query(&["T"]),
            Some(ReadOptions { limit: 1, after: Some(midpoint), reverse: false }),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].position > midpoint);
}

// ---------------------------------------------------------------------------
// Reverse reads
// ---------------------------------------------------------------------------

// TestRead_Reverse_ReturnsEventsInDescendingOrder
#[tokio::test]
async fn test_read_reverse_descending_order() {
    let store = common::make_store("read");
    for _ in 0..3 {
        store.append(vec![common::tagged("T", &["tag:1"])], vec![]).await.unwrap();
    }

    let results = store
        .read(
            common::tag_query(&["tag:1"]),
            Some(ReadOptions { limit: 0, after: None, reverse: true }),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 3);
    for w in results.windows(2) {
        assert!(w[0].position > w[1].position);
    }
}

// TestRead_Reverse_WithLimit
#[tokio::test]
async fn test_read_reverse_with_limit_returns_latest() {
    let store = common::make_store("read");
    for _ in 0..4 {
        store.append(vec![common::event("T")], vec![]).await.unwrap();
    }
    let all_fwd = store.read(common::type_query(&["T"]), None).await.unwrap();

    let rev = store
        .read(
            common::type_query(&["T"]),
            Some(ReadOptions { limit: 2, after: None, reverse: true }),
        )
        .await
        .unwrap();
    assert_eq!(rev.len(), 2);
    // must be the two latest
    assert_eq!(rev[0].position, all_fwd[3].position);
    assert_eq!(rev[1].position, all_fwd[2].position);
}

// TestRead_Reverse_EmptyResult
#[tokio::test]
async fn test_read_reverse_empty_for_nonexistent_tag() {
    let store = common::make_store("read");
    let results = store
        .read(
            common::tag_query(&["no-such-tag"]),
            Some(ReadOptions { limit: 0, after: None, reverse: true }),
        )
        .await
        .unwrap();
    assert!(results.is_empty());
}

// TestRead_Reverse_SingleEvent
#[tokio::test]
async fn test_read_reverse_single_event() {
    let store = common::make_store("read");
    store.append(vec![common::event("T")], vec![]).await.unwrap();

    let results = store
        .read(
            common::type_query(&["T"]),
            Some(ReadOptions { limit: 0, after: None, reverse: true }),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
}

// TestRead_Reverse_Deduplication
#[tokio::test]
async fn test_read_reverse_deduplication() {
    let store = common::make_store("read");
    // X has [shared, extra:1], Y has [shared, extra:2].
    // Both items in the OR query match events via "shared", so X and Y
    // each appear in two ranges. Dedup must collapse them to 2 unique events.
    store
        .append(
            vec![
                common::tagged("T", &["shared", "extra:1"]),
                common::tagged("T", &["shared", "extra:2"]),
            ],
            vec![],
        )
        .await
        .unwrap();

    let results = store
        .read(
            common::or_query(&[(&[], &["shared"]), (&[], &["shared"])]),
            Some(ReadOptions { limit: 0, after: None, reverse: true }),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 2, "dedup must yield exactly 2 events");
    assert!(results[0].position > results[1].position);
}

// TestRead_ForwardVsReverse_SameEvents
#[tokio::test]
async fn test_read_forward_vs_reverse_same_positions() {
    let store = common::make_store("read");
    for _ in 0..3 {
        store.append(vec![common::event("T")], vec![]).await.unwrap();
    }

    let fwd = store.read(common::type_query(&["T"]), None).await.unwrap();
    let rev = store
        .read(
            common::type_query(&["T"]),
            Some(ReadOptions { limit: 0, after: None, reverse: true }),
        )
        .await
        .unwrap();

    assert_eq!(fwd.len(), 3);
    assert_eq!(rev.len(), 3);
    assert_eq!(fwd[0].position, rev[2].position);
    assert_eq!(fwd[1].position, rev[1].position);
    assert_eq!(fwd[2].position, rev[0].position);
}
