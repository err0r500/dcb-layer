import DCBConflict.Model

namespace DCBConflict

/-- Event matches a query item: the type is unconstrained or listed,
    and every required tag is present (set semantics). -/
def matchesItem (e : Event) (i : QueryItem) : Prop :=
  (i.types = [] ∨ e.type ∈ i.types) ∧ i.tags ⊆ e.tags

/-- Event matches query: version > afterVersion AND matches some item. -/
def matchesQuery (e : Event) (q : Query) : Prop :=
  e.version > q.afterVersion ∧ ∃ i ∈ q.items, matchesItem e i

end DCBConflict
