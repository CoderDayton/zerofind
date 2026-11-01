# Design

How `zf` is built, and the reason behind each choice.

- [Pipeline](#pipeline)
- [The plan is fixed before the walk](#the-plan-is-fixed-before-the-walk)
- [Grammar instead of flags](#grammar-instead-of-flags)
- [Pruning before descent](#pruning-before-descent)
- [One writer, bounded queue](#one-writer-bounded-queue)
- [Records travel inline](#records-travel-inline)
- [Errors are typed until the edge](#errors-are-typed-until-the-edge)
- [Invariants](#invariants)
- [Out of scope](#out-of-scope)

## Pipeline

```text
argv -> cli -> SearchPlan -> traversal: N workers filter + match -> bounded queue -> writer -> stdout
```

| Module | Owns | Never touches |
|---|---|---|
| `cli` | argument parsing, help | search logic |
| `domain::query` | query compilation, name and path matching | the filesystem |
| `domain::scope` | root resolution and validation | output formatting |
| `domain::policy` | hidden, excluded-directory and result-kind decisions | scheduling |
| `search::filtering` | walker configuration, pruning | query grammar |
| `search::matcher` | per-entry match and kind | output |
| `search::traversal` | workers, cancellation, error counting | argument parsing |
| `output::writer` | serialization, flushing, broken pipe | matching |
| `error` | typed failures, exit statuses | search strategy |

`domain` depends on no CLI, traversal or output library, so its tests run on plain values.

## The plan is fixed before the walk

`lib.rs` resolves the query, roots, visibility, result kind and output format into an immutable
`SearchPlan` before any directory is read. Nothing a walk finds can change the plan: an empty
result never turns an extension query into a hidden-name query, and scope never widens. This is
what makes `zf` predictable: the same command means the same search on every tree.

## Grammar instead of flags

Three modes cover the common intents, and the query text alone selects one (see the
[CLI reference](cli.md#query-modes)). The cost is ambiguity at the edges, settled by fixed rules
rather than guesses:

- `.config` is an extension query. Hidden names are reached with name mode or a path.
- Path components match whole, except the last, so `proj/index` does not match
  `project/index.ts`. Predictable structure was preferred over fuzzy convenience.
- One `--all` replaces separate hidden, ignore and exclusion switches.

Ambiguous edge cases were settled explicitly in [decision 0001](decisions/0001-v1-behavior-choices.md).

## Pruning before descent

Filtering runs in the walker's entry filter, so an excluded directory is never opened. The
`ignore` crate supplies ignore-file parsing and the parallel walk; every walker option is set
explicitly in `search::filtering` so a dependency default cannot change behavior unnoticed.
Hidden-entry filtering is done by `zf`, not the crate, because the crate cannot express the
exemption for hidden components named in a path query.

## One writer, bounded queue

Workers never write to stdout. They send matches into a channel of 1,024 records; one writer
drains it into a 32 KiB buffer and flushes whenever the channel is momentarily empty. A slow
reader fills the channel and blocks the workers, so memory stays at 7.5–8.5 MB from 0 to 100,000
matches. There is no sorting or collection stage, so the first match is written as soon as it is
found.

The writer owns broken-pipe handling (quiet exit 0). Ctrl+C sets a flag that stops workers at
their next entry and stops the writer before its next record, so only whole records reach stdout.

## Records travel inline

A path is copied into a fixed 254-byte record inside the channel slot. Paths longer than that use
a heap buffer. The reason is measured, not stylistic: with a heap `PathBuf` per match, the writer
freed memory the workers had allocated, and glibc's per-arena locking turned that into roughly one
context switch per match. On 100,000 matches that was 88 ms and ~100,000 context switches; inline
records brought it to 21 ms and under 2,000, with no change in memory. See
[performance](performance.md#cost-model).

## Errors are typed until the edge

Bad queries, bad roots and output failures are `ZfError` values; recoverable walk failures are
counted, not thrown. Only `lib::run` turns them into text and an exit status, so the same mapping
holds everywhere: 2 for anything that stops a search before it starts, 3 when some entries could
not be read, 130 on interrupt.

## Invariants

1. The default root is the current directory; parents are never walked.
2. Query interpretation depends on the query text only.
3. Scope never widens at runtime.
4. Match results are deterministic; output order is not.
5. Excluded directories are pruned before they are read.
6. Files and directories can both be results.
7. Results stream; they are never collected or ranked.
8. Stdout carries paths only.
9. No configuration or persistent state.
10. A performance change may not alter the match set; every benchmark row checks this first.

## Out of scope

Fuzzy matching, ranking, regex and glob queries, content search, size and time filters, custom
exclusions, output templates, sorting, an index or daemon, thread-count and depth flags, and an
interactive picker. Adding one needs evidence that the current grammar cannot express a common
need.
