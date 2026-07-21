use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};

use crate::synthetic::GraphDataset;

use super::{APPLICATION_ID, FORMAT_VERSION, SqliteError};
use crate::storage::dataset::{canonical_edges, dataset_checksum};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const SCHEMA: &str = r#"
PRAGMA journal_mode = DELETE;
PRAGMA synchronous = FULL;
PRAGMA temp_store = MEMORY;
PRAGMA locking_mode = EXCLUSIVE;
CREATE TABLE graph_metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    node_count INTEGER NOT NULL CHECK (node_count BETWEEN 0 AND 4294967295),
    edge_count INTEGER NOT NULL CHECK (edge_count >= 0),
    dataset_checksum INTEGER NOT NULL
);

CREATE TABLE edges (
    source INTEGER NOT NULL CHECK (source >= 0),
    target INTEGER NOT NULL CHECK (target >= 0),
    kind INTEGER NOT NULL CHECK (kind BETWEEN 0 AND 65535),
    PRIMARY KEY (source, target, kind)
) WITHOUT ROWID;

CREATE INDEX edges_by_target ON edges (target, source, kind);
"#;

/// Metadata produced after an immutable SQLite graph is committed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SqliteWriteSummary {
    pub node_count: u32,
    pub edge_count: u64,
    pub dataset_checksum: u64,
    pub file_len: u64,
}

/// Writes a new immutable SQLite graph and refuses to replace an existing path.
pub fn write_sqlite(
    path: impl AsRef<Path>,
    dataset: &GraphDataset,
) -> Result<SqliteWriteSummary, SqliteError> {
    let path = path.as_ref();
    if path.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("SQLite graph already exists: {}", path.display()),
        )
        .into());
    }

    let temp_path = temporary_path(path);
    let result = write_then_commit(path, &temp_path, dataset);
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
        let _ = fs::remove_file(journal_path(&temp_path));
    }
    result
}

fn write_then_commit(
    path: &Path,
    temp_path: &Path,
    dataset: &GraphDataset,
) -> Result<SqliteWriteSummary, SqliteError> {
    let edges = canonical_edges(dataset)?;
    let edge_count = u64::try_from(edges.len()).map_err(|_| SqliteError::SizeOverflow)?;
    let edge_count_i64 = i64::try_from(edge_count).map_err(|_| SqliteError::SizeOverflow)?;
    let checksum = dataset_checksum(dataset.node_count, &edges);

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let mut connection = Connection::open_with_flags(temp_path, flags)?;
    connection.execute_batch(SCHEMA)?;
    connection.pragma_update(None, "application_id", APPLICATION_ID)?;
    connection.pragma_update(None, "user_version", FORMAT_VERSION)?;

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    {
        let mut insert = transaction
            .prepare_cached("INSERT INTO edges (source, target, kind) VALUES (?1, ?2, ?3)")?;
        for edge in &edges {
            insert.execute(params![
                i64::from(edge.source.0),
                i64::from(edge.target.0),
                i64::from(edge.kind.0),
            ])?;
        }
    }
    transaction.execute(
        "INSERT INTO graph_metadata \
         (singleton, node_count, edge_count, dataset_checksum) \
         VALUES (1, ?1, ?2, ?3)",
        params![
            i64::from(dataset.node_count),
            edge_count_i64,
            checksum as i64,
        ],
    )?;
    transaction.commit()?;
    connection.execute_batch("PRAGMA optimize;")?;
    connection.close().map_err(|(_, error)| error)?;

    fs::rename(temp_path, path)?;
    let file_len = fs::metadata(path)?.len();
    Ok(SqliteWriteSummary {
        node_count: dataset.node_count,
        edge_count,
        dataset_checksum: checksum,
        file_len,
    })
}

fn temporary_path(path: &Path) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut name = path
        .file_name()
        .map_or_else(|| OsString::from("arcana"), OsString::from);
    name.push(format!(".tmp.{}.{}", std::process::id(), sequence));
    path.with_file_name(name)
}

fn journal_path(path: &Path) -> PathBuf {
    let mut journal = path.as_os_str().to_owned();
    journal.push("-journal");
    PathBuf::from(journal)
}
