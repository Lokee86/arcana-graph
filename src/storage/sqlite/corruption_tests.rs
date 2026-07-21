use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use crate::storage::{SqliteError, SqliteGraph, write_sqlite};
use crate::synthetic::{GraphSpec, Topology, generate};

static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempPath(PathBuf);

impl TempPath {
    fn new(label: &str) -> Self {
        let sequence = PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "arcana-graph-sqlite-corrupt-{label}-{}-{sequence}.sqlite",
            std::process::id()
        )))
    }

    fn as_path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn base_database() -> TempPath {
    let path = TempPath::new("base");
    let dataset = generate(&GraphSpec {
        topology: Topology::Entangled {
            cluster_count: 8,
            hub_count: 4,
        },
        node_count: 64,
        edge_count: 300,
        seed: 92,
    })
    .expect("valid synthetic graph");
    write_sqlite(path.as_path(), &dataset).unwrap();
    path
}

fn mutate_copy(source: &Path, label: &str, sql: &str) -> TempPath {
    let path = TempPath::new(label);
    fs::copy(source, path.as_path()).unwrap();
    let connection = Connection::open(path.as_path()).unwrap();
    connection.execute_batch(sql).unwrap();
    connection.close().unwrap();
    path
}

#[test]
fn reader_rejects_identity_and_metadata_corruption() {
    let base = base_database();

    let application = mutate_copy(base.as_path(), "application", "PRAGMA application_id = 1;");
    assert!(matches!(
        SqliteGraph::open(application.as_path()),
        Err(SqliteError::InvalidApplicationId { found: 1 })
    ));

    let version = mutate_copy(base.as_path(), "version", "PRAGMA user_version = 2;");
    assert!(matches!(
        SqliteGraph::open(version.as_path()),
        Err(SqliteError::UnsupportedVersion { found: 2 })
    ));

    let missing = mutate_copy(base.as_path(), "missing", "DELETE FROM graph_metadata;");
    assert!(matches!(
        SqliteGraph::open(missing.as_path()),
        Err(SqliteError::MissingMetadata)
    ));

    let checksum = mutate_copy(
        base.as_path(),
        "checksum",
        "UPDATE graph_metadata SET dataset_checksum = dataset_checksum + 1;",
    );
    assert!(matches!(
        SqliteGraph::open(checksum.as_path()),
        Err(SqliteError::DatasetChecksumMismatch { .. })
    ));
}

#[test]
fn reader_rejects_edge_count_and_logical_graph_corruption() {
    let base = base_database();

    let count = mutate_copy(
        base.as_path(),
        "count",
        "UPDATE graph_metadata SET edge_count = edge_count + 1;",
    );
    assert!(matches!(
        SqliteGraph::open(count.as_path()),
        Err(SqliteError::EdgeCountMismatch { .. })
    ));

    let edge = mutate_copy(
        base.as_path(),
        "edge",
        "DELETE FROM edges WHERE (source, target, kind) = \
         (SELECT source, target, kind FROM edges ORDER BY source, target, kind LIMIT 1);",
    );
    assert!(matches!(
        SqliteGraph::open(edge.as_path()),
        Err(SqliteError::EdgeCountMismatch { .. })
            | Err(SqliteError::DatasetChecksumMismatch { .. })
    ));
}
