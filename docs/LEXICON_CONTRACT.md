# Lexicon ingestion boundary

Arcana consumes deterministic Lexicon facts v1 JSONL and compacts transport identities into its packed repository graph.

## Identity boundary

Lexicon owns cross-tool SHA-256 node identities. Arcana stores each full Lexicon identity in the catalogue, hashes it into an internal 64-bit `NodeKey`, checks for compaction collisions during import, and assigns dense packed `NodeId` values during compilation. Dense IDs are snapshot-local and must never escape as durable cross-tool identities. Lexicon file content IDs are compacted for Arcana's internal change detection.

Arcana continues to read its legacy TSV facts during migration, but no language adapter is owned by this repository.

## Preserved semantics

Arcana accepts the common Lexicon node and relation vocabulary, including:

- interfaces, traits, constructors, and parameters;
- definite `calls` and conservative `possible-calls` as separate relations;
- conversions, implementations, inheritance, trait use, overrides, reads, writes, and annotations;
- unresolved references with source spans and candidate metadata.

Source spans are preserved in the catalogue and unresolved-reference store. Explicit file ownership drives Arcana's file-scoped replacement model. Arbitrary Lexicon `attributes` are currently ignored rather than persisted; adding a provenance sidecar later will not require changing graph identity.

## Incremental ownership

Lexicon defines ownership and deletion semantics for scoped incremental streams. Arcana's current `update-facts` path consumes complete Lexicon views, compares file content identities, replaces changed-file-owned facts, and emits graph overlays. Direct ingestion of Lexicon `mode=incremental` streams is intentionally rejected until snapshot manifests persist the declared `changed_files` and `removed_files` scope. This prevents partial input from being mistaken for a complete repository.
