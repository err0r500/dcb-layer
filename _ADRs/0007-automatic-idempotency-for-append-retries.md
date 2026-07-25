# Safe append retries via FDB AutomaticIdempotency transaction option

Status: accepted
Date: 2026-07-05

## Context and Problem Statement

The append retry loop must survive `commit_unknown_result`: a commit whose outcome is unknown (network blip after submit). Blindly retrying could apply the same batch twice; giving up leaves the caller unsure. How is exactly-once append guaranteed?

## Decision Drivers

- No double-applied batches under retries
- Keep the retry loop simple — no bespoke dedup bookkeeping
- Behavior must survive `on_error` transaction resets

## Considered Options

- `TransactionOption::AutomaticIdempotency` (client-managed idempotency id)
- Surface `commit_unknown_result` to the caller
- App-level dedup key (write a client-chosen id, check before retry)

## Decision Outcome

Chosen option: "AutomaticIdempotency", because the FDB client then attaches an idempotency id to the commit and resolves `commit_unknown_result` internally — the retry loop can never double-apply. Crucial detail: the option is re-set on **every** loop iteration (`append.rs:105`) because `on_error` resets transaction options; `RetryLimit`, set once, is one of the few that persists across resets (FDB API ≥ 610).

### Positive Consequences

- Exactly-once append semantics with a one-line option
- Retry loop stays a plain `loop { probe → write → commit | on_error }`

### Negative Consequences

- Depends on a relatively recent FDB client feature (part of why FDB ≥ 7.3 is required)
- Small commit-payload overhead for the idempotency id

## Pros and Cons of the Options

### AutomaticIdempotency

- Good, because correctness handled where the ambiguity arises (the client)
- Bad, because subtle re-set-per-iteration requirement — easy to regress

### Surface commit_unknown_result

- Good, because no magic
- Bad, because pushes an unanswerable question ("did it commit?") onto every caller

### App-level dedup key

- Good, because backend-agnostic
- Bad, because extra key per append + read-before-retry; reimplements what the client does natively

## Links

- Implemented by `dcb-layer/src/append.rs:96-106` (RetryLimit + AutomaticIdempotency, with rationale comment)
- Related: ADR-0010 (retry bounds)
