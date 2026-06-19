mod common;

use dcb_layer::{Event, Query, QueryItem, ReadOptions};

// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_read_all_empty_store() {
    let store = common::make_store("ra");
    let events = store.read_all().await.unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn test_read_all_single_event_round_trips() {
    let store = common::make_store("ra");
    let original = common::event_with_data("OrderPlaced", &["tenant:1"], &[1, 2, 3]);
    store.append(vec![original.clone()], vec![]).await.unwrap();

    let events = store.read_all().await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event.type_name, "OrderPlaced");
    assert_eq!(events[0].event.tags, vec!["tenant:1"]);
    assert_eq!(&*events[0].event.data, &[1u8, 2, 3][..]);
}

#[tokio::test]
async fn test_read_all_returns_all_events() {
    let store = common::make_store("ra");
    store
        .append(vec![common::event("T1"), common::event("T2"), common::event("T3")], vec![])
        .await
        .unwrap();
    let events = store.read_all().await.unwrap();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn test_read_all_versionstamps_strictly_increasing() {
    let store = common::make_store("ra");
    store.append(vec![common::event("A"), common::event("B")], vec![]).await.unwrap();
    store.append(vec![common::event("C")], vec![]).await.unwrap();

    let events = store.read_all().await.unwrap();
    assert_eq!(events.len(), 3);
    for window in events.windows(2) {
        assert!(window[0].position < window[1].position, "positions must be strictly increasing");
    }
}

#[tokio::test]
async fn test_read_all_returns_all_types_and_tags() {
    let store = common::make_store("ra");
    store
        .append(
            vec![
                common::event_with_data("T1", &["tag:a"], &[]),
                common::event_with_data("T2", &[], &[42]),
                common::event_with_data("T3", &["tag:a", "tag:b"], &[1, 2]),
            ],
            vec![],
        )
        .await
        .unwrap();

    let all = store.read_all().await.unwrap();
    assert_eq!(all.len(), 3);
    let types: Vec<&str> = all.iter().map(|e| e.event.type_name.as_str()).collect();
    assert!(types.contains(&"T1"));
    assert!(types.contains(&"T2"));
    assert!(types.contains(&"T3"));
}

#[tokio::test]
async fn test_read_all_position_consistent_with_indexed_read() {
    let store = common::make_store("ra");
    store.append(vec![common::event("T")], vec![]).await.unwrap();

    let all = store.read_all().await.unwrap();
    let indexed = store
        .read(
            Query { items: vec![QueryItem { types: vec!["T".into()], tags: vec![] }] },
            Some(ReadOptions { limit: 1, after: None, reverse: false }),
        )
        .await
        .unwrap();

    assert_eq!(all.len(), 1);
    assert_eq!(indexed.len(), 1);
    assert_eq!(all[0].position, indexed[0].position);
}

#[tokio::test]
async fn test_read_all_multiple_batches_ordered() {
    let store = common::make_store("ra");
    for i in 0..5u8 {
        store
            .append(
                vec![Event { type_name: "E".into(), tags: vec![], data: vec![i].into() }],
                vec![],
            )
            .await
            .unwrap();
    }

    let events = store.read_all().await.unwrap();
    assert_eq!(events.len(), 5);
    for window in events.windows(2) {
        assert!(window[0].position < window[1].position);
    }
    let data: Vec<u8> = events.iter().map(|e| e.event.data[0]).collect();
    assert_eq!(data, vec![0, 1, 2, 3, 4]);
}
