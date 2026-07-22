# Go repository adapter

The adapter scans a Go module and writes canonical Arcana repository facts. It
uses `go/parser` and `go/ast` for repository-wide extraction, then
`golang.org/x/tools/go/packages` and Go type information for semantic call
resolution.

Requirements:

- Go 1.22 or newer;
- a repository root containing `go.mod`.

From this directory:

```text
go run . -repo /path/to/repository -output /path/to/repository.facts.tsv
go test ./...
```

The scanner skips `.git`, `.worktrees`, `.workingtrees`, and `vendor`
directories. The deterministic UTF-8 TSV output uses fact format version 2:

- `N` records for nodes;
- `E` records for resolved relationships;
- `U` records for observed but unresolved relationships.

The semantic resolver emits internal same-package and cross-package function and
method calls, including recursion. It classifies built-ins, type conversions,
external calls, dynamic dispatch, ambiguity, and missing targets instead of
inventing edges. Anonymous function bodies remain outside the call graph until
Arcana models closures as independent nodes.

Node identities and file content IDs use FNV-1a 64-bit with the same offset basis
and prime as Arcana's Rust `StableHasher`. Identity strings include a kind prefix
to keep categories distinct.

The command replaces an existing fact output file. Arcana's importer converts
the facts into immutable `graph.arcana`, `catalogue.tsv`, and `unresolved.tsv`
artifacts.
