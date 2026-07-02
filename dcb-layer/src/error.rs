use std::convert::TryFrom;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("events slice is empty")]
    EmptyEvents,

    #[error("append condition failed")]
    AppendConditionFailed,

    #[error("query must have at least one type or tag")]
    InvalidQuery,

    #[error("event must have a type")]
    MissingEventType,

    #[error("event batch exceeds maximum size of 65535 events")]
    BatchTooLarge,

    #[error("event exceeds maximum of 10 tags")]
    TooManyTags,

    #[error("tag value \"_\" is reserved")]
    ReservedTag,

    #[error("key is all-0xFF bytes; cannot compute upper range bound")]
    AllFfKey,

    #[error("FDB error: {0}")]
    Fdb(#[from] foundationdb::FdbError),

    #[error("tuple encoding error: {0}")]
    TupleEncode(String),

    #[error("tuple decoding error: {0}")]
    TupleDecode(String),

    #[error("event not found for versionstamp {0}")]
    EventNotFound(String),

    #[error("OS random source unavailable: {0}")]
    RandomSource(String),
}

/// Required by `Database::transact_boxed`: lets the retry loop distinguish FDB
/// errors (retryable) from application errors (not retryable).
impl TryFrom<Error> for foundationdb::FdbError {
    type Error = Error;

    fn try_from(e: Error) -> Result<foundationdb::FdbError, Error> {
        match e {
            Error::Fdb(fdb_err) => Ok(fdb_err),
            other => Err(other),
        }
    }
}
