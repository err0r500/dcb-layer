use std::borrow::Cow;

use bytes::Bytes;
use foundationdb::tuple::{
    Bytes as FdbBytes, Element, Versionstamp as FdbVs, pack, pack_with_versionstamp, unpack,
};

use crate::error::Error;
use crate::types::{Event, Versionstamp};

const EVENTS_SUB: &str = "e";
pub(crate) const INDEXES_SUB: &str = "i";
pub(crate) const EVENTS_IN_INDEXES_SUB: &str = "_";
const SENTINEL_SUB: &str = "lastvs";
/// Number of sentinel shard keys per namespace. The sentinel exists only to
/// wake watchers after an append; sharding a single fixed key into many lets
/// FDB's data distributor spread the append-notification write load across
/// storage teams instead of pinning the whole namespace to one team. Must stay
/// a power of two (cheap `%`) and small enough that a watcher can arm all shards
/// in one transaction (well under FDB's per-client watch limit).
pub(crate) const SENTINEL_SHARDS: u32 = 32;
const SUBS_SUB: &str = "subs";
const TXID_SUB: &str = "t";

// ---------------------------------------------------------------------------
// Tag helpers
// ---------------------------------------------------------------------------

pub(crate) fn sort_tags<S>(tags: &[S]) -> Vec<S>
where
    S: AsRef<str> + Clone + Ord,
{
    let mut sorted = tags.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted
}

/// All subsets (including empty) of pre-sorted, deduplicated `sorted_tags`.
pub(crate) fn generate_superset_presorted<S>(sorted_tags: &[S]) -> Vec<Vec<S>>
where
    S: AsRef<str> + Clone,
{
    let n = sorted_tags.len();
    let total = (1usize << n).saturating_sub(1);
    let mut result = Vec::with_capacity(total + 1);
    for mask in 0..=total {
        let subset: Vec<S> = (0..n)
            .filter(|&i| mask & (1 << i) != 0)
            .map(|i| sorted_tags[i].clone())
            .collect();
        result.push(subset);
    }
    result
}

// ---------------------------------------------------------------------------
// Versionstamp helpers
// ---------------------------------------------------------------------------

pub(crate) fn versionstamp_to_hex(vs: Versionstamp) -> String {
    hex::encode(vs)
}

/// Convert our 12-byte Versionstamp to the foundationdb tuple Versionstamp (complete).
pub(crate) fn vs_to_fdb(vs: Versionstamp) -> FdbVs {
    let mut tx = [0u8; 10];
    tx.copy_from_slice(&vs[..10]);
    let user = u16::from_be_bytes([vs[10], vs[11]]);
    FdbVs::complete(tx, user)
}

// ---------------------------------------------------------------------------
// Subscription key encoding
// ---------------------------------------------------------------------------

/// Sentinel shard key touched by every append: pack([namespace, "lastvs", shard]).
/// Each append writes exactly one shard (round-robin); a watcher arms all shards.
pub(crate) fn pack_sentinel_shard_key(namespace: &str, shard: u32) -> Vec<u8> {
    pack(&(namespace, SENTINEL_SUB, shard))
}

/// Durable cursor key for a named subscription: pack([namespace, "subs", name])
pub(crate) fn pack_cursor_key(namespace: &str, name: &str) -> Vec<u8> {
    pack(&(namespace, SUBS_SUB, name))
}

/// Idempotency key for one `append` call: pack([namespace, "t", txid]).
/// Its versionstamped value lets a retry after `commit_unknown_result`
/// detect that the previous commit actually landed.
pub(crate) fn pack_txid_key(namespace: &str, txid: &[u8; 16]) -> Vec<u8> {
    pack(&(
        namespace,
        TXID_SUB,
        FdbBytes(Cow::Borrowed(txid.as_slice())),
    ))
}

// ---------------------------------------------------------------------------
// Key encoding — complete versionstamp (for reads / tests)
// ---------------------------------------------------------------------------

/// Prefix covering the entire primary events subspace: pack([namespace, "e"])
pub(crate) fn pack_events_prefix(namespace: &str) -> Vec<u8> {
    pack(&(namespace, EVENTS_SUB))
}

/// Primary event key: pack([namespace, "e", versionstamp])
pub(crate) fn pack_event_key(namespace: &str, vs: Versionstamp) -> Vec<u8> {
    pack(&(namespace, EVENTS_SUB, vs_to_fdb(vs)))
}

// ---------------------------------------------------------------------------
// Key encoding — write-side helpers (accept FdbVs directly, may be incomplete)
// ---------------------------------------------------------------------------

/// For write path: packs key with an (possibly incomplete) FdbVs and appends
/// the 4-byte LE offset when the stamp is incomplete.
pub(crate) fn pack_event_key_fdb(namespace: &str, fdb_vs: FdbVs) -> Vec<u8> {
    pack_with_versionstamp(&(namespace, EVENTS_SUB, fdb_vs))
}

pub(crate) fn pack_tag_index_key_fdb<S: AsRef<str>>(
    namespace: &str,
    sorted_tags: &[S],
    type_name: &str,
    fdb_vs: FdbVs,
) -> Vec<u8> {
    let mut elems: Vec<Element<'_>> = Vec::with_capacity(4 + sorted_tags.len());
    elems.push(Element::String(Cow::Borrowed(namespace)));
    elems.push(Element::String(Cow::Borrowed(INDEXES_SUB)));
    for tag in sorted_tags {
        elems.push(Element::String(Cow::Borrowed(tag.as_ref())));
    }
    elems.push(Element::String(Cow::Borrowed(EVENTS_IN_INDEXES_SUB)));
    elems.push(Element::String(Cow::Borrowed(type_name)));
    elems.push(Element::Versionstamp(fdb_vs));
    pack_with_versionstamp(&elems)
}

// ---------------------------------------------------------------------------
// Value encoding / decoding
// ---------------------------------------------------------------------------

/// Encode event as tuple (type: string, tags: nested-tuple, data: bytes)
pub(crate) fn encode_event_value(event: &Event) -> Vec<u8> {
    let tags_elems: Vec<Element<'_>> = event
        .tags
        .iter()
        .map(|t| Element::String(Cow::Borrowed(t.as_ref())))
        .collect();

    let elems: Vec<Element<'_>> = vec![
        Element::String(Cow::Borrowed(event.type_name.as_ref())),
        Element::Tuple(tags_elems),
        Element::Bytes(FdbBytes(Cow::Borrowed(event.data.as_ref()))),
    ];
    pack(&elems)
}

pub(crate) fn decode_event_value(bytes: &[u8]) -> Result<Event, Error> {
    let elems: Vec<Element<'_>> =
        unpack(bytes).map_err(|e| Error::TupleDecode(e.to_string()))?;

    if elems.len() != 3 {
        return Err(Error::TupleDecode(format!(
            "expected 3-element tuple, got {}",
            elems.len()
        )));
    }

    let type_name: String = match &elems[0] {
        Element::String(s) => s.to_string(),
        other => {
            return Err(Error::TupleDecode(format!(
                "type field: expected string, got {:?}",
                other
            )))
        }
    };

    let tags: Vec<String> = match &elems[1] {
        Element::Tuple(t) => t
            .iter()
            .map(|e| match e {
                Element::String(s) => Ok(s.to_string()),
                other => Err(Error::TupleDecode(format!(
                    "tag element: expected string, got {:?}",
                    other
                ))),
            })
            .collect::<Result<Vec<String>, Error>>()?,
        other => {
            return Err(Error::TupleDecode(format!(
                "tags field: expected tuple, got {:?}",
                other
            )))
        }
    };

    let data: Bytes = match &elems[2] {
        Element::Bytes(b) => Bytes::copy_from_slice(b.as_ref()),
        other => {
            return Err(Error::TupleDecode(format!(
                "data field: expected bytes, got {:?}",
                other
            )))
        }
    };

    Ok(Event { type_name, tags, data })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    #[test]
    fn test_sort_tags_sorts_and_deduplicates() {
        let tags: Vec<String> = vec!["c".into(), "a".into(), "b".into(), "a".into()];
        assert_eq!(sort_tags(&tags), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_generate_superset_presorted_stable_on_sorted_input() {
        let sorted: Vec<String> = vec!["a".into(), "b".into()];
        let subs = generate_superset_presorted(&sorted);
        // 4 subsets in mask order: {}, {a}, {b}, {a,b}
        assert_eq!(subs.len(), 4);
        assert_eq!(subs[0], Vec::<String>::new()); // mask=0b00
        assert_eq!(subs[1], vec!["a".to_string()]); // mask=0b01
        assert_eq!(subs[2], vec!["b".to_string()]); // mask=0b10
        assert_eq!(subs[3], vec!["a".to_string(), "b".to_string()]); // mask=0b11
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let event = Event {
            type_name: "OrderPlaced".into(),
            tags: vec!["tenant:42".into(), "user:7".into()],
            data: Bytes::from_static(&[1u8, 2, 3, 4]),
        };
        let encoded = encode_event_value(&event);
        let decoded = decode_event_value(&encoded).expect("decode failed");
        assert_eq!(decoded.type_name, event.type_name);
        assert_eq!(decoded.tags, event.tags);
        assert_eq!(decoded.data, event.data);
    }

    #[test]
    fn test_encode_decode_no_tags() {
        let event = Event {
            type_name: "Ping".into(),
            tags: vec![],
            data: Bytes::new(),
        };
        let encoded = encode_event_value(&event);
        let decoded = decode_event_value(&encoded).expect("decode failed");
        assert_eq!(decoded.type_name, "Ping");
        assert!(decoded.tags.is_empty());
        assert!(decoded.data.is_empty());
    }

    #[test]
    fn test_versionstamp_compare() {
        let mut a = [0u8; 12];
        let mut b = [0u8; 12];
        a[9] = 1;
        b[9] = 2;
        assert_eq!(a.cmp(&b), Ordering::Less);
        assert_eq!(b.cmp(&a), Ordering::Greater);
        assert_eq!(a.cmp(&a), Ordering::Equal);
    }

    #[test]
    fn test_versionstamp_to_hex() {
        let mut vs = [0u8; 12];
        for (i, b) in vs.iter_mut().enumerate() {
            *b = (i + 1) as u8;
        }
        assert_eq!(versionstamp_to_hex(vs), "0102030405060708090a0b0c");
    }

    #[test]
    fn test_pack_event_key_deterministic() {
        let vs = [0u8; 12];
        let k1 = pack_event_key("ns", vs);
        let k2 = pack_event_key("ns", vs);
        assert_eq!(k1, k2);
        assert!(!k1.is_empty());
    }

    #[test]
    fn test_pack_sentinel_key_deterministic() {
        assert_eq!(pack_sentinel_shard_key("ns", 0), pack_sentinel_shard_key("ns", 0));
        assert!(!pack_sentinel_shard_key("ns", 0).is_empty());
    }

    #[test]
    fn test_pack_sentinel_key_varies_by_namespace() {
        assert_ne!(pack_sentinel_shard_key("ns_a", 0), pack_sentinel_shard_key("ns_b", 0));
    }

    #[test]
    fn test_pack_sentinel_shard_key_varies_by_shard() {
        assert_ne!(pack_sentinel_shard_key("ns", 0), pack_sentinel_shard_key("ns", 1));
    }

    #[test]
    fn test_pack_cursor_key_deterministic() {
        assert_eq!(pack_cursor_key("ns", "sub1"), pack_cursor_key("ns", "sub1"));
        assert!(!pack_cursor_key("ns", "sub1").is_empty());
    }

    #[test]
    fn test_pack_cursor_key_varies_by_name_and_namespace() {
        assert_ne!(pack_cursor_key("ns", "sub1"), pack_cursor_key("ns", "sub2"));
        assert_ne!(pack_cursor_key("ns_a", "sub1"), pack_cursor_key("ns_b", "sub1"));
    }

    #[test]
    fn test_sentinel_and_cursor_keys_do_not_collide() {
        // Sentinel key must not alias any cursor key (different subspace strings).
        assert_ne!(pack_sentinel_shard_key("ns", 0), pack_cursor_key("ns", "lastvs"));
        assert_ne!(pack_sentinel_shard_key("ns", 0), pack_cursor_key("ns", "any"));
    }

    #[test]
    fn test_sentinel_key_does_not_collide_with_event_key() {
        let vs = [0u8; 12];
        assert_ne!(pack_sentinel_shard_key("ns", 0), pack_event_key("ns", vs));
    }

    #[test]
    fn test_txid_key_deterministic_and_distinct_per_txid_and_namespace() {
        let a = [1u8; 16];
        let b = [2u8; 16];
        assert_eq!(pack_txid_key("ns", &a), pack_txid_key("ns", &a));
        assert_ne!(pack_txid_key("ns", &a), pack_txid_key("ns", &b));
        assert_ne!(pack_txid_key("ns_a", &a), pack_txid_key("ns_b", &a));
    }

    #[test]
    fn test_txid_key_does_not_collide_with_other_subspaces() {
        let txid = [0u8; 16];
        let key = pack_txid_key("ns", &txid);
        assert_ne!(key, pack_sentinel_shard_key("ns", 0));
        assert_ne!(key, pack_cursor_key("ns", "t"));
        assert_ne!(key, pack_event_key("ns", [0u8; 12]));
    }
}
