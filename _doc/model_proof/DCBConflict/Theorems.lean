import DCBConflict.Model
import DCBConflict.Matching
import DCBConflict.Operations
import DCBConflict.Lemmas

namespace DCBConflict

/-- Completeness: if an event matches a query, conflict is detected.
    Forward direction of the main theorem (no false negatives).

    Witness bucket: `⟨i.tags, e.type⟩` — written because `i.tags ⊆ e.tags`
    (the `[]` subset write makes this work even for type-only items). -/
theorem completeness (e : Event) (q : Query) (h : matchesQuery e q) : conflictDetected e q := by
  obtain ⟨hv, i, hi, htype, htags⟩ := h
  refine ⟨i, hi, ?_⟩
  by_cases hts : i.types = []
  · exact ⟨⟨i.tags, none, q.afterVersion⟩, wildcard_mem_readTargets q.afterVersion hts,
      ⟨i.tags, e.type⟩, ⟨rfl, htags⟩, ⟨rfl, Or.inl rfl⟩, hv⟩
  · have hmem : e.type ∈ i.types := htype.resolve_left hts
    exact ⟨⟨i.tags, some e.type, q.afterVersion⟩, typed_mem_readTargets q.afterVersion hmem,
      ⟨i.tags, e.type⟩, ⟨rfl, htags⟩, ⟨rfl, Or.inr rfl⟩, hv⟩

/-- Precision: if conflict is detected, the event matches the query.
    Backward direction of the main theorem (no false positives).

    Tags: `i.tags = rt.tags = b.tags ⊆ e.tags`. Type: the wildcard target
    only exists for tags-only items; a typed target forces an exact match. -/
theorem precision (e : Event) (q : Query) (h : conflictDetected e q) : matchesQuery e q := by
  obtain ⟨i, hi, rt, hrt, b, ⟨hbtype, hbtags⟩, ⟨htags, hcover⟩, hv⟩ := h
  obtain ⟨hrt_tags, hrt_av, hrt_type⟩ := readTargets_mem_spec hrt
  refine ⟨hrt_av ▸ hv, i, hi, ?_, ?_⟩
  · rcases hrt_type with ⟨hempty, _⟩ | ⟨t, ht_mem, ht_eq⟩
    · exact Or.inl hempty
    · rcases hcover with hnone | hsome
      · rw [ht_eq] at hnone; cases hnone
      · rw [ht_eq] at hsome
        have : t = b.type := Option.some.inj hsome
        exact Or.inr (hbtype ▸ this ▸ ht_mem)
  · rw [← hrt_tags, htags]
    exact hbtags

/-- Main theorem: matchesQuery and conflictDetected are equivalent.
    Proves the DCB conflict detection mechanism is both complete and precise. -/
theorem conflict_iff_matches (e : Event) (q : Query) : matchesQuery e q ↔ conflictDetected e q :=
  ⟨completeness e q, precision e q⟩

end DCBConflict
