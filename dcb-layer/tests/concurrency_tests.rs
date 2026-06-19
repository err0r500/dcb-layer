mod common;

use dcb_layer::{Error, FdbStore, Versionstamp};
use foundationdb::Database;
use futures::future::join_all;

/// N tasks simultaneously race to insert a "Singleton" event with condition
/// {types:[Singleton]}. FDB's MVCC ensures only one commits; the others get
/// a read-write conflict, retry, then see the already-committed event and
/// return AppendConditionFailed.
#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_only_one_writer_wins() {
    common::ensure_network();
    let cluster = common::fdb_cluster_file();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let ns = format!("conc_{ts}");

    let n = 10usize;
    let handles: Vec<_> = (0..n)
        .map(|_| {
            let ns = ns.clone();
            let cluster = cluster.to_string();
            tokio::spawn(async move {
                let db = Database::new(Some(cluster.as_str())).unwrap();
                FdbStore::new(db, ns)
                    .append(
                        vec![common::event("Singleton")],
                        vec![common::type_condition(&["Singleton"])],
                    )
                    .await
            })
        })
        .collect();

    let results: Vec<Result<Versionstamp, Error>> =
        join_all(handles).await.into_iter().map(|h| h.unwrap()).collect();

    let successes = results.iter().filter(|r| r.is_ok()).count();
    let cond_failures = results
        .iter()
        .filter(|r| matches!(r, Err(Error::AppendConditionFailed)))
        .count();

    assert_eq!(successes, 1, "exactly one task must win");
    assert_eq!(
        successes + cond_failures,
        n,
        "every other task must get AppendConditionFailed"
    );

    let db = Database::new(Some(cluster)).unwrap();
    let all = FdbStore::new(db, ns).read_all().await.unwrap();
    assert_eq!(all.len(), 1);
}

/// Same as above but with multiple independent slots.
#[tokio::test(flavor = "multi_thread")]
async fn test_concurrent_independent_slots_do_not_interfere() {
    common::ensure_network();
    let cluster = common::fdb_cluster_file();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    let slots = 3usize;
    let tasks_per_slot = 5usize;

    let handles: Vec<_> = (0..slots)
        .flat_map(|slot| {
            let ns = format!("conc_{ts}_slot{slot}");
            (0..tasks_per_slot).map(move |_| {
                let ns = ns.clone();
                let cluster = cluster.to_string();
                tokio::spawn(async move {
                    let db = Database::new(Some(cluster.as_str())).unwrap();
                    let result = FdbStore::new(db, ns.clone())
                        .append(
                            vec![common::event("Singleton")],
                            vec![common::type_condition(&["Singleton"])],
                        )
                        .await;
                    (slot, result)
                })
            })
        })
        .collect();

    let results: Vec<(usize, Result<Versionstamp, Error>)> =
        join_all(handles).await.into_iter().map(|h| h.unwrap()).collect();

    for slot in 0..slots {
        let wins = results.iter().filter(|(s, r)| *s == slot && r.is_ok()).count();
        assert_eq!(wins, 1, "slot {slot} must have exactly one winner");
    }
}
