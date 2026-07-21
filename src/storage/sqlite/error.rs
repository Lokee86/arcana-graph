use std::fmt;
use std::io;

use crate::storage::{DatasetError, QueryError};

/// A SQLite graph that cannot be written, opened, or queried.
#[derive(Debug)]
pub enum SqliteError {
    Io(io::Error),
    Database(rusqlite::Error),
    Dataset(DatasetError),
    Query(QueryError),
    SizeOverflow,
    InvalidApplicationId { found: i64 },
    UnsupportedVersion { found: i64 },
    IntegrityCheckFailed { message: String },
    MissingMetadata,
    InvalidMetadata { field: &'static str, value: i64 },
    EdgeCountMismatch { declared: u64, actual: u64 },
    DatasetChecksumMismatch { expected: u64, actual: u64 },
    InvalidEdge { source: i64, target: i64, kind: i64 },
}

impl fmt::Display for SqliteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::Database(error) => error.fmt(formatter),
            Self::Dataset(error) => error.fmt(formatter),
            Self::Query(error) => error.fmt(formatter),
            Self::SizeOverflow => formatter.write_str("SQLite graph size exceeds supported limits"),
            Self::InvalidApplicationId { found } => {
                write!(formatter, "SQLite application id {found:#x} is invalid")
            }
            Self::UnsupportedVersion { found } => {
                write!(
                    formatter,
                    "SQLite graph format version {found} is unsupported"
                )
            }
            Self::IntegrityCheckFailed { message } => {
                write!(formatter, "SQLite integrity check failed: {message}")
            }
            Self::MissingMetadata => formatter.write_str("SQLite graph metadata row is missing"),
            Self::InvalidMetadata { field, value } => {
                write!(
                    formatter,
                    "SQLite graph metadata {field} has invalid value {value}"
                )
            }
            Self::EdgeCountMismatch { declared, actual } => write!(
                formatter,
                "SQLite graph declares {declared} edges but contains {actual}"
            ),
            Self::DatasetChecksumMismatch { expected, actual } => write!(
                formatter,
                "SQLite dataset checksum mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::InvalidEdge {
                source,
                target,
                kind,
            } => write!(
                formatter,
                "SQLite graph contains invalid edge {source} -> {target} of kind {kind}"
            ),
        }
    }
}

impl std::error::Error for SqliteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::Dataset(error) => Some(error),
            Self::Query(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for SqliteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for SqliteError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<DatasetError> for SqliteError {
    fn from(error: DatasetError) -> Self {
        Self::Dataset(error)
    }
}

impl From<QueryError> for SqliteError {
    fn from(error: QueryError) -> Self {
        Self::Query(error)
    }
}
