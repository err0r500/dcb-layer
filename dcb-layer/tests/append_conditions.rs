mod common;

use dcb_layer::{AppendCondition, Error, Event, Query, QueryItem, ReadOptions};


// ---------------------------------------------------------------------------
// No-condition tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_append_no_conditions() {
    let store = common::make_store("ac");
    store.append(vec![common::event("T")], vec![]).await.unwrap();
}

#[tokio::test]
async fn test_append_empty_slice_returns_error() {
    let store = common::make_store("ac");
    let err = store.append(vec![], vec![]).await.unwrap_err();
    assert!(matches!(err, Error::EmptyEvents));
}

// ---------------------------------------------------------------------------
// Single condition tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_append_condition_does_not_exist_passes() {
    let store = common::make_store("ac");
    store
        .append(vec![common::event("Other")], vec![common::type_condition(&["T"])])
        .await
        .unwrap();
}

#[tokio::test]
async fn test_append_condition_exists_fails() {
    let store = common::make_store("ac");
    store.append(vec![common::event("T")], vec![]).await.unwrap();
    let err = store
        .append(vec![common::event("X")], vec![common::type_condition(&["T"])])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::AppendConditionFailed));
}

#[tokio::test]
async fn test_append_condition_failed_leaves_store_unchanged() {
    let store = common::make_store("ac");
    store.append(vec![common::event("T")], vec![]).await.unwrap();
    let _ = store
        .append(vec![common::event("X")], vec![common::type_condition(&["T"])])
        .await;
    let events = store
        .read(
            Query { items: vec![QueryItem { types: vec!["X".into()], tags: vec![] }] },
            None,
        )
        .await
        .unwrap();
    assert!(events.is_empty(), "X must not have been written");
}

// ---------------------------------------------------------------------------
// Multiple conditions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_append_multiple_conditions_all_pass() {
    let store = common::make_store("ac");
    store
        .append(
            vec![common::event("X")],
            vec![common::type_condition(&["T1"]), common::type_condition(&["T2"])],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn test_append_multiple_conditions_first_fails() {
    let store = common::make_store("ac");
    store.append(vec![common::event("T1")], vec![]).await.unwrap();
    let err = store
        .append(
            vec![common::event("X")],
            vec![common::type_condition(&["T1"]), common::type_condition(&["T2"])],
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::AppendConditionFailed));
}

#[tokio::test]
async fn test_append_multiple_conditions_second_fails() {
    let store = common::make_store("ac");
    store.append(vec![common::event("T2")], vec![]).await.unwrap();
    let err = store
        .append(
            vec![common::event("X")],
            vec![common::type_condition(&["T1"]), common::type_condition(&["T2"])],
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::AppendConditionFailed));
}

// ---------------------------------------------------------------------------
// Condition with `after`
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_append_condition_after_skips_earlier_events() {
    let store = common::make_store("ac");
    store.append(vec![common::event("T")], vec![]).await.unwrap();
    let stored = store
        .read(
            Query { items: vec![QueryItem { types: vec!["T".into()], tags: vec![] }] },
            Some(ReadOptions { limit: 1, after: None, reverse: false }),
        )
        .await
        .unwrap();
    let position = stored[0].position;

    let cond = AppendCondition {
        query: Query {
            items: vec![QueryItem { types: vec!["T".into()], tags: vec![] }],
        },
        after: Some(position),
    };
    store.append(vec![common::event("Y")], vec![cond]).await.unwrap();
}

// ---------------------------------------------------------------------------
// Fix #2 — batch size guard
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_append_batch_too_large_returns_error() {
    let store = common::make_store("ac");
    let events: Vec<Event> = (0..=u16::MAX as usize + 1)
        .map(|i| Event { type_name: format!("T{i}").into(), tags: vec![], data: vec![].into() })
        .collect();
    let err = store.append(events, vec![]).await.unwrap_err();
    assert!(matches!(err, Error::BatchTooLarge), "expected BatchTooLarge, got {err:?}");
}

// ---------------------------------------------------------------------------
// Fix #10 — upfront condition query validation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_append_condition_with_empty_query_item_returns_invalid_query() {
    let store = common::make_store("ac");
    let cond = AppendCondition {
        query: Query { items: vec![QueryItem { types: vec![], tags: vec![] }] },
        after: None,
    };
    let err = store.append(vec![common::event("X")], vec![cond]).await.unwrap_err();
    assert!(matches!(err, Error::InvalidQuery), "expected InvalidQuery, got {err:?}");
}

#[tokio::test]
async fn test_append_condition_with_empty_items_returns_invalid_query() {
    let store = common::make_store("ac");
    let cond = AppendCondition { query: Query { items: vec![] }, after: None };
    let err = store.append(vec![common::event("X")], vec![cond]).await.unwrap_err();
    assert!(matches!(err, Error::InvalidQuery), "expected InvalidQuery, got {err:?}");
}
