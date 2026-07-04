use foundationdb::options::StreamingMode;
use foundationdb::tuple::pack;
use foundationdb::{RangeOption, Transaction};
use futures::{Stream, StreamExt};

use crate::encoding::{sort_tags, vs_to_fdb, TAG_INDEX_SUB, TYPE_INDEX_SUB};
use crate::error::Error;
use crate::types::{QueryItem, Versionstamp};

/// A single index range: `(begin, end)` suitable for `RangeOption::from`.
pub(crate) type Range = (Vec<u8>, Vec<u8>);

// ---------------------------------------------------------------------------
// Low-level key-range helpers
// ---------------------------------------------------------------------------

/// FDB "strinc" — increment the last non-0xFF byte, strip trailing 0xFF bytes.
/// Gives the exclusive upper bound for a prefix range scan.
pub(crate) fn strinc(mut key: Vec<u8>) -> Result<Vec<u8>, Error> {
    for i in (0..key.len()).rev() {
        if key[i] != 0xFF {
            key[i] += 1;
            key.truncate(i + 1);
            return Ok(key);
        }
    }
    Err(Error::AllFfKey)
}

/// Range covering all keys with the given prefix: [prefix, strinc(prefix)).
pub(crate) fn prefix_range(prefix: Vec<u8>) -> Result<Range, Error> {
    let end = strinc(prefix.clone())?;
    Ok((prefix, end))
}

/// Range starting strictly after the event at `after` within `prefix`.
fn after_range(prefix: Vec<u8>, after: Versionstamp) -> Result<Range, Error> {
    let vs_bytes = pack(&vs_to_fdb(after));
    let mut begin = prefix.clone();
    begin.extend_from_slice(&vs_bytes);
    begin.push(0x00);
    let end = strinc(prefix)?;
    Ok((begin, end))
}

fn range_for(prefix: Vec<u8>, after: Option<Versionstamp>) -> Result<Range, Error> {
    match after {
        Some(vs) => after_range(prefix, vs),
        None => prefix_range(prefix),
    }
}

// ---------------------------------------------------------------------------
// Subspace prefix builders
// ---------------------------------------------------------------------------

fn tag_prefix(namespace: &str, tag: &str) -> Vec<u8> {
    pack(&(namespace, TAG_INDEX_SUB, tag))
}

fn type_prefix(namespace: &str, type_name: &str) -> Vec<u8> {
    pack(&(namespace, TYPE_INDEX_SUB, type_name))
}

// ---------------------------------------------------------------------------
// Query branch builder
// ---------------------------------------------------------------------------

/// A "branch" is a set of index ranges that must all be **intersected** (AND).
/// `build_query_branches` returns one branch per `QueryItem` alternative that
/// must then be **unioned** (OR) with the other branches.
///
/// - Type-only: one single-range branch per type, over the type index.
/// - Tags-only: one branch containing every tag's range (any type matches).
/// - Type + tags: one branch per type, each containing every tag's range
///   plus that type's range.
///
/// Unlike a power-set index this needs no extra read to build: every branch
/// is derived directly from the query, so this function is synchronous.
pub(crate) fn build_query_branches(
    namespace: &str,
    item: &QueryItem,
    after: Option<Versionstamp>,
) -> Result<Vec<Vec<Range>>, Error> {
    if item.has_no_type_nor_tags() {
        return Err(Error::InvalidQuery);
    }

    if item.has_types_only() {
        return item
            .types
            .iter()
            .map(|t| range_for(type_prefix(namespace, t), after).map(|r| vec![r]))
            .collect();
    }

    let sorted_tags = sort_tags(&item.tags);
    let tag_ranges: Vec<Range> = sorted_tags
        .iter()
        .map(|t| range_for(tag_prefix(namespace, t), after))
        .collect::<Result<_, _>>()?;

    // Tags-only: any type matches, so a single branch intersecting the tag
    // ranges is the complete answer — no per-type fan-out needed.
    if !item.has_types_and_tags() {
        return Ok(vec![tag_ranges]);
    }

    // Type + tags: one branch per type, each requiring every tag plus that type.
    item.types
        .iter()
        .map(|t| {
            let mut branch = tag_ranges.clone();
            branch.push(range_for(type_prefix(namespace, t), after)?);
            Ok(branch)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Versionstamp stream primitives
// ---------------------------------------------------------------------------

pub(crate) type VsStream<'a> = Box<dyn Stream<Item = Result<Versionstamp, Error>> + Unpin + Send + 'a>;

/// Extract the trailing complete versionstamp from a tuple-encoded key.
///
/// foundationdb-tuple 0.10 always packs a `Versionstamp` element as:
///   `0x33` (1 byte) || tx_version (10 bytes) || user_version (2 bytes)
/// so the last 13 bytes of any key whose final element is a versionstamp have
/// this layout. Checking and slicing directly avoids the `unpack()` heap
/// allocation (a `Vec<Element>`) that decoding the whole tuple would incur.
pub(crate) fn extract_vs_from_key(key: &[u8]) -> Result<Versionstamp, Error> {
    if key.len() < 13 || key[key.len() - 13] != 0x33 {
        return Err(Error::TupleDecode(
            "key last element is not a versionstamp".into(),
        ));
    }
    let mut vs = [0u8; 12];
    vs.copy_from_slice(&key[key.len() - 12..]);
    Ok(vs)
}

/// Open a single index range as a stream of decoded versionstamps.
/// `limit`, when set, is passed to FDB as a hint so a capped scan (e.g. an
/// existence probe) doesn't fetch more than it needs.
pub(crate) fn open_vs_stream<'a>(
    tr: &'a Transaction,
    range: Range,
    reverse: bool,
    snapshot: bool,
    limit: Option<usize>,
) -> VsStream<'a> {
    let mut opt = RangeOption::from(range.0..range.1);
    opt.reverse = reverse;
    if limit.is_some() {
        opt.limit = limit;
        opt.mode = StreamingMode::WantAll;
    }
    let stream = tr
        .get_ranges_keyvalues(opt, snapshot)
        .map(|r| r.map_err(Error::Fdb).and_then(|kv| extract_vs_from_key(kv.key())));
    Box::new(stream)
}

async fn advance(stream: &mut VsStream<'_>) -> Result<Option<Versionstamp>, Error> {
    stream.next().await.transpose()
}

// ---------------------------------------------------------------------------
// K-way intersection (AND of tag/type index streams)
// ---------------------------------------------------------------------------

/// Intersect every range in `branch`, returning the versionstamps present in
/// **all** of them, in the requested order. A single-range branch (the common
/// case: one tag, or type-only) is just a direct scan. Multi-range branches
/// run a sort-merge join: repeatedly align every stream's current head on the
/// same versionstamp (advancing whichever heads lag behind), emitting only
/// when all heads agree, and stopping the moment any stream is exhausted —
/// past that point no further match is possible.
pub(crate) async fn intersect_branch(
    tr: &Transaction,
    branch: Vec<Range>,
    reverse: bool,
    snapshot: bool,
    max_matches: Option<usize>,
) -> Result<Vec<Versionstamp>, Error> {
    if branch.len() == 1 {
        let range = branch.into_iter().next().expect("len checked above");
        let mut stream = open_vs_stream(tr, range, reverse, snapshot, max_matches);
        let mut out = Vec::new();
        while let Some(vs) = advance(&mut stream).await? {
            out.push(vs);
            if max_matches.is_some_and(|n| out.len() >= n) {
                break;
            }
        }
        return Ok(out);
    }

    let k = branch.len();
    let mut streams: Vec<VsStream<'_>> = branch
        .into_iter()
        .map(|range| open_vs_stream(tr, range, reverse, snapshot, None))
        .collect();

    let mut fronts: Vec<Option<Versionstamp>> = Vec::with_capacity(k);
    for stream in streams.iter_mut() {
        fronts.push(advance(stream).await?);
    }

    let mut out = Vec::new();
    while fronts.iter().all(Option::is_some) {
        let target = if reverse {
            fronts.iter().map(|f| f.expect("checked above")).min()
        } else {
            fronts.iter().map(|f| f.expect("checked above")).max()
        }
        .expect("branch is non-empty");

        if fronts.iter().all(|f| f == &Some(target)) {
            out.push(target);
            if max_matches.is_some_and(|n| out.len() >= n) {
                break;
            }
            for (front, stream) in fronts.iter_mut().zip(streams.iter_mut()) {
                *front = advance(stream).await?;
            }
        } else {
            for (front, stream) in fronts.iter_mut().zip(streams.iter_mut()) {
                let lagging = match front {
                    Some(vs) => {
                        if reverse {
                            *vs > target
                        } else {
                            *vs < target
                        }
                    }
                    None => false,
                };
                if lagging {
                    *front = advance(stream).await?;
                }
            }
        }
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strinc_basic() {
        assert_eq!(strinc(b"a".to_vec()).unwrap(), b"b".to_vec());
        assert_eq!(strinc(b"fdb".to_vec()).unwrap(), b"fdc".to_vec());
    }

    #[test]
    fn test_strinc_strips_ff() {
        assert_eq!(strinc(vec![0x61, 0xFF]).unwrap(), vec![0x62]);
        assert_eq!(strinc(vec![0x61, 0xFF, 0xFF]).unwrap(), vec![0x62]);
    }

    #[test]
    fn test_strinc_all_ff_returns_error() {
        assert!(strinc(vec![0xFF, 0xFF]).is_err());
    }

    #[test]
    fn test_prefix_range_begin_eq_prefix() {
        let prefix = type_prefix("ns", "T");
        let (begin, end) = prefix_range(prefix.clone()).unwrap();
        assert_eq!(begin, prefix);
        assert!(end > begin);
    }

    #[test]
    fn test_after_range_begin_after_prefix() {
        let prefix = type_prefix("ns", "T");
        let (begin_plain, _) = prefix_range(prefix.clone()).unwrap();
        let vs = [0u8; 12];
        let (begin_after, end_after) = after_range(prefix.clone(), vs).unwrap();
        assert!(begin_after > begin_plain);
        assert_eq!(end_after, strinc(prefix).unwrap());
    }

    #[test]
    fn test_tag_prefix_is_prefix_of_after_range() {
        let prefix = tag_prefix("ns", "tagA");
        let (begin, _) = prefix_range(prefix.clone()).unwrap();
        assert_eq!(begin, prefix);
    }

    #[test]
    fn test_build_query_branches_type_only() {
        let item = QueryItem { types: vec!["T1".into(), "T2".into()], tags: vec![] };
        let branches = build_query_branches("ns", &item, None).unwrap();
        assert_eq!(branches.len(), 2);
        assert!(branches.iter().all(|b| b.len() == 1));
    }

    #[test]
    fn test_build_query_branches_tags_only_single_branch() {
        let item = QueryItem { types: vec![], tags: vec!["a".into(), "b".into()] };
        let branches = build_query_branches("ns", &item, None).unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].len(), 2);
    }

    #[test]
    fn test_build_query_branches_type_and_tags() {
        let item = QueryItem {
            types: vec!["T1".into(), "T2".into()],
            tags: vec!["a".into(), "b".into()],
        };
        let branches = build_query_branches("ns", &item, None).unwrap();
        // one branch per type, each with 2 tag ranges + 1 type range
        assert_eq!(branches.len(), 2);
        assert!(branches.iter().all(|b| b.len() == 3));
    }

    #[test]
    fn test_build_query_branches_rejects_empty_item() {
        let item = QueryItem { types: vec![], tags: vec![] };
        assert!(matches!(
            build_query_branches("ns", &item, None),
            Err(Error::InvalidQuery)
        ));
    }
}
