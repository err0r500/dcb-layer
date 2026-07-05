namespace DCBConflict

abbrev Tag := String
abbrev EventType := String
abbrev Version := Nat

/-- Event: type, tags, version (monotonically increasing id).
    Tags are read with set semantics (`⊆` / `∈`); the implementation
    canonicalizes them (sort + dedup) before touching the store. -/
structure Event where
  type : EventType
  tags : List Tag
  version : Version

/-- Query item, mirroring the implementation struct: a list of candidate
    types and a set of required tags. Empty `types` means "any type";
    empty `tags` means "no tag constraint" (type-only). -/
structure QueryItem where
  types : List EventType
  tags : List Tag

/-- Query: OR of items, with version filter. -/
structure Query where
  items : List QueryItem
  afterVersion : Version

/-- Index bucket, one uniform shape mirroring the key layout
    `(ns, "i", tags…, "_", type, vs)`: a tag subset followed by the type.
    The type-only slot is simply the bucket with `tags = []`. -/
structure Bucket where
  tags : List Tag
  type : EventType

/-- Read target: a tag set plus an optional type. `none` models the
    tags-only discovery scan over the `(tags, "_")` prefix, which covers
    every type stored under that tag subset. -/
structure ReadTarget where
  tags : List Tag
  type : Option EventType
  afterVersion : Version

end DCBConflict
