//! Ports the condition-semantics matrix from the Go concurrency test suite.
//!
//! Each test uses Way 1 (tx1 appends BEFORE tx2 checks): append the "conflicting"
//! event first, then attempt the conditional append. This tests the condition
//! matching logic exhaustively without needing a lower-level transaction hook.
//!
//! Way 2 (FDB read-write conflict detection during concurrent commit) would require
//! injecting work between `queryExists` and the commit inside a single transaction,
//! which needs a test hook not exposed by the current public API.

mod common;

use dcb_layer::{AppendCondition, Error};

fn cond(types: &[&str], tags: &[&str]) -> AppendCondition {
    AppendCondition {
        query: common::or_query(&[(types, tags)]),
        after: None,
    }
}

fn multi_item_cond(items: &[(&[&str], &[&str])]) -> AppendCondition {
    AppendCondition { query: common::or_query(items), after: None }
}

async fn assert_conflict(
    store: &dcb_layer::FdbStore,
    existing: dcb_layer::Event,
    condition: AppendCondition,
) {
    store.append(vec![existing], vec![]).await.unwrap();
    let err = store
        .append(vec![common::event("X")], vec![condition])
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::AppendConditionFailed),
        "expected AppendConditionFailed"
    );
}

async fn assert_no_conflict(
    store: &dcb_layer::FdbStore,
    existing: dcb_layer::Event,
    condition: AppendCondition,
) {
    store.append(vec![existing], vec![]).await.unwrap();
    store
        .append(vec![common::event("X")], vec![condition])
        .await
        .unwrap();
}

// ---------------------------------------------------------------------------
// Conflict cases — condition SHOULD fail
// ---------------------------------------------------------------------------

#[tokio::test]
async fn conflict_exact_match() {
    let store = common::make_store("csem");
    assert_conflict(
        &store,
        common::tagged("T", &["tagA"]),
        cond(&["T"], &["tagA"]),
    )
    .await;
}

#[tokio::test]
async fn conflict_wider_match_tags() {
    // event has [tA,tB]; condition targets only tA → still matches
    let store = common::make_store("csem");
    assert_conflict(
        &store,
        common::tagged("T", &["tA", "tB"]),
        cond(&["T"], &["tA"]),
    )
    .await;
}

#[tokio::test]
async fn conflict_wider_match_type_only() {
    // event has tags but condition ignores them
    let store = common::make_store("csem");
    assert_conflict(&store, common::tagged("T", &["tA"]), cond(&["T"], &[])).await;
}

#[tokio::test]
async fn conflict_wider_match_tags_only() {
    // condition is tags-only; event type is irrelevant
    let store = common::make_store("csem");
    assert_conflict(&store, common::tagged("T", &["tA"]), cond(&[], &["tA"])).await;
}

#[tokio::test]
async fn conflict_wider_match_type_plus_another() {
    // condition covers T1 OR T2; only T1 exists → still fires
    let store = common::make_store("csem");
    assert_conflict(&store, common::event("T1"), cond(&["T1", "T2"], &[])).await;
}

#[tokio::test]
async fn conflict_wider_match_two_item_condition() {
    // Two-item OR condition: [{types:[T1]}, {types:[T2]}]; T1 exists
    let store = common::make_store("csem");
    assert_conflict(
        &store,
        common::event("T1"),
        multi_item_cond(&[(&["T1"], &[]), (&["T2"], &[])]),
    )
    .await;
}

#[tokio::test]
async fn conflict_wider_match_tags_only_multiple_tags() {
    // condition={tags:[t1,t2]}; event has exactly [t1,t2]
    let store = common::make_store("csem");
    assert_conflict(
        &store,
        common::tagged("T", &["t1", "t2"]),
        cond(&[], &["t1", "t2"]),
    )
    .await;
}

#[tokio::test]
async fn conflict_wider_match_tags_only_event_superset() {
    // event has [t1,t2,t3]; condition targets subset [t1,t2] → still matches
    let store = common::make_store("csem");
    assert_conflict(
        &store,
        common::tagged("T", &["t1", "t2", "t3"]),
        cond(&[], &["t1", "t2"]),
    )
    .await;
}

// ---------------------------------------------------------------------------
// No-conflict cases — condition should NOT fail
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_conflict_different_type() {
    // event=(T1,tagA), condition={types:[T2],tags:[tagA]} → T2 absent
    let store = common::make_store("csem");
    assert_no_conflict(
        &store,
        common::tagged("T1", &["tagA"]),
        cond(&["T2"], &["tagA"]),
    )
    .await;
}

#[tokio::test]
async fn no_conflict_subset_tags() {
    // event has [t1]; condition requires [t1,t2] → t2 absent on event
    let store = common::make_store("csem");
    assert_no_conflict(
        &store,
        common::tagged("T", &["t1"]),
        cond(&["T"], &["t1", "t2"]),
    )
    .await;
}

#[tokio::test]
async fn no_conflict_different_tags() {
    // event has [t1,t2]; condition wants [t1,t3] → t3 absent
    let store = common::make_store("csem");
    assert_no_conflict(
        &store,
        common::tagged("T", &["t1", "t2"]),
        cond(&["T"], &["t1", "t3"]),
    )
    .await;
}

#[tokio::test]
async fn no_conflict_type_match_but_no_tag_match() {
    // event=(T,[t1]); condition={types:[T],tags:[t2]} → tag t2 never indexed
    let store = common::make_store("csem");
    assert_no_conflict(
        &store,
        common::tagged("T", &["t1"]),
        cond(&["T"], &["t2"]),
    )
    .await;
}

#[tokio::test]
async fn no_conflict_multiple_types_none_match_with_tags() {
    // event=(T1,[tag]); condition={types:[T2,T3],tags:[tag]} → T2/T3 absent
    let store = common::make_store("csem");
    assert_no_conflict(
        &store,
        common::tagged("T1", &["tag"]),
        cond(&["T2", "T3"], &["tag"]),
    )
    .await;
}

#[tokio::test]
async fn no_conflict_multiple_types_none_match_no_tags() {
    // event=(T1); condition={types:[T2,T3]}
    let store = common::make_store("csem");
    assert_no_conflict(&store, common::event("T1"), cond(&["T2", "T3"], &[])).await;
}

#[tokio::test]
async fn no_conflict_multiple_query_items_none_match() {
    // event=(T1,[t1]); condition=[{types:[T2]},{tags:[t2]}]
    let store = common::make_store("csem");
    assert_no_conflict(
        &store,
        common::tagged("T1", &["t1"]),
        multi_item_cond(&[(&["T2"], &[]), (&[], &["t2"])]),
    )
    .await;
}

#[tokio::test]
async fn no_conflict_tags_only_event_missing_tag() {
    // event has only [t1]; condition requires [t1,t2]
    let store = common::make_store("csem");
    assert_no_conflict(
        &store,
        common::tagged("T", &["t1"]),
        cond(&[], &["t1", "t2"]),
    )
    .await;
}
