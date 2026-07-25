# Architecture Decision Records

Retrospective ADRs (0001–0014, `accepted`) document decisions already embodied in the code; 0015–0019 (`proposed`) challenge current choices or capture known remaining work.

## Accepted

- [0001](0001-foundationdb-as-storage-backend.md) — FoundationDB as the storage backend
- [0002](0002-rust-core-with-elixir-nif-bindings.md) — Rust core crate + Elixir bindings via precompiled Rustler NIF
- [0003](0003-per-append-conditions-over-aggregate-streams.md) — DCB per-append conditions instead of aggregate streams
- [0004](0004-fdb-versionstamps-as-global-event-position.md) — FDB versionstamps as global event position (10 B tx version + 2 B batch index)
- [0005](0005-tag-powerset-indexing.md) — Tag powerset indexing (one index key per tag subset, 10-tag cap)
- [0006](0006-condition-checks-as-read-conflict-probes.md) — Conditions enforced as read-conflict probes in the write transaction
- [0007](0007-automatic-idempotency-for-append-retries.md) — Safe append retries via FDB AutomaticIdempotency
- [0008](0008-fdb-tuple-encoding-opaque-payloads-no-serde.md) — FDB tuple encoding everywhere; opaque payloads; no serde
- [0009](0009-sentinel-watch-plus-durable-named-cursors.md) — Subscriptions via sentinel-key watch + durable named cursors
- [0010](0010-bounded-retries-and-caller-pagination-under-fdb-limits.md) — Bounded retries and caller-side pagination under FDB's 5 s / 10 MB limits
- [0011](0011-namespace-prefix-for-multi-tenancy.md) — Namespace prefix on every key for multi-tenancy
- [0012](0012-integration-tests-against-real-fdb-via-testcontainers.md) — Integration tests against real FDB via a shared testcontainer
- [0013](0013-lean4-proof-of-conflict-detection-correctness.md) — Lean 4 proof that conflict detection ⟺ query matching
- [0014](0014-lockstep-release-with-natively-built-precompiled-nifs.md) — Lockstep releases with natively built precompiled NIFs (no cross)

## Proposed

- [0015](0015-adopt-or-drop-property-based-testing.md) — Adopt property-based testing (or drop unused proptest)
- [0016](0016-revisit-powerset-indexing-cost.md) — Revisit powerset indexing cost (write amplification, 10-tag cap, type discovery)
- [0017](0017-streaming-read-api.md) — Streaming read API instead of Vec-returning reads
- [0018](0018-storage-backend-abstraction.md) — Storage backend abstraction / in-memory implementation
- [0019](0019-cursor-concurrency-semantics.md) — Cursor concurrency: LWW vs CAS/leases
- [0020](0020-backup-restore-and-position-portability.md) — Backup/restore across clusters and position portability
