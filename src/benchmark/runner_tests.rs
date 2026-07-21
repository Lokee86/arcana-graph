use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Backend, BenchmarkConfig, BenchmarkError, BenchmarkMetric, run_benchmark};
use crate::synthetic::{GraphSpec, Topology};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn config(query_count: usize, sample_count: usize) -> BenchmarkConfig {
    let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let work_dir = std::env::temp_dir().join(format!(
        "arcana-benchmark-test-{}-{sequence}",
        std::process::id()
    ));
    BenchmarkConfig::new(
        GraphSpec {
            topology: Topology::Modular {
                cluster_count: 4,
                cross_cluster_ratio: 2_500,
            },
            node_count: 32,
            edge_count: 150,
            seed: 17,
        },
        query_count,
        sample_count,
        work_dir,
        false,
    )
}

#[test]
fn both_backends_complete_matching_samples_and_cleanup() {
    let config = config(50, 2);
    let work_dir: PathBuf = config.work_dir.clone();
    let report = run_benchmark(&config).expect("benchmark should complete");

    assert_eq!(report.samples().len(), 32);
    assert_eq!(
        report
            .samples()
            .iter()
            .filter(|sample| sample.metric == BenchmarkMetric::Query)
            .count(),
        24
    );
    assert!(
        report
            .samples()
            .iter()
            .any(|sample| sample.backend == Backend::Packed)
    );
    assert!(
        report
            .samples()
            .iter()
            .any(|sample| sample.backend == Backend::Sqlite)
    );
    assert!(
        report
            .samples()
            .iter()
            .all(|sample| sample.graph == "modular-n32-e150-seed17")
    );
    assert_eq!(fs::read_dir(&work_dir).unwrap().count(), 0);
    fs::remove_dir(work_dir).unwrap();
}

#[test]
fn rejects_empty_query_or_sample_counts() {
    assert!(matches!(
        run_benchmark(&config(0, 1)),
        Err(BenchmarkError::InvalidConfig(_))
    ));
    assert!(matches!(
        run_benchmark(&config(1, 0)),
        Err(BenchmarkError::InvalidConfig(_))
    ));
}
