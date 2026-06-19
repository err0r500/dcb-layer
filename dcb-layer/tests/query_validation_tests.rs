mod common;

use dcb_layer::{AppendCondition, Error, Query, QueryItem};

// TestReadEmptyQuery — both types and tags empty → InvalidQuery
#[tokio::test]
async fn test_read_empty_query_returns_invalid_query_error() {
    let store = common::make_store("qval");
    let err = store
        .read(Query { items: vec![QueryItem { types: vec![], tags: vec![] }] }, None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::InvalidQuery));
}

// TestReadQueryWithEmptyTypes — treated as tags-only
#[tokio::test]
async fn test_read_query_with_empty_types_is_tags_only() {
    let store = common::make_store("qval");
    store
        .append(vec![common::tagged("T", &["someTag"])], vec![])
        .await
        .unwrap();

    // types:[] tags:[someTag] → tags-only query
    let results = store.read(common::tag_query(&["someTag"]), None).await.unwrap();
    assert_eq!(results.len(), 1);
}

// TestReadQueryWithEmptyTags — treated as type-only
#[tokio::test]
async fn test_read_query_with_empty_tags_is_type_only() {
    let store = common::make_store("qval");
    store.append(vec![common::event("someType")], vec![]).await.unwrap();

    // types:[someType] tags:[] → type-only query
    let results = store.read(common::type_query(&["someType"]), None).await.unwrap();
    assert_eq!(results.len(), 1);
}

// TestVersionstampCompare and TestVersionstampString are already covered
// by unit tests in src/encoding.rs.

#[tokio::test]
async fn test_append_event_with_reserved_tag_underscore_returns_error() {
    let store = common::make_store("qval_reserved");
    let err = store
        .append(vec![common::tagged("T", &["_"])], vec![])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::ReservedTag));
}

#[tokio::test]
async fn test_read_query_with_reserved_tag_underscore_returns_error() {
    let store = common::make_store("qval_reserved");
    let err = store
        .read(common::tag_query(&["_"]), None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::ReservedTag));
}

#[tokio::test]
async fn test_append_condition_with_reserved_tag_underscore_returns_error() {
    let store = common::make_store("qval_reserved");
    let cond = AppendCondition { query: common::tag_query(&["_"]), after: None };
    let err = store
        .append(vec![common::event("T")], vec![cond])
        .await
        .unwrap_err();
    assert!(matches!(err, Error::ReservedTag));
}
