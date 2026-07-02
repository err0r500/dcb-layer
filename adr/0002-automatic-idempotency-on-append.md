# Replace the txid-marker recipe with FDB AutomaticIdempotency on append

Status: proposed
Date: 2026-07-02

## Context and Problem Statement

`append()` must not double-apply a batch when a commit fails with
`commit_unknown_result`. The initial hardening implemented FDB's documented
"unique side effect" recipe: a random txid marker written in the append
transaction, read back on a maybe-committed retry, and eagerly deleted in a
**second transaction** after every successful append. That cleanup commit
doubled the serial commit count per append and halved write throughput at
every concurrency level (~0.5x from w2 to w128 in the
`write-one-tag-one-type` bench).

## Decision Drivers

- Restore single-commit appends (remove the cleanup transaction from the hot path).
- Keep exactly-once append semantics under `commit_unknown_result`.
- Avoid leaking marker keys (one ~50-byte orphan per append at full rate adds up).

## Considered Options

- `TransactionOption::AutomaticIdempotency` (client-managed idempotency ids)
- Keep the txid recipe, make cleanup lazy (time-bucketed marker keys, opportunistic range-clear)
- Keep the txid recipe, drop cleanup entirely (accept orphaned markers)

## Decision Outcome

Chosen option: "`AutomaticIdempotency`", because it removes the marker write,
the recovery read, and the cleanup commit in one move — the FDB client attaches
a random 16-byte idempotency id and resolves `commit_unknown_result` itself, so
the append retry loop can never double-apply. The cluster runs FDB 7.4, and the
foundationdb crate (0.10) exposes the option.

**Accepted risk:** FDB marks the option "in development and not ready for
general use". The documented caveat concerns the multiversion client and
transaction timeouts, neither of which this crate uses. The full integration
suite (105 tests against a real 7.4 cluster) passes with the option set.
Fallback if it misbehaves: the lazy-cleanup variant of the txid recipe.

### Positive Consequences

- One commit per append again; bench throughput expected to return to ~0.1.0 levels.
- ~90 lines of idempotency machinery deleted (`pack_txid_key`, recovery branch,
  `cleanup_txid_marker`, `getrandom` dependency, `Error::RandomSource`).
- No marker keys to leak or clean.

### Negative Consequences

- Depends on an FDB feature flagged experimental upstream.
- Versionstamp behavior on the recovered-after-unknown-result path is
  undocumented; unverified beyond the test suite.

## Pros and Cons of the Options

### `TransactionOption::AutomaticIdempotency`

Set once per retry-loop iteration (options reset on `on_error`, unlike
`RetryLimit` which persists at API >= 610).

- Good, because the hot path pays zero extra keys, reads, or commits.
- Good, because idempotency state lives in the system keyspace, cleaned by FDB.
- Bad, because the feature is experimental upstream.

### Txid recipe with lazy cleanup

- Good, because it stays on fully-supported primitives.
- Bad, because it keeps an extra key write per append and needs time-bucketed
  keys plus a sweeper to reclaim markers safely — the most code of all options.

### Txid recipe without cleanup

- Good, because it is a two-line diff from the previous state.
- Bad, because it leaks one marker per append, forever.

## Links

- Supersedes the txid marker introduced in 001724d ("fix(perf & error handling)")
- Implementation — `dcb-layer/src/append.rs` (retry loop, option set per iteration)
