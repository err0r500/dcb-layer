# DCB conflict detection — Lean model & proof

```
matchesQuery e q ↔ conflictDetected e q
```

## Context

DCB (Dynamic Consistency Boundaries) uses FoundationDB to store events with:

- Type: event type (e.g., "OrderPlaced")
- Tags: set of tags (e.g., {"tenant:acme", "priority:high"})
- Version: monotonic versionstamp

## Index structure (unified layout)

One index subspace, one key shape:

```
/i/<sorted-tag-subset…>/_/<type>/<versionstamp>
```

An append writes one entry per subset of the event's (sorted, deduplicated)
tags — **including the empty subset**. The empty-subset slot
`/i/_/<type>/<vs>` is where type-only queries read: the former separate type
index is just the `∅` row of the tag index. The `_` separator is a reserved
tag, so a key parses unambiguously.

## Query semantics

A Query has `items` (OR'd) and `afterVersion`. Each QueryItem has `types`
(event must match one; empty = unconstrained) and `tags` (event must have
ALL). Event E matches Query Q iff `E.version > Q.afterVersion` and some item
I has (`I.types = ∅` or `E.type ∈ I.types`) and `I.tags ⊆ E.tags`.

## Range reads

| QueryItem               | Range read (within version > afterVersion)      |
| ----------------------- | ------------------------------------------------ |
| types={T1,T2}, tags=∅   | `/i/_/T1/…`, `/i/_/T2/…`                          |
| types=∅, tags={A,B}     | `/i/A/B/_/…` — covers **all** types (wildcard)    |
| types={T1}, tags={A,B}  | `/i/A/B/_/T1/…`                                   |

The tags-only case is the type-discovery scan over the `(tags, "_")` prefix;
as a read conflict range it covers every type under that tag subset.

## Model

- `Bucket = (tags, type)` — one uniform key shape.
- `ReadTarget = (tags, Option type, afterVersion)` — `none` models the
  tags-only wildcard prefix.
- `writeBuckets e = { b | b.type = e.type ∧ b.tags ⊆ e.tags }`.
- `readTargets i av` = the wildcard target if `i.types = []`, else one typed
  target per candidate type.
- `covers rt b = (rt.tags = b.tags) ∧ (rt.type = none ∨ rt.type = some b.type)`.
- `conflictDetected e q` = some read target of some item covers some written
  bucket with `e.version > afterVersion`.

### Abstractions (model vs implementation)

- **Powerset enumeration**: the implementation materializes the write set by
  enumerating all subsets of the sorted tags (`generate_superset_presorted`);
  the model characterizes the same set by its membership predicate
  `b.tags ⊆ e.tags`.
- **Canonicalization**: `sort_tags` (sort + dedup) is applied to both event
  tags and query tags, so byte equality of encoded tag subsets is set
  equality. The model uses list `⊆` (set semantics) and identifies a target
  with the bucket carrying the very same tag list, which is faithful because
  both sides pass through the same canonical form.
- **Key encoding injectivity** (tuple encoding, reserved `_` tag) is assumed,
  not modeled: buckets are structured pairs.

## Proof sketch

Read-target invariants: every `rt ∈ readTargets i av` has `rt.tags = i.tags`,
`rt.afterVersion = av`, and `rt.type` is `none` (iff `i.types = []`) or
`some t` with `t ∈ i.types`.

**Completeness** (match ⇒ conflict): witness bucket `⟨i.tags, e.type⟩`,
written because `i.tags ⊆ e.tags` (the `∅`-subset write makes this work even
for type-only items, where `i.tags = []`). If `i.types = []` the wildcard
target covers it; otherwise `e.type ∈ i.types` gives a typed target with an
exact type match.

**Precision** (conflict ⇒ match): version from the invariant;
tags from `i.tags = rt.tags = b.tags ⊆ e.tags`; type because a wildcard
target only exists for tags-only items, and a typed target `some t` forces
`t = b.type = e.type`, so `e.type ∈ i.types`.

No non-emptiness assumptions are needed: even a (rejected-in-practice)
empty/empty item is sound — it matches every event and every event writes
the `∅` bucket.

## Files

- `DCBConflict/Model.lean` — structures
- `DCBConflict/Matching.lean` — `matchesItem`, `matchesQuery` (the spec)
- `DCBConflict/Operations.lean` — `writeBuckets`, `readTargets`, `covers`, `conflictDetected` (the mechanism)
- `DCBConflict/Lemmas.lean` — read-target membership invariants
- `DCBConflict/Theorems.lean` — `completeness`, `precision`, `conflict_iff_matches`
- `_old_v1/` — previous model (two-bucket layout, non-empty powersets)

Build: `lake build` (Lean 4, Batteries only).
