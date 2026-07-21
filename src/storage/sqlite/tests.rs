use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::storage::{
    DatasetError, InMemoryGraph, Neighbor, PackedGraph, QueryError, SqliteError, SqliteGraph,
    write_packed, write_sqlite,
};
use crate::synthetic::{
    Edge, EdgeKind, GraphDataset, GraphSpec, NodeId, ScaleTier, Topology, generate,
};

static PATH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempPath(PathBuf);

impl TempPath {
    fn new(label: &str, extension: &str) -> Self {
        let sequence = PATH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "arcana-graph-sqlite-{label}-{}-{sequence}.{extension}",
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

fn topology_specs() -> Vec<GraphSpec> {
    vec![
        GraphSpec {
            topology: Topology::Modular {
                cluster_count: 8,
                cross_cluster_ratio: 2_500,
            },
            node_count: 64,
            edge_count: 300,
            seed: 51,
        },
        GraphSpec {
            topology: Topology::Entangled {
                cluster_count: 8,
                hub_count: 4,
            },
            node_count: 64,
            edge_count: 300,
            seed: 52,
        },
        GraphSpec {
            topology: Topology::HubHeavy { hub_count: 4 },
            node_count: 64,
            edge_count: 300,
            seed: 53,
        },
        GraphSpec {
            topology: Topology::Layered { layer_count: 8 },
            node_count: 64,
            edge_count: 300,
            seed: 54,
        },
        GraphSpec {
            topology: Topology::DenseSubsystem {
                dense_node_count: 16,
            },
            node_count: 64,
            edge_count: 300,
            seed: 55,
        },
    ]
}

fn assert_round_trip(dataset: &GraphDataset, label: &str) {
    let sqlite_path = TempPath::new(label, "sqlite");
    let packed_path = TempPath::new(label, "pack");
    let oracle = InMemoryGraph::new(dataset).expect("valid reference graph");
    let sqlite_summary =
        write_sqlite(sqlite_path.as_path(), dataset).expect("SQLite write succeeds");
    let packed_summary =
        write_packed(packed_path.as_path(), dataset).expect("packed write succeeds");
    let sqlite = SqliteGraph::open(sqlite_path.as_path()).expect("SQLite graph opens");
    let packed = PackedGraph::open(packed_path.as_path()).expect("packed graph opens");

    assert_eq!(sqlite.node_count(), oracle.node_count());
    assert_eq!(sqlite.edge_count(), oracle.edge_count());
    assert_eq!(sqlite.dataset_checksum(), packed.dataset_checksum());
    assert_eq!(
        sqlite_summary.dataset_checksum,
        packed_summary.dataset_checksum
    );
    assert_eq!(
        sqlite_summary.file_len,
        fs::metadata(sqlite_path.as_path()).unwrap().len()
    );

    for node in 0..dataset.node_count {
        let node = NodeId(node);
        let forward = oracle.forward_neighbors(node).unwrap();
        let reverse = oracle.reverse_neighbors(node).unwrap();
        assert_eq!(sqlite.forward_neighbors(node).unwrap(), forward);
        assert_eq!(sqlite.reverse_neighbors(node).unwrap(), reverse);
        assert_eq!(
            sqlite.forward_neighbors(node).unwrap(),
            packed.forward_neighbors(node).unwrap()
        );
        assert_eq!(
            sqlite.reverse_neighbors(node).unwrap(),
            packed.reverse_neighbors(node).unwrap()
        );
    }
}

#[test]
fn every_synthetic_topology_matches_packed_and_oracle() {
    for (index, spec) in topology_specs().iter().enumerate() {
        let dataset = generate(spec).expect("valid synthetic graph");
        assert_round_trip(&dataset, &format!("topology-{index}"));
    }
}

#[test]
fn empty_adjacency_and_parallel_kinds_round_trip() {
    assert_round_trip(
        &GraphDataset {
            node_count: 5,
            edges: Vec::new(),
        },
        "empty",
    );

    assert_round_trip(
        &GraphDataset {
            node_count: 4,
            edges: vec![
                Edge {
                    source: NodeId(0),
                    target: NodeId(1),
                    kind: EdgeKind(2),
                },
                Edge {
                    source: NodeId(2),
                    target: NodeId(0),
                    kind: EdgeKind(3),
                },
                Edge {
                    source: NodeId(0),
                    target: NodeId(1),
                    kind: EdgeKind(1),
                },
            ],
        },
        "parallel-kinds",
    );
}

#[test]
fn writer_rejects_invalid_datasets_and_existing_paths() {
    let invalid = GraphDataset {
        node_count: 2,
        edges: vec![Edge {
            source: NodeId(0),
            target: NodeId(2),
            kind: EdgeKind(0),
        }],
    };
    let invalid_path = TempPath::new("invalid", "sqlite");
    assert!(matches!(
        write_sqlite(invalid_path.as_path(), &invalid),
        Err(SqliteError::Dataset(
            DatasetError::EndpointOutOfRange { .. }
        ))
    ));
    assert!(!invalid_path.as_path().exists());

    let existing_path = TempPath::new("existing", "sqlite");
    fs::write(existing_path.as_path(), b"owned").unwrap();
    assert!(matches!(
        write_sqlite(
            existing_path.as_path(),
            &GraphDataset {
                node_count: 1,
                edges: Vec::new(),
            }
        ),
        Err(SqliteError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists
    ));
    assert_eq!(fs::read(existing_path.as_path()).unwrap(), b"owned");
}

#[test]
fn invalid_node_queries_are_explicit() {
    let path = TempPath::new("invalid-query", "sqlite");
    write_sqlite(
        path.as_path(),
        &GraphDataset {
            node_count: 2,
            edges: Vec::new(),
        },
    )
    .unwrap();
    let graph = SqliteGraph::open(path.as_path()).unwrap();

    assert!(matches!(
        graph.forward_neighbors(NodeId(2)),
        Err(SqliteError::Query(QueryError::InvalidNode {
            node: NodeId(2),
            node_count: 2,
        }))
    ));
}

#[test]
#[ignore = "medium-scale SQLite storage smoke"]
fn medium_scale_sqlite_smoke() {
    let dataset = generate(&GraphSpec::for_tier(
        Topology::Modular {
            cluster_count: 1_000,
            cross_cluster_ratio: 2_500,
        },
        ScaleTier::Medium,
        87,
    ))
    .expect("valid medium graph");
    let path = TempPath::new("medium", "sqlite");
    let summary = write_sqlite(path.as_path(), &dataset).expect("medium SQLite write");
    let graph = SqliteGraph::open(path.as_path()).expect("medium SQLite open");

    assert_eq!(summary.node_count, 100_000);
    assert_eq!(summary.edge_count, 1_000_000);
    for node in [0, 1, 999, 50_000, 99_999] {
        let mut forward: Vec<_> = dataset
            .edges
            .iter()
            .filter(|edge| edge.source == NodeId(node))
            .map(|edge| Neighbor {
                node: edge.target,
                kind: edge.kind,
            })
            .collect();
        let mut reverse: Vec<_> = dataset
            .edges
            .iter()
            .filter(|edge| edge.target == NodeId(node))
            .map(|edge| Neighbor {
                node: edge.source,
                kind: edge.kind,
            })
            .collect();
        forward.sort_unstable();
        reverse.sort_unstable();
        assert_eq!(graph.forward_neighbors(NodeId(node)).unwrap(), forward);
        assert_eq!(graph.reverse_neighbors(NodeId(node)).unwrap(), reverse);
    }
}
