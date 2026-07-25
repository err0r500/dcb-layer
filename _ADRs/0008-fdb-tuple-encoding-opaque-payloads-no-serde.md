# FDB tuple encoding for all keys/values; event payloads stay opaque bytes; no serde

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

Keys must sort correctly for range scans and versionstamp ordering; values must round-trip event metadata. What encoding is used, and does the store interpret event payloads?

## Decision Drivers

- Order-preserving key encoding is non-negotiable (range scans, ADR-0005)
- Versionstamp placeholders must integrate with key packing (`pack_with_versionstamp`)
- Store should not impose a payload format on applications
- Minimal dependency surface

## Considered Options

- FDB tuple encoding everywhere; `data` as opaque `Bytes`
- serde + bincode/JSON values, tuple keys
- Custom binary layout

## Decision Outcome

Chosen option: "FDB tuple encoding everywhere". Keys: `pack([ns, subspace, ...])` with incomplete versionstamps filled at commit. Values: tuple `(type, tags-nested-tuple, data)` (`encode_event_value`). Event `data` is opaque `bytes::Bytes` — applications encode however they like. No serde anywhere in the crate.

### Positive Consequences

- One encoding for keys and values; order-preservation guaranteed by the FDB tuple spec
- Zero-copy payloads via `Bytes`
- Hot-path optimization possible: `extract_vs_from_key` (`read.rs:290`) hand-parses the trailing 13 bytes (0x33 tag + 12-byte versionstamp) instead of a full `unpack()` per key

### Negative Consequences

- No key-layout version marker: subspace layout (`e`, `i`, `_`, `lastvs`, `subs`) is implicit — a future layout change means an ad-hoc migration story
- Tuple decode errors surface as stringly-typed `Error::TupleDecode`
- Hand-parsed key bytes duplicate knowledge of the tuple spec (guarded by unit tests in `encoding.rs`)

## Pros and Cons of the Options

### Tuple encoding + opaque payloads

- Good, because canonical, ordered, and already required for keys
- Bad, because implicit schema versioning

### serde values

- Good, because typed evolution (versioned structs)
- Bad, because extra dependency and format choice imposed on a layer that owns only metadata

### Custom binary layout

- Good, because maximal control
- Bad, because reimplements ordering rules the tuple layer already guarantees

## Links

- Implemented by `dcb-layer/src/encoding.rs`, `dcb-layer/src/read.rs:290`
- Related: ADR-0005 (key layout), ADR-0004 (versionstamps)
