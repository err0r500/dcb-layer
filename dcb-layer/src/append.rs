use foundationdb::options::{MutationType, TransactionOption};
use foundationdb::tuple::Versionstamp as FdbVs;
use foundationdb::{FdbError, Transaction};

use crate::encoding::{
    encode_event_value, pack_event_key_fdb, pack_sentinel_key, pack_tag_index_key_fdb,
    pack_type_index_key_fdb, sort_tags,
};
use crate::error::Error;
use crate::query::{build_query_branches, intersect_branch};
use crate::types::{AppendCondition, Event, FdbStore, Versionstamp};

/// Cap on `on_error` retries for an append transaction, so pathological
/// contention terminates with an error instead of looping forever.
/// RetryLimit persists across transaction resets (FDB API >= 610).
const APPEND_RETRY_LIMIT: i32 = 100;

impl FdbStore {
    /// Write one event into all FDB indexes inside an existing transaction.
    pub(crate) fn append_single(
        tr: &Transaction,
        namespace: &str,
        event: &Event,
        batch_index: u16,
    ) -> Result<(), Error> {
        let sorted_tags = sort_tags(&event.tags);
        let type_name: &str = event.type_name.as_ref();

        let event_key = pack_event_key_fdb(namespace, FdbVs::incomplete(batch_index));
        let event_value = encode_event_value(event);
        tr.atomic_op(&event_key, &event_value, MutationType::SetVersionstampedKey);

        for tag in &sorted_tags {
            let tag_key = pack_tag_index_key_fdb(namespace, tag, FdbVs::incomplete(batch_index));
            tr.atomic_op(&tag_key, &[], MutationType::SetVersionstampedKey);
        }

        let type_key = pack_type_index_key_fdb(namespace, type_name, FdbVs::incomplete(batch_index));
        tr.atomic_op(&type_key, &[], MutationType::SetVersionstampedKey);

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
        let ns = self.namespace.clone();
        let sentinel_key = pack_sentinel_key(&ns);
        // Value: 12-byte versionstamp placeholder at offset 0, then 4-byte LE offset.
        // FDB fills bytes 0–9 with the tx version at commit; the 4-byte suffix is stripped.
        // Stored value is always 12 unique bytes, ensuring the watch fires every append.
        let mut sentinel_val = [0u8; 16];
        sentinel_val[12..].copy_from_slice(&0u32.to_le_bytes());
        let mut tr = self.db.create_trx().map_err(Error::Fdb)?;
        tr.set_option(TransactionOption::RetryLimit(APPEND_RETRY_LIMIT))
            .map_err(Error::Fdb)?;

        loop {
            // Client-managed idempotency id: the FDB client resolves
            // commit_unknown_result itself instead of surfacing it, so this
            // retry loop can never double-apply the batch. Set every iteration
            // because on_error resets transaction options (RetryLimit, set
            // once above, is one of the few that persists across resets).
            tr.set_option(TransactionOption::AutomaticIdempotency)
                .map_err(Error::Fdb)?;

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
                            .map(|item| query_item_exists(&tr, &ns, item, cond.after))
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
                FdbStore::append_single(&tr, &ns, event, i as u16)?;
            }

            tr.atomic_op(&sentinel_key, &sentinel_val, MutationType::SetVersionstampedValue);

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
                    return Ok(last_vs);
                }
                Err(commit_err) => {
                    // on_error does exponential backoff and resets the transaction
                    // for reuse if the error is retryable; otherwise it propagates.
                    tr = commit_err.on_error().await.map_err(Error::Fdb)?;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Condition helpers
// ---------------------------------------------------------------------------

/// Does any event matching `item` (after `after`) exist? Each branch only
/// needs its first intersection match, so every probe caps out at 1 result.
async fn query_item_exists(
    tr: &Transaction,
    namespace: &str,
    item: &crate::types::QueryItem,
    after: Option<Versionstamp>,
) -> Result<bool, Error> {
    let branches = build_query_branches(namespace, item, after)?;
    // Issue every probe before awaiting any: the FDB client pipelines them.
    let futs = branches
        .into_iter()
        .map(|branch| intersect_branch(tr, branch, false, false, Some(1)));
    for result in futures::future::join_all(futs).await {
        if !result?.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}
