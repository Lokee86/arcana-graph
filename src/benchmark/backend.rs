use std::path::Path;
use std::time::Instant;

use crate::storage::{Neighbor, PackedGraph, SqliteGraph, write_packed, write_sqlite};
use crate::synthetic::{GraphDataset, NodeId};

use super::runner::{NamedWorkload, QueryObservation};
use super::{Backend, BenchmarkError, BenchmarkMetric, BenchmarkReport, BenchmarkSample};
use super::{QueryDirection, QueryWorkload};

const DATASET_WORKLOAD: &str = "dataset";

pub(super) fn run_backend(
    backend: Backend,
    path: &Path,
    dataset: &GraphDataset,
    workloads: &[NamedWorkload],
    sample: u32,
    graph_name: &str,
    report: &mut BenchmarkReport,
) -> Result<Vec<QueryObservation>, BenchmarkError> {
    let started = Instant::now();
    let (node_count, edge_count, file_size) = match backend {
        Backend::Packed => {
            let summary = write_packed(path, dataset)?;
            (summary.node_count, summary.edge_count, summary.file_len)
        }
        Backend::Sqlite => {
            let summary = write_sqlite(path, dataset)?;
            (summary.node_count, summary.edge_count, summary.file_len)
        }
        Backend::Overlay | Backend::RebuiltPacked => {
            return Err(BenchmarkError::InvalidConfig(
                "packed/SQLite runner received a mutation backend",
            ));
        }
    };
    report.push(BenchmarkSample::new(
        graph_name,
        backend,
        BenchmarkMetric::Build,
        DATASET_WORKLOAD,
        sample,
        started.elapsed(),
        edge_count,
        u64::from(node_count),
        file_size,
        0,
    ));

    let started = Instant::now();
    let graph = match backend {
        Backend::Packed => BackendGraph::Packed(PackedGraph::open(path)?),
        Backend::Sqlite => BackendGraph::Sqlite(SqliteGraph::open(path)?),
        Backend::Overlay | Backend::RebuiltPacked => {
            return Err(BenchmarkError::InvalidConfig(
                "packed/SQLite runner received a mutation backend",
            ));
        }
    };
    report.push(BenchmarkSample::new(
        graph_name,
        backend,
        BenchmarkMetric::Reopen,
        DATASET_WORKLOAD,
        sample,
        started.elapsed(),
        edge_count,
        u64::from(graph.node_count()),
        file_size,
        0,
    ));

    let mut observations = Vec::with_capacity(workloads.len());
    for named in workloads {
        std::hint::black_box(execute_workload(&graph, &named.workload)?);
        let started = Instant::now();
        let (items, fingerprint) = execute_workload(&graph, &named.workload)?;
        report.push(BenchmarkSample::new(
            graph_name,
            backend,
            BenchmarkMetric::Query,
            named.name,
            sample,
            started.elapsed(),
            named.workload.len() as u64,
            items,
            file_size,
            fingerprint,
        ));
        observations.push(QueryObservation {
            workload: named.name,
            items,
            fingerprint,
        });
    }
    Ok(observations)
}

fn execute_workload(
    graph: &BackendGraph,
    workload: &QueryWorkload,
) -> Result<(u64, u64), BenchmarkError> {
    let mut items = 0_u64;
    let mut fingerprint = 0xcbf2_9ce4_8422_2325_u64;
    for &node in workload.node_ids() {
        let neighbors = graph.neighbors(node, workload.direction)?;
        fingerprint = mix(fingerprint, u64::from(node.0));
        fingerprint = mix(fingerprint, neighbors.len() as u64);
        for neighbor in neighbors {
            items += 1;
            fingerprint = mix_neighbor(fingerprint, neighbor);
        }
    }
    Ok((items, std::hint::black_box(fingerprint)))
}

fn mix(value: u64, input: u64) -> u64 {
    (value ^ input.wrapping_add(0x9e37_79b9_7f4a_7c15))
        .wrapping_mul(0x1000_0000_01b3)
        .rotate_left(13)
}

fn mix_neighbor(value: u64, neighbor: Neighbor) -> u64 {
    mix(
        mix(value, u64::from(neighbor.node.0)),
        u64::from(neighbor.kind.0),
    )
}

enum BackendGraph {
    Packed(PackedGraph),
    Sqlite(SqliteGraph),
}

impl BackendGraph {
    fn node_count(&self) -> u32 {
        match self {
            Self::Packed(graph) => graph.node_count(),
            Self::Sqlite(graph) => graph.node_count(),
        }
    }

    fn neighbors(
        &self,
        node: NodeId,
        direction: QueryDirection,
    ) -> Result<Vec<Neighbor>, BenchmarkError> {
        match (self, direction) {
            (Self::Packed(graph), QueryDirection::Forward) => Ok(graph.forward_neighbors(node)?),
            (Self::Packed(graph), QueryDirection::Reverse) => Ok(graph.reverse_neighbors(node)?),
            (Self::Sqlite(graph), QueryDirection::Forward) => Ok(graph.forward_neighbors(node)?),
            (Self::Sqlite(graph), QueryDirection::Reverse) => Ok(graph.reverse_neighbors(node)?),
        }
    }
}
