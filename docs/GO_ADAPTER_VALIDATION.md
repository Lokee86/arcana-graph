# Go adapter real-repository validation

Validated on July 21, 2026 against two existing Go modules without modifying
either source repository.

## Results

| Repository | Nodes | Edges | Indexed files | Packages | Call expressions | Resolved direct calls | Unresolved facts | Packed graph | Catalogue | Unresolved file |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Demon Docs | 2,871 | 5,964 | 353 | 33 | 14,727 | 3,333 | 11,394 | 117,648 B | 350,220 B | 1,307,296 B |
| Space Rocks game server | 4,091 | 7,651 | 528 | 53 | 15,370 | 3,497 | 11,873 | 157,424 B | 591,426 B | 1,495,806 B |

The first-pass resolver covered approximately 22.6% of call expressions in
Demon Docs and 22.8% in the Space Rocks game server. This is a deliberately
narrow baseline: only unambiguous, unqualified, same-package function calls are
resolved.

## Query checks

Demon Docs `ManagedRootTitle` resolved to its source declaration, outgoing calls
to `FirstHeadingTitle` and `TitleFromName`, and the reverse call from
`TestTitlesAndRootTitleFallbacks`.

Space Rocks `ProjectEventLane` resolved to its source declaration, its outgoing
call to `sequenceBackedBatchID`, and reverse calls from `BuildEventBatchPacket`
and four tests.

Both fact files were regenerated and compared byte-for-byte. The repeated
outputs were identical.

## Current omissions

The adapter does not yet resolve:

- selector and method calls;
- interface dispatch;
- internal cross-package calls;
- built-ins and calls through variables;
- calls nested in function literals;
- recursive self-calls, because the current packed graph rejects self-edges;
- build-tag and generated-code semantics beyond ordinary parsing.

These omissions are represented as unresolved-reference facts rather than
possibly incorrect edges. Every unresolved call in both validation repositories
produced exactly one fact carrying its source node, intended relation,
expression, candidate namespace and name when available, resolution reason, and
source span. The importer persisted those records in `unresolved.tsv`.
