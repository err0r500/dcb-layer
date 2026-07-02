use std::borrow::Cow;

use foundationdb::options::StreamingMode;
use foundationdb::tuple::{Element, pack, unpack};
use foundationdb::{RangeOption, Transaction};
use futures::StreamExt;

use crate::encoding::{sort_tags, vs_to_fdb, EVENTS_IN_INDEXES_SUB, INDEXES_SUB};
use crate::error::Error;
use crate::types::{QueryItem, Versionstamp};

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
pub(crate) fn prefix_range(prefix: Vec<u8>) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let end = strinc(prefix.clone())?;
    Ok((prefix, end))
}

/// Range starting strictly after the event at `after` within `prefix`.
fn after_range(prefix: Vec<u8>, after: Versionstamp) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let vs_bytes = pack(&vs_to_fdb(after));
    let mut begin = prefix.clone();
    begin.extend_from_slice(&vs_bytes);
    begin.push(0x00);
    let end = strinc(prefix)?;
    Ok((begin, end))
}

// ---------------------------------------------------------------------------
// Subspace prefix builders
// ---------------------------------------------------------------------------

/// Shared elements for `[ns, INDEXES_SUB, tag1..tagN, EVENTS_IN_INDEXES_SUB]`.
fn tag_sentinel_elems<'a>(namespace: &'a str, sorted_tags: &'a [String]) -> Vec<Element<'a>> {
    let mut elems: Vec<Element<'a>> = Vec::with_capacity(3 + sorted_tags.len());
    elems.push(Element::String(Cow::Borrowed(namespace)));
    elems.push(Element::String(Cow::Borrowed(INDEXES_SUB)));
    for tag in sorted_tags {
        elems.push(Element::String(Cow::Borrowed(tag.as_str())));
    }
    elems.push(Element::String(Cow::Borrowed(EVENTS_IN_INDEXES_SUB)));
    elems
}

/// Prefix for a tag+type index subspace:
/// pack([ns, "g", tag1..tagN, "_", type_name])
fn tag_type_prefix(namespace: &str, sorted_tags: &[String], type_name: &str) -> Vec<u8> {
    let mut elems = tag_sentinel_elems(namespace, sorted_tags);
    elems.push(Element::String(Cow::Borrowed(type_name)));
    pack(&elems)
}

/// Prefix for the "_" sentinel subspace (for type discovery):
/// pack([ns, "g", tag1..tagN, "_"])
fn events_in_tag_prefix(namespace: &str, sorted_tags: &[String]) -> Vec<u8> {
    pack(&tag_sentinel_elems(namespace, sorted_tags))
}

// ---------------------------------------------------------------------------
// Type discovery
// ---------------------------------------------------------------------------

/// Enumerate the distinct event type strings present in the `_` subspace for
/// a given set of sorted tags.  Used by tags-only queries to build per-type
/// ranges.
///
/// Seek scan: read one key, extract its type, then jump past that type's
/// entire subrange (`strinc` of the type prefix) — O(distinct types) limit-1
/// reads instead of streaming every index entry.
///
/// Conflict-range note (append path, `snapshot = false`): each limit-1 read
/// that returns a key conflicts `[cursor, returned_key]`, and the final empty
/// read conflicts `[cursor, end)` — the union is the contiguous subspace, so
/// a concurrent event of a brand-new type still triggers a conflict.
async fn discover_types_in_tag_subspace(
    tr: &Transaction,
    namespace: &str,
    sorted_tags: &[String],
    snapshot: bool,
) -> Result<Vec<String>, Error> {
    let prefix = events_in_tag_prefix(namespace, sorted_tags);
    let (mut cursor, end) = prefix_range(prefix)?;

    // Full key: [ns, INDEXES_SUB, tag1..tagN, "_", type_name, vs]
    let type_elem_idx = sorted_tags.len() + 3;

    let mut types: Vec<String> = Vec::new();
    loop {
        let mut opt = RangeOption::from(cursor.clone()..end.clone());
        opt.limit = Some(1);
        opt.mode = StreamingMode::Small;
        let mut stream = tr.get_ranges_keyvalues(opt, snapshot);

        let kv = match stream.next().await {
            None => break,
            Some(Err(e)) => return Err(Error::Fdb(e)),
            Some(Ok(kv)) => kv,
        };
        let elements: Vec<Element<'_>> =
            unpack(kv.key()).map_err(|e| Error::TupleDecode(e.to_string()))?;
        // Only appends write this subspace, so every key has the fixed shape
        // above; anything else is corruption and must not be skipped silently
        // (skipping without advancing the cursor would also loop forever).
        let type_name = match elements.get(type_elem_idx) {
            Some(Element::String(s)) if elements.len() == type_elem_idx + 2 => s.to_string(),
            _ => {
                return Err(Error::TupleDecode(
                    "malformed key in tag index subspace".into(),
                ))
            }
        };
        cursor = strinc(tag_type_prefix(namespace, sorted_tags, &type_name))?;
        types.push(type_name);
    }

    Ok(types)
}

// ---------------------------------------------------------------------------
// Public interface
// ---------------------------------------------------------------------------

/// Translate one `QueryItem` into a list of (begin, end) FDB key ranges.
///
/// Each pair can be wrapped in `RangeOption::from(begin..end)` for reading.
///
/// - Type-only  → one range per type at the root of the unified index (`/i/_e/<type>/`)
/// - Type+tags  → one range per type over the unified index (`/i/…/_e/<type>/`)
/// - Tags-only  → discover types first (extra read), then same as type+tags
pub(crate) async fn build_query_ranges(
    tr: &Transaction,
    namespace: &str,
    item: &QueryItem,
    after: Option<Versionstamp>,
    snapshot: bool,
) -> Result<Vec<(Vec<u8>, Vec<u8>)>, Error> {
    if item.has_no_type_nor_tags() {
        return Err(Error::InvalidQuery);
    }

    let mut ranges: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();

    // ---- Case 1: type-only (no tags) — root of the unified index (/i/_e/<type>/)
    if item.has_types_only() {
        for type_name in &item.types {
            let prefix = tag_type_prefix(namespace, &[], type_name);
            ranges.push(match after {
                Some(vs) => after_range(prefix, vs)?,
                None => prefix_range(prefix)?,
            });
        }
        return Ok(ranges);
    }

    // ---- Case 2: tags present (with or without explicit types) ---------------
    let sorted_tags = sort_tags(&item.tags);

    let types: Vec<String> = if item.has_types_and_tags() {
        item.types.clone()
    } else {
        // Tags-only: discover types from the index
        discover_types_in_tag_subspace(tr, namespace, &sorted_tags, snapshot).await?
    };

    for type_name in &types {
        let prefix = tag_type_prefix(namespace, &sorted_tags, type_name);
        ranges.push(match after {
            Some(vs) => after_range(prefix, vs)?,
            None => prefix_range(prefix)?,
        });
    }

    Ok(ranges)
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
        let prefix = tag_type_prefix("ns", &[], "T");
        let (begin, end) = prefix_range(prefix.clone()).unwrap();
        assert_eq!(begin, prefix);
        assert!(end > begin);
    }

    #[test]
    fn test_after_range_begin_after_prefix() {
        let prefix = tag_type_prefix("ns", &[], "T");
        let (begin_plain, _) = prefix_range(prefix.clone()).unwrap();
        let vs = [0u8; 12];
        let (begin_after, end_after) = after_range(prefix.clone(), vs).unwrap();
        assert!(begin_after > begin_plain);
        assert_eq!(end_after, strinc(prefix).unwrap());
    }

    #[test]
    fn test_events_in_tag_prefix_is_prefix_of_tag_type_prefix() {
        let sorted_tags = vec!["tagA".into()];
        let evts = events_in_tag_prefix("ns", &sorted_tags);
        let ttp = tag_type_prefix("ns", &sorted_tags, "T");
        assert!(ttp.starts_with(&evts));
    }
}
