use std::cmp::Reverse;
use std::collections::BinaryHeap;

use foundationdb::options::StreamingMode;
use foundationdb::{RangeOption, Transaction, TransactOption};
use futures::StreamExt;

use crate::encoding::{
    decode_event_value, pack_event_key, pack_events_prefix, versionstamp_to_hex,
};
use crate::error::Error;
use crate::query::{
    build_query_branches, extract_vs_from_key, intersect_branch, open_vs_stream, prefix_range,
    Range, VsStream,
};
use crate::types::{FdbStore, Query, ReadOptions, StoredEvent, Versionstamp};

// ---------------------------------------------------------------------------
// Heap item for k-way merge
// ---------------------------------------------------------------------------

// Natural order: larger vs pops first (backward scan). For forward scan,
// wrap in Reverse<HeapItem> so smaller vs pops first from the max-heap.
#[derive(Eq, PartialEq)]
struct HeapItem {
    vs: Versionstamp,
    iter_idx: usize,
}

impl Ord for HeapItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.vs.cmp(&other.vs).then_with(|| self.iter_idx.cmp(&other.iter_idx))
    }
}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

enum MergeHeap {
    Forward(BinaryHeap<Reverse<HeapItem>>),
    Backward(BinaryHeap<HeapItem>),
}

impl MergeHeap {
    fn new(reverse: bool, capacity: usize) -> Self {
        if reverse {
            MergeHeap::Backward(BinaryHeap::with_capacity(capacity))
        } else {
            MergeHeap::Forward(BinaryHeap::with_capacity(capacity))
        }
    }

    fn push(&mut self, item: HeapItem) {
        match self {
            MergeHeap::Forward(h) => h.push(Reverse(item)),
            MergeHeap::Backward(h) => h.push(item),
        }
    }

    fn pop(&mut self) -> Option<HeapItem> {
        match self {
            MergeHeap::Forward(h) => h.pop().map(|Reverse(item)| item),
            MergeHeap::Backward(h) => h.pop(),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Cap on transaction retries for reads. A read that exceeds FDB's 5-second
/// transaction limit fails with `transaction_too_old`, which is retryable —
/// without a cap, `transact_boxed` would re-run the doomed scan forever.
/// Callers hitting this should paginate with `limit` + `after`.
const READ_RETRY_LIMIT: u32 = 10;

fn read_transact_option() -> TransactOption {
    TransactOption {
        retry_limit: Some(READ_RETRY_LIMIT),
        ..TransactOption::default()
    }
}

impl FdbStore {
    pub async fn read(
        &self,
        query: Query,
        opts: Option<ReadOptions>,
    ) -> Result<Vec<StoredEvent>, Error> {
        let opts = opts.unwrap_or_default();
        let ns = self.namespace.clone();

        self.db
            .transact_boxed(
                (ns, query, opts),
                |tr, data: &mut (String, Query, ReadOptions)| {
                    let ns: &str = &data.0;
                    let query: &Query = &data.1;
                    let opts: &ReadOptions = &data.2;
                    Box::pin(read_events(tr, ns, query, opts))
                },
                read_transact_option(),
            )
            .await
    }

    /// Scan every event in the primary subspace in versionstamp order.
    /// No index is used; all events are returned regardless of type or tags.
    ///
    /// Subject to FDB's 5-second transaction limit: on stores too large to
    /// scan in one transaction this fails with `transaction_too_old` after
    /// `READ_RETRY_LIMIT` retries.
    pub async fn read_all(&self) -> Result<Vec<StoredEvent>, Error> {
        let ns = self.namespace.clone();
        self.db
            .transact_boxed(
                ns,
                |tr, ns: &mut String| Box::pin(scan_all_events(tr, ns)),
                read_transact_option(),
            )
            .await
    }
}

// ---------------------------------------------------------------------------
// K-way merge implementation
// ---------------------------------------------------------------------------

async fn read_events(
    tr: &Transaction,
    namespace: &str,
    query: &Query,
    opts: &ReadOptions,
) -> Result<Vec<StoredEvent>, Error> {
    for item in &query.items {
        if item.tags.iter().any(|t| t == "_") {
            return Err(Error::ReservedTag);
        }
    }

    // 1. Build every OR-branch (each itself an AND of one or more index
    // ranges) across all query items. No extra read is needed to do this —
    // unlike a type-discovery step, branches are derived from the query alone.
    let mut branches: Vec<Vec<Range>> = Vec::new();
    for item in &query.items {
        branches.extend(build_query_branches(namespace, item, opts.after)?);
    }

    let n = branches.len();
    if n == 0 {
        return Ok(Vec::new());
    }

    let max_matches = if opts.limit > 0 { Some(opts.limit) } else { None };

    // Fast path: single branch — no union needed, and no heap either.
    if n == 1 {
        let branch = branches.remove(0);
        let ordered_vses = intersect_branch(tr, branch, opts.reverse, true, max_matches).await?;
        return fetch_events(tr, namespace, ordered_vses).await;
    }

    // 2. One versionstamp stream per branch: a direct index scan for a
    // single-range branch, or an eagerly-computed intersection (wrapped back
    // into a stream) for a multi-range one.
    let mut streams: Vec<VsStream<'_>> = Vec::with_capacity(n);
    for branch in branches {
        if branch.len() == 1 {
            let range = branch.into_iter().next().expect("len checked above");
            streams.push(open_vs_stream(tr, range, opts.reverse, true, max_matches));
        } else {
            let vses = intersect_branch(tr, branch, opts.reverse, true, max_matches).await?;
            streams.push(Box::new(futures::stream::iter(vses.into_iter().map(Ok))));
        }
    }

    // 3. Advance each stream to its first item and seed the heap.
    let mut heap = MergeHeap::new(opts.reverse, n);
    for (i, stream) in streams.iter_mut().enumerate() {
        if let Some(vs) = stream.next().await.transpose()? {
            heap.push(HeapItem { vs, iter_idx: i });
        }
    }

    // 4. Phase 1 — k-way merge: collect ordered, deduplicated versionstamps.
    let mut ordered_vses: Vec<Versionstamp> = Vec::new();
    let mut last_emitted: Option<Versionstamp> = None;

    while let Some(item) = heap.pop() {
        let vs = item.vs;
        let idx = item.iter_idx;

        // Dedup: skip if this versionstamp was already emitted.
        // All duplicates of a given VS appear consecutively in heap order
        // (because VS is the primary sort key and a duplicate is always ≤
        // any other VS still in the heap).
        if last_emitted == Some(vs) {
            advance_and_repush(&mut streams[idx], idx, &mut heap).await?;
            continue;
        }

        ordered_vses.push(vs);
        last_emitted = Some(vs);

        if opts.limit > 0 && ordered_vses.len() >= opts.limit {
            break;
        }

        advance_and_repush(&mut streams[idx], idx, &mut heap).await?;
    }

    fetch_events(tr, namespace, ordered_vses).await
}

/// Phase 2 — batch-fetch all events from the primary subspace in parallel.
/// Issuing all tr.get futures before awaiting any of them lets the FDB client
/// pipeline the reads in a single batch round-trip.
async fn fetch_events(
    tr: &Transaction,
    namespace: &str,
    ordered_vses: Vec<Versionstamp>,
) -> Result<Vec<StoredEvent>, Error> {
    let keys: Vec<Vec<u8>> = ordered_vses
        .iter()
        .map(|&vs| pack_event_key(namespace, vs))
        .collect();
    let futs: Vec<_> = keys.iter().map(|k| tr.get(k.as_slice(), true)).collect();
    let raw_results = futures::future::join_all(futs).await;

    ordered_vses
        .into_iter()
        .zip(raw_results)
        .map(|(vs, fdb_result)| {
            let maybe_slice = fdb_result.map_err(Error::Fdb)?;
            let slice =
                maybe_slice.ok_or_else(|| Error::EventNotFound(versionstamp_to_hex(vs)))?;
            let event = decode_event_value(&slice)?;
            Ok(StoredEvent { event, position: vs })
        })
        .collect()
}

/// Advance `streams[idx]` and push the next versionstamp into the heap if present.
async fn advance_and_repush(
    stream: &mut VsStream<'_>,
    idx: usize,
    heap: &mut MergeHeap,
) -> Result<(), Error> {
    if let Some(vs) = stream.next().await.transpose()? {
        heap.push(HeapItem { vs, iter_idx: idx });
    }
    Ok(())
}

/// Linear scan of the primary events subspace: `<namespace>/e/*`.
/// Yields all events in versionstamp order without touching any index.
async fn scan_all_events(tr: &Transaction, namespace: &str) -> Result<Vec<StoredEvent>, Error> {
    let prefix = pack_events_prefix(namespace);
    let (begin, end) = prefix_range(prefix)?;
    let mut opt = RangeOption::from(begin..end);
    opt.mode = StreamingMode::WantAll;

    let mut results: Vec<StoredEvent> = Vec::new();
    let mut stream = tr.get_ranges_keyvalues(opt, true);

    while let Some(item) = stream.next().await {
        let kv = item.map_err(Error::Fdb)?;
        let vs = extract_vs_from_key(kv.key())?;
        let event = decode_event_value(kv.value())?;
        results.push(StoredEvent { event, position: vs });
    }

    Ok(results)
}
