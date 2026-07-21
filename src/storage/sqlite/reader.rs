use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::storage::format::StableHasher;
use crate::storage::{Neighbor, QueryError};
use crate::synthetic::{EdgeKind, NodeId};

use super::{APPLICATION_ID, FORMAT_VERSION, SqliteError};

/// Validated immutable graph backed by one read-only SQLite connection.
pub struct SqliteGraph {
    connection: Connection,
    node_count: u32,
    edge_count: u64,
    dataset_checksum: u64,
}

impl SqliteGraph {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SqliteError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags)?;
        connection.execute_batch("PRAGMA query_only = ON;")?;
        validate_identity(&connection)?;
        validate_integrity(&connection)?;
        let (node_count, edge_count, dataset_checksum) = read_metadata(&connection)?;
        validate_dataset(&connection, node_count, edge_count, dataset_checksum)?;

        Ok(Self {
            connection,
            node_count,
            edge_count,
            dataset_checksum,
        })
    }

    pub const fn node_count(&self) -> u32 {
        self.node_count
    }

    pub const fn edge_count(&self) -> u64 {
        self.edge_count
    }

    pub const fn dataset_checksum(&self) -> u64 {
        self.dataset_checksum
    }

    pub fn forward_neighbors(&self, node: NodeId) -> Result<Vec<Neighbor>, SqliteError> {
        self.neighbors(
            node,
            "SELECT target, kind FROM edges \
             WHERE source = ?1 ORDER BY target, kind",
        )
    }

    pub fn reverse_neighbors(&self, node: NodeId) -> Result<Vec<Neighbor>, SqliteError> {
        self.neighbors(
            node,
            "SELECT source, kind FROM edges \
             WHERE target = ?1 ORDER BY source, kind",
        )
    }

    fn neighbors(&self, node: NodeId, sql: &str) -> Result<Vec<Neighbor>, SqliteError> {
        if node.0 >= self.node_count {
            return Err(QueryError::InvalidNode {
                node,
                node_count: self.node_count,
            }
            .into());
        }

        let mut statement = self.connection.prepare_cached(sql)?;
        let rows = statement.query_map(params![i64::from(node.0)], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut neighbors = Vec::new();
        for row in rows {
            let (adjacent, kind) = row?;
            let adjacent = u32::try_from(adjacent).map_err(|_| SqliteError::InvalidEdge {
                source: i64::from(node.0),
                target: adjacent,
                kind,
            })?;
            let kind = u16::try_from(kind).map_err(|_| SqliteError::InvalidEdge {
                source: i64::from(node.0),
                target: i64::from(adjacent),
                kind,
            })?;
            neighbors.push(Neighbor {
                node: NodeId(adjacent),
                kind: EdgeKind(kind),
            });
        }
        Ok(neighbors)
    }
}

fn validate_identity(connection: &Connection) -> Result<(), SqliteError> {
    let application_id: i64 =
        connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(SqliteError::InvalidApplicationId {
            found: application_id,
        });
    }
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version != FORMAT_VERSION {
        return Err(SqliteError::UnsupportedVersion { found: version });
    }
    Ok(())
}

fn validate_integrity(connection: &Connection) -> Result<(), SqliteError> {
    let message: String = connection.query_row("PRAGMA quick_check(1)", [], |row| row.get(0))?;
    if message != "ok" {
        return Err(SqliteError::IntegrityCheckFailed { message });
    }
    Ok(())
}

fn read_metadata(connection: &Connection) -> Result<(u32, u64, u64), SqliteError> {
    let metadata = connection
        .query_row(
            "SELECT node_count, edge_count, dataset_checksum \
             FROM graph_metadata WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or(SqliteError::MissingMetadata)?;

    let node_count = u32::try_from(metadata.0).map_err(|_| SqliteError::InvalidMetadata {
        field: "node_count",
        value: metadata.0,
    })?;
    let edge_count = u64::try_from(metadata.1).map_err(|_| SqliteError::InvalidMetadata {
        field: "edge_count",
        value: metadata.1,
    })?;
    Ok((node_count, edge_count, metadata.2 as u64))
}

fn validate_dataset(
    connection: &Connection,
    node_count: u32,
    edge_count: u64,
    expected_checksum: u64,
) -> Result<(), SqliteError> {
    let mut hasher = StableHasher::new();
    hasher.update(&node_count.to_le_bytes());
    hasher.update(&edge_count.to_le_bytes());

    let mut statement = connection
        .prepare("SELECT source, target, kind FROM edges ORDER BY source, target, kind")?;
    let mut rows = statement.query([])?;
    let mut actual_count = 0_u64;
    while let Some(row) = rows.next()? {
        let source = row.get::<_, i64>(0)?;
        let target = row.get::<_, i64>(1)?;
        let kind = row.get::<_, i64>(2)?;
        let valid = source >= 0
            && source < i64::from(node_count)
            && target >= 0
            && target < i64::from(node_count)
            && source != target
            && (0..=i64::from(u16::MAX)).contains(&kind);
        if !valid {
            return Err(SqliteError::InvalidEdge {
                source,
                target,
                kind,
            });
        }
        hasher.update(&(source as u32).to_le_bytes());
        hasher.update(&(target as u32).to_le_bytes());
        hasher.update(&(kind as u16).to_le_bytes());
        actual_count = actual_count
            .checked_add(1)
            .ok_or(SqliteError::SizeOverflow)?;
    }

    if actual_count != edge_count {
        return Err(SqliteError::EdgeCountMismatch {
            declared: edge_count,
            actual: actual_count,
        });
    }
    let actual_checksum = hasher.finish();
    if actual_checksum != expected_checksum {
        return Err(SqliteError::DatasetChecksumMismatch {
            expected: expected_checksum,
            actual: actual_checksum,
        });
    }
    Ok(())
}
