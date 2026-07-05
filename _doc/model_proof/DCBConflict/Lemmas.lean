import DCBConflict.Model
import DCBConflict.Operations

namespace DCBConflict

/-- Shape of read-target membership: every target carries the item's tag
    subset and the query's afterVersion, and its type field is either the
    wildcard (tags-only item) or one of the item's candidate types. -/
theorem readTargets_mem_spec {i : QueryItem} {av : Version} {rt : ReadTarget}
    (h : rt ∈ readTargets i av) :
    rt.tags = i.tags ∧ rt.afterVersion = av ∧
      ((i.types = [] ∧ rt.type = none) ∨ ∃ t ∈ i.types, rt.type = some t) := by
  by_cases hts : i.types = []
  · simp only [readTargets, if_pos hts, List.mem_singleton] at h
    subst h
    exact ⟨rfl, rfl, Or.inl ⟨hts, rfl⟩⟩
  · simp only [readTargets, if_neg hts, List.mem_map] at h
    obtain ⟨t, ht, rfl⟩ := h
    exact ⟨rfl, rfl, Or.inr ⟨t, ht, rfl⟩⟩

/-- The wildcard target is a read target of a tags-only item. -/
theorem wildcard_mem_readTargets {i : QueryItem} (av : Version) (hts : i.types = []) :
    (⟨i.tags, none, av⟩ : ReadTarget) ∈ readTargets i av := by
  simp only [readTargets, if_pos hts, List.mem_singleton]

/-- The per-type target is a read target for each candidate type. -/
theorem typed_mem_readTargets {i : QueryItem} {t : EventType} (av : Version)
    (ht : t ∈ i.types) :
    (⟨i.tags, some t, av⟩ : ReadTarget) ∈ readTargets i av := by
  have hts : i.types ≠ [] := fun hnil => by rw [hnil] at ht; cases ht
  simp only [readTargets, if_neg hts, List.mem_map]
  exact ⟨t, ht, rfl⟩

end DCBConflict
