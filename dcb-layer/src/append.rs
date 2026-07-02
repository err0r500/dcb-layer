use foundationdb::options::{MutationType, StreamingMode, TransactionOption};
use foundationdb::tuple::Versionstamp as FdbVs;
use foundationdb::{FdbError, RangeOption, Transaction};
use futures::{Stream, StreamExt};

use crate::encoding::{
    encode_event_value, generate_superset_presorted, pack_event_key_fdb,
    pack_sentinel_key, pack_tag_index_key_fdb, sort_tags,
};
use crate::error::Error;
use crate::query::build_query_ranges;
use crate::types::{AppendCondition, Event, FdbStore, Query, Versionstamp};

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
        let ns = self.namespace.clone();
        let sentinel_key = pack_sentinel_key(&ns);
        // Value: 12-byte versionstamp placeholder at offset 0, then 4-byte LE offset.
        // FDB fills bytes 0–9 with the tx version at commit; the 4-byte suffix is stripped.
        // Stored value is always 12 unique bytes, ensuring the watch fires every append.
        let mut sentinel_val = [0u8; 16];
        sentinel_val[12..].copy_from_slice(&0u32.to_le_bytes());
        let mut tr = self.db.create_trx().map_err(Error::Fdb)?;

        loop {
            // Client-managed idempotency id: the FDB client resolves
            // commit_unknown_result itself instead of surfacing it, so this
            // retry loop can never double-apply the batch. Set every iteration
            // because on_error resets transaction options.
            tr.set_option(TransactionOption::AutomaticIdempotency)
                .map_err(Error::Fdb)?;

            // Condition check — runs inside the same transaction as the writes.
            // On a retryable FDB error, reset the transaction and retry the whole loop.
            let mut retry = false;
            for cond in &conditions {
                match query_exists(&tr, &ns, &cond.query, cond.after).await {
                    Ok(true) => return Err(Error::AppendConditionFailed),
                    Ok(false) => {}
                    Err(Error::Fdb(e)) => {
                        tr = tr.on_error(e).await.map_err(Error::Fdb)?;
                        retry = true;
                        break;
                    }
                    Err(other) => return Err(other),
                }
            }
            if retry {
                continue;
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

async fn query_exists(
    tr: &Transaction,
    namespace: &str,
    query: &Query,
    after: Option<Versionstamp>,
) -> Result<bool, Error> {
    for item in &query.items {
        if query_item_exists(tr, namespace, item, after).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn query_item_exists(
    tr: &Transaction,
    namespace: &str,
    item: &crate::types::QueryItem,
    after: Option<Versionstamp>,
) -> Result<bool, Error> {
    let ranges = build_query_ranges(tr, namespace, item, after, false).await?;
    for (begin, end) in ranges {
        let mut opt = RangeOption::from(begin..end);
        opt.limit = Some(1);
        opt.mode = StreamingMode::Small;
        let mut stream = tr.get_ranges_keyvalues(opt, false);
        if consume_first_kv(&mut stream).await? {
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


}
