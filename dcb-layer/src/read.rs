use std::cmp::Reverse;
use std::collections::BinaryHeap;

use foundationdb::future::FdbValue;
use foundationdb::options::StreamingMode;
use foundationdb::{FdbResult, RangeOption, Transaction, TransactOption};
use futures::{Stream, StreamExt};

use crate::encoding::{
    decode_event_value, pack_event_key, pack_events_prefix, versionstamp_to_hex,
};
use crate::error::Error;
use crate::query::{build_query_ranges, prefix_range};
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
                TransactOption::default(),
            )
            .await
    }

    /// Scan every event in the primary subspace in versionstamp order.
    /// No index is used; all events are returned regardless of type or tags.
    pub async fn read_all(&self) -> Result<Vec<StoredEvent>, Error> {
        let ns = self.namespace.clone();
        self.db
            .transact_boxed(
                ns,
                |tr, ns: &mut String| Box::pin(scan_all_events(tr, ns)),
                TransactOption::default(),
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

    // 1. Build all key ranges, parallelising across query items (OR branches).
    let range_futures = query.items.iter().map(|item| build_query_ranges(tr, namespace, item, opts.after, true));
    let range_results = futures::future::join_all(range_futures).await;
    let mut all_ranges: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for r in range_results {
        all_ranges.extend(r?);
    }

    let n = all_ranges.len();

    if n == 0 {
        return Ok(Vec::new());
    }

    // Fast path: single range — skip the heap entirely (no merge, no dedup needed).
    if n == 1 {
        let (begin, end) = all_ranges.remove(0);
        let mut opt = RangeOption::from(begin..end);
        if opts.limit > 0 {
            opt.limit = Some(opts.limit);
            opt.mode = StreamingMode::WantAll;
        }
        opt.reverse = opts.reverse;
        let mut stream = tr.get_ranges_keyvalues(opt, true);
        let mut ordered_vses: Vec<Versionstamp> = Vec::new();
        while let Some(result) = stream.next().await {
            let kv = result.map_err(Error::Fdb)?;
            ordered_vses.push(extract_vs_from_key(kv.key())?);
            if opts.limit > 0 && ordered_vses.len() >= opts.limit {
                break;
            }
        }
        return fetch_events(tr, namespace, ordered_vses).await;
    }

    // 2. Open one stream per range.
    let mut streams: Vec<Box<dyn Stream<Item = FdbResult<Vec<u8>>> + Unpin + Send + '_>> =
        Vec::with_capacity(n);

    for (begin, end) in all_ranges {
        let mut opt = RangeOption::from(begin..end);
        if opts.limit > 0 {
            opt.limit = Some(opts.limit);
        }
        opt.reverse = opts.reverse;
        let s = tr
            .get_ranges_keyvalues(opt, true)
            .map(|r: FdbResult<FdbValue>| r.map(|kv| kv.key().to_vec()));
        streams.push(Box::new(s));
    }

    // 3. Advance each stream to its first item and seed the heap.
    let mut heap = MergeHeap::new(opts.reverse, n);
    for (i, stream) in streams.iter_mut().enumerate() {
        if let Some(vs) = next_vs(stream).await? {
            heap.push(HeapItem { vs, iter_idx: i });
        }
    }

    // 4. Phase 1 — k-way merge: collect ordered, deduplicated versionstamps.
    let mut ordered_vses: Vec<Versionstamp> = Vec::new();
    let mut last_emitted: Option<Versionstamp> = None;

    loop {
        let item = match heap.pop() {
            Some(x) => x,
            None => break,
        };
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
async fn advance_and_repush<'a>(
    stream: &mut (dyn Stream<Item = FdbResult<Vec<u8>>> + Unpin + Send + 'a),
    idx: usize,
    heap: &mut MergeHeap,
) -> Result<(), Error> {
    if let Some(vs) = next_vs(stream).await? {
        heap.push(HeapItem { vs, iter_idx: idx });
    }
    Ok(())
}

/// Pull the next item from a stream and decode the versionstamp from its key bytes.
async fn next_vs(
    stream: &mut (dyn Stream<Item = FdbResult<Vec<u8>>> + Unpin + Send),
) -> Result<Option<Versionstamp>, Error> {
    match stream.next().await {
        Some(Ok(key_bytes)) => Ok(Some(extract_vs_from_key(&key_bytes)?)),
        Some(Err(e)) => Err(Error::Fdb(e)),
        None => Ok(None),
    }
}

/// Extract the trailing complete versionstamp from a tuple-encoded key.
///
/// foundationdb-tuple 0.10 always packs a `Versionstamp` element as:
///   `0x33` (1 byte) || tx_version (10 bytes) || user_version (2 bytes)
/// so the last 13 bytes of any key whose final element is a versionstamp have
/// this layout.  Checking and slicing directly avoids the `unpack()` heap
/// allocation (a `Vec<Element>`) that the original code incurred per key.
fn extract_vs_from_key(key: &[u8]) -> Result<Versionstamp, Error> {
    if key.len() < 13 || key[key.len() - 13] != 0x33 {
        return Err(Error::TupleDecode(
            "key last element is not a versionstamp".into(),
        ));
    }
    let mut vs = [0u8; 12];
    vs.copy_from_slice(&key[key.len() - 12..]);
    Ok(vs)
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
