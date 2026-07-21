use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::synthetic::{GraphDataset, GraphSpec, Topology, generate};

use super::backend::run_backend;
use super::{Backend, BenchmarkError, BenchmarkReport};
use super::{QueryDirection, QueryPattern, QueryWorkload, generate_workload};

const HOT_NODE_COUNT: usize = 16;
static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Inputs for one packed-versus-SQLite benchmark run.
#[derive(Clone, Debug)]
pub struct BenchmarkConfig {
    pub graph: GraphSpec,
    pub query_count: usize,
    pub sample_count: usize,
    pub work_dir: PathBuf,
    pub keep_files: bool,
}

impl BenchmarkConfig {
    pub fn new(
        graph: GraphSpec,
        query_count: usize,
        sample_count: usize,
        work_dir: impl Into<PathBuf>,
        keep_files: bool,
    ) -> Self {
        Self {
            graph,
            query_count,
            sample_count,
            work_dir: work_dir.into(),
            keep_files,
        }
    }
}

/// Generates one dataset and executes identical workloads against both backends.
pub fn run_benchmark(config: &BenchmarkConfig) -> Result<BenchmarkReport, BenchmarkError> {
    validate_config(config)?;
    fs::create_dir_all(&config.work_dir)?;
    let dataset = generate(&config.graph)?;
    let workloads = shared_workloads(&dataset, config.query_count, config.graph.seed)?;
    let graph_name = graph_name(config.graph);
    let run_id = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut files = GeneratedFiles::new(config.keep_files);
    let mut report = BenchmarkReport::new();

    for sample in 0..config.sample_count {
        let sample = u32::try_from(sample)
            .map_err(|_| BenchmarkError::InvalidConfig("sample_count exceeds u32"))?;
        let packed_path = benchmark_path(&config.work_dir, run_id, sample, Backend::Packed);
        let sqlite_path = benchmark_path(&config.work_dir, run_id, sample, Backend::Sqlite);
        files.push(packed_path.clone());
        files.push(sqlite_path.clone());

        let order = if sample.is_multiple_of(2) {
            [Backend::Packed, Backend::Sqlite]
        } else {
            [Backend::Sqlite, Backend::Packed]
        };
        let mut packed = None;
        let mut sqlite = None;
        for backend in order {
            let path = match backend {
                Backend::Packed => &packed_path,
                Backend::Sqlite => &sqlite_path,
                Backend::Overlay | Backend::RebuiltPacked => {
                    unreachable!("standard benchmark order contains only packed and SQLite")
                }
            };
            let observations = run_backend(
                backend,
                path,
                &dataset,
                &workloads,
                sample,
                &graph_name,
                &mut report,
            )?;
            match backend {
                Backend::Packed => packed = Some(observations),
                Backend::Sqlite => sqlite = Some(observations),
                Backend::Overlay | Backend::RebuiltPacked => {
                    unreachable!("standard benchmark order contains only packed and SQLite")
                }
            }
        }
        compare_observations(
            sample,
            &packed.expect("packed benchmark ran"),
            &sqlite.expect("SQLite benchmark ran"),
        )?;
    }
    Ok(report)
}

pub(super) fn validate_config(config: &BenchmarkConfig) -> Result<(), BenchmarkError> {
    if config.query_count == 0 {
        return Err(BenchmarkError::InvalidConfig(
            "query_count must be greater than zero",
        ));
    }
    if config.sample_count == 0 {
        return Err(BenchmarkError::InvalidConfig(
            "sample_count must be greater than zero",
        ));
    }
    config.graph.validate()?;
    Ok(())
}

pub(super) fn shared_workloads(
    dataset: &GraphDataset,
    query_count: usize,
    seed: u64,
) -> Result<Vec<NamedWorkload>, BenchmarkError> {
    let definitions = [
        (
            "random-forward",
            QueryPattern::Random,
            QueryDirection::Forward,
        ),
        (
            "random-reverse",
            QueryPattern::Random,
            QueryDirection::Reverse,
        ),
        (
            "sequential-forward",
            QueryPattern::Sequential,
            QueryDirection::Forward,
        ),
        (
            "sequential-reverse",
            QueryPattern::Sequential,
            QueryDirection::Reverse,
        ),
        (
            "hot-forward",
            QueryPattern::HotNodes {
                count: HOT_NODE_COUNT,
            },
            QueryDirection::Forward,
        ),
        (
            "hot-reverse",
            QueryPattern::HotNodes {
                count: HOT_NODE_COUNT,
            },
            QueryDirection::Reverse,
        ),
    ];
    definitions
        .into_iter()
        .enumerate()
        .map(|(index, (name, pattern, direction))| {
            Ok(NamedWorkload {
                name,
                workload: generate_workload(
                    dataset,
                    pattern,
                    direction,
                    query_count,
                    seed.wrapping_add(index as u64),
                )?,
            })
        })
        .collect()
}

fn compare_observations(
    sample: u32,
    packed: &[QueryObservation],
    sqlite: &[QueryObservation],
) -> Result<(), BenchmarkError> {
    for (packed, sqlite) in packed.iter().zip(sqlite) {
        if packed.items != sqlite.items || packed.fingerprint != sqlite.fingerprint {
            return Err(BenchmarkError::BackendMismatch {
                sample,
                workload: packed.workload.to_owned(),
                packed_items: packed.items,
                sqlite_items: sqlite.items,
                packed_fingerprint: packed.fingerprint,
                sqlite_fingerprint: sqlite.fingerprint,
            });
        }
    }
    Ok(())
}

pub(super) fn graph_name(spec: GraphSpec) -> String {
    let topology = match spec.topology {
        Topology::Modular { .. } => "modular",
        Topology::Entangled { .. } => "entangled",
        Topology::HubHeavy { .. } => "hub-heavy",
        Topology::Layered { .. } => "layered",
        Topology::DenseSubsystem { .. } => "dense-subsystem",
    };
    format!(
        "{topology}-n{}-e{}-seed{}",
        spec.node_count, spec.edge_count, spec.seed
    )
}

fn benchmark_path(work_dir: &Path, run_id: u64, sample: u32, backend: Backend) -> PathBuf {
    let extension = match backend {
        Backend::Packed => "pack",
        Backend::Sqlite => "sqlite",
        Backend::Overlay => "overlay",
        Backend::RebuiltPacked => "pack",
    };
    work_dir.join(format!(
        "arcana-benchmark-{}-{run_id}-{sample}.{extension}",
        std::process::id()
    ))
}

pub(super) struct NamedWorkload {
    pub(super) name: &'static str,
    pub(super) workload: QueryWorkload,
}

pub(super) struct QueryObservation {
    pub(super) workload: &'static str,
    pub(super) items: u64,
    pub(super) fingerprint: u64,
}

struct GeneratedFiles {
    paths: Vec<PathBuf>,
    keep_files: bool,
}

impl GeneratedFiles {
    fn new(keep_files: bool) -> Self {
        Self {
            paths: Vec::new(),
            keep_files,
        }
    }

    fn push(&mut self, path: PathBuf) {
        self.paths.push(path);
    }
}

impl Drop for GeneratedFiles {
    fn drop(&mut self) {
        if self.keep_files {
            return;
        }
        for path in &self.paths {
            let _ = fs::remove_file(path);
            let _ = fs::remove_file(format!("{}-journal", path.display()));
        }
    }
}
