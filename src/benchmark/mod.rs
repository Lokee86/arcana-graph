//! Shared deterministic workloads for graph storage benchmarks.

mod workload;

pub use workload::{
    QueryDirection, QueryPattern, QueryWorkload, QueryWorkloadError, generate_workload,
};
