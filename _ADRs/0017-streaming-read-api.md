# Streaming read API instead of Vec-returning reads

Status: proposed
Date: 2026-07-05

## Context and Problem Statement

`read`/`read_all` return `Vec<StoredEvent>` from a single transaction. Large result sets buffer fully in memory and can exceed FDB's 5 s limit, failing after bounded retries (ADR-0010); every consumer then hand-rolls the same `limit`+`after` pagination loop. Should the crate offer a streaming/paged API?

## Decision Drivers

- Projection rebuilds read the whole store — the worst case for the current API
- Pagination boilerplate is duplicated in every consumer (incl. the Elixir layer)
- Cross-transaction reads have no snapshot consistency — must be explicit, not hidden

## Considered Options

- Add `read_stream(query, opts) -> impl Stream<Item = Result<StoredEvent>>` that paginates across transactions internally
- Add a `read_page` helper returning `(Vec<StoredEvent>, Option<Versionstamp>)` and keep transactions explicit
- Keep Vec-only API

## Decision Outcome

Proposed: "read_stream", implemented as repeated bounded reads chained on the last position, yielding events as pages arrive. Because the store is append-only and positions are immutable (ADR-0004), a chained scan is a consistent prefix view — the one semantic caveat (events appended mid-stream may or may not appear) must be documented. Keep `read` for bounded queries. NIF note: a streaming NIF needs chunked delivery to the BEAM (e.g. cursor resource + `read_next` NIF), so `read_page` may be the pragmatic first step for the Elixir layer.

### Positive Consequences

- Whole-store scans work out of the box; constant memory
- One tested pagination implementation instead of N caller copies

### Negative Consequences

- API surface grows; two read paths to maintain
- Stream lifetime vs FDB transaction lifetime needs care (each page = fresh transaction)

## Links

- `dcb-layer/src/read.rs:87-129`, `dcb-layer/src/lib.rs:149-152`
- Related: ADR-0010, ADR-0004
