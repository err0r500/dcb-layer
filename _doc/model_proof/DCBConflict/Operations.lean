import DCBConflict.Model

namespace DCBConflict

/-- Write set of an event: one bucket per tag subset (including `[]`,
    the type-only slot). The implementation enumerates exactly the
    canonical (sorted, deduplicated) representatives of these subsets
    via `generate_superset_presorted`; here the set is characterized
    by its membership predicate. -/
def writeBuckets (e : Event) (b : Bucket) : Prop :=
  b.type = e.type ∧ b.tags ⊆ e.tags

/-- Read targets of a query item.
    - `types = []` (tags-only): one wildcard target — the discovery scan
      over the `(tags, "_")` prefix, covering every type.
    - otherwise: one target per candidate type, at the item's tag subset
      (`tags = []` lands on the type-only slot). -/
def readTargets (i : QueryItem) (afterVersion : Version) : List ReadTarget :=
  if i.types = [] then [⟨i.tags, none, afterVersion⟩]
  else i.types.map fun t => ⟨i.tags, some t, afterVersion⟩

/-- A read target covers a bucket: exact tag-subset match, and either a
    type wildcard or an exact type match. -/
def covers (rt : ReadTarget) (b : Bucket) : Prop :=
  rt.tags = b.tags ∧ (rt.type = none ∨ rt.type = some b.type)

/-- Conflict: some read target of the query covers some written bucket,
    at a version past the target's afterVersion. -/
def conflictDetected (e : Event) (q : Query) : Prop :=
  ∃ i ∈ q.items, ∃ rt ∈ readTargets i q.afterVersion,
    ∃ b, writeBuckets e b ∧ covers rt b ∧ e.version > rt.afterVersion

end DCBConflict
