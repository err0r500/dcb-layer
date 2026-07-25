# Bounded retry caps and caller-side pagination under FDB's 5 s / 10 MB transaction limits

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

FDB transactions abort after 5 seconds (`transaction_too_old`, 1007) and cap affected data at 10 MB. Naive retry loops re-run doomed oversized scans forever; unbounded contention loops never terminate. How does the store behave at these limits?

## Decision Drivers

- Fail loudly and boundedly instead of hanging
- Keep the core simple: no internal multi-transaction orchestration
- Give callers the tools to stay within limits

## Considered Options

- Bounded retries + caller-side pagination (limit/after)
- Internal transparent pagination across transactions
- Unbounded retries (FDB default behavior)

## Decision Outcome

Chosen option: "Bounded retries + caller pagination". Appends cap `on_error` retries at `APPEND_RETRY_LIMIT = 100` (pathological contention terminates with an error). Reads use `transact_boxed` with `READ_RETRY_LIMIT = 10` — an oversized scan fails fast instead of re-running a doomed 5 s scan ten-plus times. Callers paginate with `ReadOptions { limit, after }`; `lib.rs` documents the sizing math (2^n index keys per event vs the 10 MB budget).

### Positive Consequences

- Deterministic failure modes; no silent infinite loops
- Core stays single-transaction — simple atomicity story (one append = one commit)
- Batch validation up front: max 65 535 events per append (u16 batch index)

### Negative Consequences

- Large reads are the caller's problem (loop on `limit`+`after`); challenged in ADR-0017
- Paginated reads span transactions — no snapshot consistency across pages (positions make this safe for append-only data, but readers must know it)
- Retry caps are constants, not tunable per store

## Pros and Cons of the Options

### Bounded retries + caller pagination

- Good, because honest about FDB's limits; smallest core
- Bad, because pushes pagination boilerplate to every consumer

### Internal transparent pagination

- Good, because ergonomic
- Bad, because hides the consistency boundary between transactions

### Unbounded retries

- Good, because zero code
- Bad, because a query that can never finish in 5 s retries forever

## Links

- Implemented by `dcb-layer/src/append.rs:14-17`, `dcb-layer/src/read.rs:70-85`
- Documented in `dcb-layer/src/lib.rs:134-155`
- Challenged by ADR-0017
