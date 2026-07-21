# ArcanaGraph

ArcanaGraph is an independent repository-graph project. Its role is to model
repositories as a queryable graph and to provide the storage and snapshot
foundations needed to inspect repository structure and relationships.

## Ownership boundaries

- **ArcanaGraph** owns the factual repository graph, graph storage, snapshots,
  deterministic queries, and measurements of storage representations.
- **Demon Docs** owns documentation semantics, policy, review history, and
  Codemap decisions. It consumes ArcanaGraph facts without owning the graph
  engine.
- **Context Grimoire** owns task interpretation, relevance ranking, token
  budgets, and final context construction. It queries ArcanaGraph and Demon
  Docs without becoming either system's storage layer.

ArcanaGraph remains a standalone Rust process or CLI boundary. Go consumers do
not link it through cgo or FFI.

## Storage proof of concept

The first milestone compares a packed immutable adjacency representation with a
SQLite reference implementation. The comparison will use identical generated
datasets and query workloads rather than a single toy graph.

The workload foundation currently includes five deterministic topology
families:

- **Modular** — cohesive clusters with a configurable cross-cluster edge share.
- **Entangled** — hubs, cross-cluster relationships, cycles, and local edges.
- **Hub-heavy** — a small set of nodes owns most incoming and outgoing edges.
- **Layered** — deep, mostly forward relationships with a smaller irregular
  edge set.
- **Dense subsystem** — a tightly connected subsystem inside a larger sparse
  graph.

Standard scale tiers range from 10,000 nodes and 100,000 edges to 5,000,000
nodes and 50,000,000 edges. Generation scales with requested edges rather than
enumerating every possible node pair.

Mutation plans cover single-node, local-range, scattered, hub-focused, and
percentage updates. A plan contains exact removed and replacement edges so
both storage backends receive the same update.

## Determinism and invariants

Synthetic datasets and mutations are controlled by explicit seeds. Generated
and mutated graphs guarantee:

- exact requested edge counts;
- unique directed non-self edges;
- canonical source/target/kind ordering;
- topology-specific edge distributions;
- stable output for the same specification and seed; and
- preserved edge-kind counts across mutations.

The generator uses a small internal permutation sampler and currently has no
third-party dependencies. Dataset construction is not part of the storage
performance measurement; datasets will be generated once, identified, and
reused by both backends.

## Next implementation steps

1. Define the first deliberately simple packed adjacency format.
2. Implement forward and reverse neighbor reads.
3. Implement the equivalent SQLite reference backend.
4. Add cold-build, reopen, query, mutation, overlay, and compaction benchmarks.
5. Validate synthetic results against captured Demon Docs and Space Rocks
   repository graphs.

## Development

The package uses Rust edition 2024.

```text
cargo fmt -- --check
cargo check --all-targets
cargo test --all-targets
cargo run -- --help
cargo run -- --version
```
