use bytes::Bytes;
use foundationdb::Database;

/// 12-byte globally unique, monotonically increasing position.
/// Bytes 0–9: FDB transaction version (big-endian).
/// Bytes 10–11: user version = batch index (big-endian u16).
pub type Versionstamp = [u8; 12];

#[derive(Debug, Clone)]
pub struct Event {
    pub type_name: String,
    pub tags: Vec<String>,
    pub data: Bytes,
}

impl Event {
    pub fn new(
        type_name: impl Into<String>,
        tags: impl IntoIterator<Item = impl Into<String>>,
        data: impl Into<Bytes>,
    ) -> Self {
        Self {
            type_name: type_name.into(),
            tags: tags.into_iter().map(Into::into).collect(),
            data: data.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub event: Event,
    pub position: Versionstamp,
}

#[derive(Debug, Clone)]
pub struct QueryItem {
    pub types: Vec<String>,
    pub tags: Vec<String>,
}

impl QueryItem {
    pub(crate) fn has_no_type_nor_tags(&self) -> bool {
        self.types.is_empty() && self.tags.is_empty()
    }

    pub(crate) fn has_types_only(&self) -> bool {
        !self.types.is_empty() && self.tags.is_empty()
    }

    pub(crate) fn has_types_and_tags(&self) -> bool {
        !self.types.is_empty() && !self.tags.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Query {
    pub items: Vec<QueryItem>,
}

#[derive(Debug, Clone)]
pub struct AppendCondition {
    pub query: Query,
    pub after: Option<Versionstamp>,
}

#[derive(Debug, Clone)]
pub struct ReadOptions {
    pub limit: usize,
    pub after: Option<Versionstamp>,
    pub reverse: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self { limit: 0, after: None, reverse: false }
    }
}

pub struct FdbStore {
    pub(crate) db: Database,
    pub(crate) namespace: String,
}

impl FdbStore {
    pub fn new(db: Database, namespace: impl Into<String>) -> Self {
        Self { db, namespace: namespace.into() }
    }
}
