# Go adapter real-repository validation

Validated on July 21, 2026 against two existing Go modules without modifying
either source repository.

## Results

| Repository | Nodes | Edges | Indexed files | Packages | Call expressions | Resolved call expressions | Raw coverage | Repository-eligible calls | Eligible coverage | Unresolved facts | Packed graph | Catalogue | Unresolved file |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Demon Docs | 2,871 | 7,167 | 353 | 33 | 15,803 | 4,749 | 30.1% | 5,218 | 91.0% | 11,054 | 132,096 B | 350,220 B | 1,260,072 B |
| Space Rocks game server | 4,091 | 11,638 | 528 | 53 | 17,557 | 8,350 | 47.6% | 8,704 | 95.9% | 9,207 | 205,264 B | 591,426 B | 1,117,936 B |

Raw coverage divides resolved internal calls by every Go call expression. That
denominator also includes standard-library and third-party calls, built-ins such
as `len` and `append`, and type conversions such as `Widget(value)`. Those
expressions cannot resolve to callable nodes inside the indexed repository.

Repository-eligible coverage excludes external calls, built-ins, and type
conversions. It retains dynamic calls, missing internal targets, ambiguity, and
unsupported forms as honest unresolved work. On that denominator, the resolver
covers 91.0% of Demon Docs and 95.9% of the Space Rocks game server.

Compared with the syntax-only baseline, resolved call expressions increased from
3,333 to 4,749 in Demon Docs and from 3,497 to 8,350 in Space Rocks. The new
resolver also includes named method bodies, so the total expression counts are
not identical to the earlier baseline.

Multiple call sites between the same two nodes become one packed graph edge.
The final graphs contain 3,533 unique call edges for Demon Docs and 6,715 for
Space Rocks, including 11 and 3 recursive self-edges respectively.

## Resolution behavior

The adapter uses `golang.org/x/tools/go/packages` and Go type information to
resolve:

- unqualified same-package function calls;
- same-package method calls through concrete receiver types;
- internal cross-package function calls;
- internal cross-package method calls;
- promoted and pointer-receiver methods when Go resolves a concrete target;
- recursive calls as graph self-edges.

It classifies rather than guesses at:

- standard-library and third-party calls;
- built-in functions;
- type conversions;
- calls through function variables;
- unresolved interface or dynamic dispatch;
- missing or ambiguous internal targets;
- unsupported expression forms.

Anonymous function bodies are not attributed to the enclosing function. They
remain outside the call graph until Arcana models closures as their own nodes.

Both repositories completed type loading with zero reported package errors.

## Unresolved breakdown

| Repository | Built-in | Type conversion | External | Dynamic | Missing internal | Ambiguous | Unsupported |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Demon Docs | 1,945 | 298 | 8,342 | 440 | 23 | 0 | 6 |
| Space Rocks game server | 1,870 | 688 | 6,295 | 239 | 111 | 1 | 3 |

Every unresolved expression produces exactly one fact carrying its source node,
intended relation, expression, candidate namespace and name when available,
resolution reason, and source span. `arcana import-facts` persists these records
in `unresolved.tsv`.

## Query checks

Demon Docs `ManagedRootTitle` resolved to its source declaration and outgoing
calls to `FirstHeadingTitle` and `TitleFromName`.

Space Rocks `ProjectEventLane` resolved to its source declaration and its
outgoing call to `sequenceBackedBatchID`.

Both fact files were generated twice and compared byte-for-byte. The repeated
outputs were identical. Both files imported successfully into packed Arcana
graphs, including recursive self-edges, and reopened successfully for queries.

## Remaining semantic work

The main remaining gaps are:

- interface and other genuinely dynamic dispatch;
- calls through function values;
- closure nodes and relationships;
- inactive build-tag variants and explicit multi-configuration indexing;
- generated-code classification;
- external dependency graphs when those dependencies are indexed separately.
