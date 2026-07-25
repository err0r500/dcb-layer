# Machine-checked Lean 4 proof that conflict detection equals query matching

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

The DCB guarantee rests on a subtle claim: the index ranges an append condition reads (ADR-0005 + ADR-0006) conflict with a concurrent write **iff** the written event matches the condition's query. A false negative silently breaks consistency; a false positive silently serializes unrelated writers. Tests sample this space — can the claim be established exhaustively?

## Decision Drivers

- The property is the crux of the whole design and easy to break with an index-layout change
- Integration tests (`condition_semantics_tests.rs`) cover a matrix, not the full space
- A machine-checked model documents the mechanism precisely

## Considered Options

- Lean 4 formal proof of the abstract model
- Exhaustive property-based tests on the Rust implementation
- Rely on the integration test matrix

## Decision Outcome

Chosen option: "Lean 4 proof" (`_doc/model_proof/`, lake + Batteries only). The model (`DCBConflict/Model.lean`) captures the unified index layout `/i/<sorted-tag-subset>/_/<type>/<versionstamp>` where an append writes one entry per tag subset (including empty). `Matching.lean` defines the spec (`matchesQuery`); `Operations.lean` defines the mechanism (`writeBuckets`, `readTargets`, `conflictDetected`); `Theorems.lean` proves `completeness` (no false negatives), `precision` (no false positives), and the main `conflict_iff_matches`.

### Positive Consequences

- The index/conflict design is provably correct at the model level, and the README documents the model-vs-implementation abstraction gap explicitly
- Layout changes (e.g. ADR-0016 alternatives) can be re-verified against the same spec before touching Rust
- Replaced an earlier two-bucket model (`_old_v1/`) — the proof process itself simplified the design

### Negative Consequences

- Proves the model, not the Rust code — the encoding/query translation must faithfully implement the model (a gap ADR-0015 property tests could narrow)
- Lean toolchain maintenance for a repo otherwise Rust/Elixir

## Pros and Cons of the Options

### Lean proof

- Good, because exhaustive over the model's space; survives as precise documentation
- Bad, because model–implementation gap remains

### Property-based tests on Rust

- Good, because tests the real code
- Bad, because sampled, not exhaustive; complements rather than replaces (see ADR-0015)

### Integration matrix only

- Good, because zero extra tooling
- Bad, because the dangerous cases are the ones nobody thought to enumerate

## Links

- Implemented by `_doc/model_proof/DCBConflict/{Model,Matching,Operations,Lemmas,Theorems}.lean`, `_doc/model_proof/README.md`
- Models `dcb-layer/src/encoding.rs`, `dcb-layer/src/query.rs`, `dcb-layer/src/append.rs`
- Related: ADR-0005, ADR-0006, ADR-0015
