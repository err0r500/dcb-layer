use foundationdb::options::{MutationType, StreamingMode, TransactionOption};
use foundationdb::tuple::Versionstamp as FdbVs;
use foundationdb::{FdbError, RangeOption, Transaction};
use futures::{Stream, StreamExt};

use std::sync::atomic::{AtomicU32, Ordering};

use crate::encoding::{
    encode_event_value, generate_superset_presorted, pack_event_key_fdb,
    pack_sentinel_shard_key, pack_tag_index_key_fdb, pack_txid_key, sort_tags, SENTINEL_SHARDS,
};
use crate::error::Error;
use crate::query::build_query_ranges;
use crate::types::{AppendCondition, Event, FdbStore, Versionstamp};

/// Cap on `on_error` retries for an append transaction, so pathological
/// contention terminates with an error instead of looping forever.
/// RetryLimit persists across transaction resets (FDB API >= 610).
const APPEND_RETRY_LIMIT: i32 = 100;

/// Process-wide round-robin cursor for sentinel shard selection. Shared by the
/// whole process (not per-`FdbStore`) so that workloads constructing a fresh
/// store/handle per append — e.g. the concurrency tests and the Elixir NIF —
/// still spread across shards instead of every short-lived instance restarting
/// at shard 0 and collapsing back onto one hot key.
static SENTINEL_SHARD_CTR: AtomicU32 = AtomicU32::new(0);

/// Pick the next sentinel shard, round-robin across `SENTINEL_SHARDS`.
fn next_sentinel_shard() -> u32 {
    SENTINEL_SHARD_CTR.fetch_add(1, Ordering::Relaxed) % SENTINEL_SHARDS
}

impl FdbStore {
    /// Write one event into all FDB indexes inside an existing transaction.
    pub(crate) fn append_single(
        tr: &Transaction,
        namespace: &str,
        event: &Event,
        batch_index: u16,
    ) -> Result<(), Error> {
        // Sort/dedup as &str refs: subsets below then copy pointers, not Strings.
        let tag_refs: Vec<&str> = event.tags.iter().map(String::as_str).collect();
        let sorted_tags = sort_tags(&tag_refs);
        let type_name: &str = event.type_name.as_ref();

        let event_key = pack_event_key_fdb(namespace, FdbVs::incomplete(batch_index));
        let event_value = encode_event_value(event);
        tr.atomic_op(&event_key, &event_value, MutationType::SetVersionstampedKey);

        for subset in generate_superset_presorted(&sorted_tags) {
            let tag_key = pack_tag_index_key_fdb(
                namespace,
                &subset,
                type_name,
                FdbVs::incomplete(batch_index),
            );
            tr.atomic_op(&tag_key, &[], MutationType::SetVersionstampedKey);
        }

        Ok(())
    }

    /// Append events atomically, checking all conditions in the same transaction.
    ///
    /// Returns the `Versionstamp` (position) of the **last** event in the batch.
    /// For a single-event batch that is also the only event's position.
    pub async fn append(
        &self,
        events: Vec<Event>,
        conditions: Vec<AppendCondition>,
    ) -> Result<Versionstamp, Error> {
        if events.is_empty() {
            return Err(Error::EmptyEvents);
        }
        if events.len() > u16::MAX as usize {
            return Err(Error::BatchTooLarge);
        }
        for event in &events {
            if event.type_name.is_empty() {
                return Err(Error::MissingEventType);
            }
            if event.tags.len() > 10 {
                return Err(Error::TooManyTags);
            }
            if event.tags.iter().any(|t| t == "_") {
                return Err(Error::ReservedTag);
            }
        }
        for cond in &conditions {
            if cond.query.items.is_empty() {
                return Err(Error::InvalidQuery);
            }
            for item in &cond.query.items {
                if item.has_no_type_nor_tags() {
                    return Err(Error::InvalidQuery);
                }
                if item.tags.iter().any(|t| t == "_") {
                    return Err(Error::ReservedTag);
                }
            }
        }

        let n = events.len();
        let ns: &str = &self.namespace;
        // One shard picked per append() call (reused across FDB retries of this
        // call, matching the old single-key behavior). A watcher arms all shards,
        // so whichever shard we land on still wakes every active subscriber.
        let sentinel_key = pack_sentinel_shard_key(ns, next_sentinel_shard());
        // Value: 12-byte versionstamp placeholder at offset 0, then 4-byte LE offset.
        // FDB fills bytes 0–9 with the tx version at commit; the 4-byte suffix is stripped.
        // Stored value is unique per commit, ensuring the watch fires every append.
        let mut sentinel_val = [0u8; 16];
        sentinel_val[12..].copy_from_slice(&0u32.to_le_bytes());

        // Idempotency marker: unique per append() call. Written in the same
        // transaction; on a maybe-committed error the retry reads it back to
        // decide whether the previous commit actually landed (FDB's documented
        // "unique side effect" recipe for commit_unknown_result).
        let mut txid = [0u8; 16];
        getrandom::getrandom(&mut txid).map_err(|e| Error::RandomSource(e.to_string()))?;
        let txid_key = pack_txid_key(ns, &txid);
        // Same versionstamped-value layout as the sentinel, but bytes 10–11
        // pre-set to the last batch index so the stored 12 bytes equal the
        // last event's position exactly.
        let mut txid_val = [0u8; 16];
        txid_val[10..12].copy_from_slice(&((n - 1) as u16).to_be_bytes());
        txid_val[12..].copy_from_slice(&0u32.to_le_bytes());

        let mut tr = self.db.create_trx().map_err(Error::Fdb)?;
        tr.set_option(TransactionOption::RetryLimit(APPEND_RETRY_LIMIT))
            .map_err(Error::Fdb)?;
        let mut maybe_committed = false;

        loop {
            // Recovery check after a maybe-committed error: if the marker is
            // readable, the previous commit succeeded — return its position.
            if maybe_committed {
                match tr.get(&txid_key, false).await {
                    Ok(Some(val)) => {
                        if val.len() != 12 {
                            return Err(Error::TupleDecode(format!(
                                "txid marker has {} bytes, expected 12",
                                val.len()
                            )));
                        }
                        let mut vs = [0u8; 12];
                        vs.copy_from_slice(&val);
                        self.cleanup_txid_marker(&txid_key).await;
                        return Ok(vs);
                    }
                    Ok(None) => {
                        maybe_committed = false;
                    }
                    Err(e) => {
                        tr = tr.on_error(e).await.map_err(Error::Fdb)?;
                        continue;
                    }
                }
            }

            // Condition checks — inside the same transaction as the writes so
            // the (empty) ranges become read conflict ranges. All existence
            // probes are issued concurrently and pipelined by the FDB client.
            {
                let futs: Vec<_> = conditions
                    .iter()
                    .flat_map(|cond| {
                        cond.query
                            .items
                            .iter()
                            .map(|item| query_item_exists(&tr, ns, item, cond.after))
                    })
                    .collect();
                let results = futures::future::join_all(futs).await;

                let mut matched = false;
                let mut fdb_err: Option<FdbError> = None;
                let mut app_err: Option<Error> = None;
                for r in results {
                    match r {
                        Ok(true) => matched = true,
                        Ok(false) => {}
                        Err(Error::Fdb(e)) => {
                            fdb_err.get_or_insert(e);
                        }
                        Err(e) => {
                            app_err.get_or_insert(e);
                        }
                    }
                }
                // Retryable FDB errors win: retry re-checks every condition,
                // so deferring the verdict is always safe.
                if let Some(e) = fdb_err {
                    tr = tr.on_error(e).await.map_err(Error::Fdb)?;
                    continue;
                }
                if let Some(e) = app_err {
                    return Err(e);
                }
                if matched {
                    return Err(Error::AppendConditionFailed);
                }
            }

            for (i, event) in events.iter().enumerate() {
                FdbStore::append_single(&tr, ns, event, i as u16)?;
            }

            tr.atomic_op(&sentinel_key, &sentinel_val, MutationType::SetVersionstampedValue);
            tr.atomic_op(&txid_key, &txid_val, MutationType::SetVersionstampedValue);

            // Capture the versionstamp future BEFORE commit — the future is backed
            // by the C-level FDB future and remains valid after the transaction is
            // committed and dropped.
            let vs_future = tr.get_versionstamp();

            match tr.commit().await {
                Ok(_committed) => {
                    // Await now that the commit has succeeded: FDB fills in the
                    // 10-byte transaction version.
                    let fdb_slice = vs_future.await.map_err(Error::Fdb)?;
                    // Last event's position: tx_version (10 B) || (n-1) as u16 BE.
                    let mut last_vs = [0u8; 12];
                    last_vs[..10].copy_from_slice(&fdb_slice);
                    let user_bytes = ((n - 1) as u16).to_be_bytes();
                    last_vs[10] = user_bytes[0];
                    last_vs[11] = user_bytes[1];
                    self.cleanup_txid_marker(&txid_key).await;
                    return Ok(last_vs);
                }
                Err(commit_err) => {
                    if commit_err.is_maybe_committed() {
                        maybe_committed = true;
                    }
                    // on_error does exponential backoff and resets the transaction
                    // for reuse if the error is retryable; otherwise it propagates.
                    tr = commit_err.on_error().await.map_err(Error::Fdb)?;
                }
            }
        }
    }

    /// Best-effort removal of the idempotency marker after a resolved append.
    /// Failure is harmless: an orphaned marker is ~50 bytes and never read
    /// again (txids are random per call).
    async fn cleanup_txid_marker(&self, txid_key: &[u8]) {
        if let Ok(tr) = self.db.create_trx() {
            tr.clear(txid_key);
            let _ = tr.commit().await;
        }
    }
}

// ---------------------------------------------------------------------------
// Condition helpers
// ---------------------------------------------------------------------------

pub(crate) async fn consume_first_kv<S, T>(stream: &mut S) -> Result<bool, Error>
where
    S: Stream<Item = Result<T, FdbError>> + Unpin,
{
    match stream.next().await {
        Some(Ok(_)) => Ok(true),
        Some(Err(e)) => Err(e.into()),
        None => Ok(false),
    }
}

async fn query_item_exists(
    tr: &Transaction,
    namespace: &str,
    item: &crate::types::QueryItem,
    after: Option<Versionstamp>,
) -> Result<bool, Error> {
    let ranges = build_query_ranges(tr, namespace, item, after, false).await?;
    // Issue every probe before awaiting any: the FDB client pipelines them.
    let futs: Vec<_> = ranges
        .into_iter()
        .map(|(begin, end)| {
            let mut opt = RangeOption::from(begin..end);
            opt.limit = Some(1);
            opt.mode = StreamingMode::Small;
            let mut stream = tr.get_ranges_keyvalues(opt, false);
            async move { consume_first_kv(&mut stream).await }
        })
        .collect();
    for result in futures::future::join_all(futs).await {
        if result? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn fdb_read_error_propagates_not_swallowed() {
        let mut s = stream::iter([Err::<(), FdbError>(FdbError::from_code(1000))]);
        assert!(consume_first_kv(&mut s).await.is_err());
    }

    #[tokio::test]
    async fn empty_range_returns_false() {
        let mut s = stream::iter([] as [Result<(), FdbError>; 0]);
        assert!(!consume_first_kv(&mut s).await.unwrap());
    }

    #[tokio::test]
    async fn found_kv_returns_true() {
        let mut s = stream::iter([Ok::<(), FdbError>(())]);
        assert!(consume_first_kv(&mut s).await.unwrap());
    }

    #[test]
    fn sentinel_shard_round_robin_visits_every_shard() {
        use std::collections::HashSet;
        // Local counter mirrors `next_sentinel_shard`'s logic, avoiding
        // cross-test interference on the process-wide static.
        let ctr = AtomicU32::new(0);
        let seen: HashSet<u32> = (0..SENTINEL_SHARDS * 3)
            .map(|_| ctr.fetch_add(1, Ordering::Relaxed) % SENTINEL_SHARDS)
            .collect();
        assert_eq!(seen.len(), SENTINEL_SHARDS as usize);
    }
}
