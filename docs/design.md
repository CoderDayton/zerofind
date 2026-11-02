# Design

How `zf` is built, and the reason behind each choice.

- [Pipeline](#pipeline)
- [The plan is fixed before the walk](#the-plan-is-fixed-before-the-walk)
- [Grammar instead of flags](#grammar-instead-of-flags)
- [Pruning before descent](#pruning-before-descent)
- [One writer, per-worker buffers](#one-writer-per-worker-buffers)
- [Errors are typed until the edge](#errors-are-typed-until-the-edge)
- [Invariants](#invariants)
- [Out of scope](#out-of-scope)

## Pipeline

```text
argv -> cli -> SearchPlan -> traversal: N workers filter + match -> per-worker buffers -> writer -> stdout
```

| Module | Owns | Never touches |
|---|---|---|
| `cli` | argument parsing, help, dash-word handling | search logic |
| `domain::query` | word compilation, name, glob and path matching, what a query names | the filesystem |
| `domain::scope` | locations, root resolution, `~`, root dedupe | output formatting |
| `domain::policy` | hidden, generated-directory and result-kind decisions | scheduling |
| `search::filtering` | walker configuration, pruning | query grammar |
| `search::matcher` | per-entry match and kind | output |
| `search::traversal` | workers, cancellation, error counting | argument parsing |
| `output::sink` | per-worker buffers, serialization, flushing, broken pipe | matching |
| `error` | typed failures, exit statuses | search strategy |

`domain` depends on no CLI, traversal or output library, so its tests run on plain values.

## The plan is fixed before the walk

`lib.rs` splits the arguments into locations and query words, then resolves words, roots,
visibility, result kind and output format into an immutable `SearchPlan` before any directory is
read. Nothing a walk finds can change the plan: an empty result never switches a word's mode, and
scope never widens. This is what makes `zf` predictable: the same command means the same search
on every tree.

## Grammar instead of flags

Four modes cover the common intents, and each word's text alone selects one (see the
[CLI reference](cli.md#query-modes)). Where a user's spelling is unambiguous, `zf` follows it
instead of asking for a flag:

- Several words mean all of them, as in a search box.
- A location in the arguments (`~/Downloads/report`, `..`, `/etc`) is where to search, so `-p`
  is rarely needed.
- `.env` means the file `.env` as well as the extension, and spelling a hidden or generated name
  reaches it. `--all` is left for browsing what the defaults hide.
- `main.rs` reads as a file name and starts at a word boundary; plain words stay substrings.
- Path components match whole, except the last, so `proj/index` does not match
  `project/index.ts`. Predictable structure was preferred over fuzzy convenience.

Ambiguous edge cases were settled explicitly in [decision 0001](decisions/0001-v1-behavior-choices.md).

## Pruning before descent

Filtering runs in the walker's entry filter, so an excluded directory is never opened. The
`ignore` crate supplies ignore-file parsing and the parallel walk; every walker option is set
explicitly in `search::filtering` so a dependency default cannot change behavior unnoticed.
Hidden and generated-directory filtering is done by `zf`, not the crate, because the crate cannot
express the exemption for names the query spells.

Generated directories are recognized by a marker beside or inside them (`Cargo.toml` beside
`target`, `pyvenv.cfg` inside a venv), not by name alone, so personal folders called `build` or
`dist` stay searchable. The check stats one or two files, and only for directories with a listed
name, so ordinary entries pay nothing. A directory admitted only because a word spells its name
is printed and then skipped (`WalkState::Skip`): `zf node_modules` does not walk every package.

## One writer, per-worker buffers

Workers never write to stdout. Each formats its matches into its own 16 KiB buffer; one writer
swaps a filled buffer for an empty one, copies it into a 32 KiB output buffer, and flushes
whenever every buffer is empty. A worker whose buffer is full waits for the writer, so a slow
reader blocks the walk and memory stays at 6.5–7.7 MB from 0 to 100,000 matches. There is no
sorting or collection stage, so the first match is written as soon as it is found.

The reason is measured: a shared channel cost about 0.8 µs of CPU per match in handoff alone.
With per-worker buffers a match costs one uncontended lock and a copy, nothing is allocated or
freed across threads, and 100,000 matches outside a git repository take 10.1 ms instead of
20.3 ms (137 ms of CPU down to 50 ms). See [performance](performance.md#cost-model).

The writer owns broken-pipe handling (quiet exit 0). Ctrl+C sets a flag that stops workers at
their next entry and stops the writer before its next record, so only whole records reach stdout.
A worker blocked in a slow `readdir` or `stat` never sees that flag, so a second Ctrl+C exits at
once. The Ctrl+C flag is the only process-wide state; each search has its own stop flag.

## Errors are typed until the edge

Bad queries, bad roots and output failures are `ZfError` values; recoverable walk failures are
counted, not thrown. Only `lib::run` turns them into text and an exit status, so the same mapping
holds everywhere: 2 for anything that stops a search before it starts, 3 when some entries could
not be read, 130 on interrupt.

All stderr output goes through `output::diag`, which ignores write failures: a closed stderr
must not crash a worker or change the result.

## Invariants

1. The default root is the current directory; parents are never walked unless a location names
   them.
2. Query interpretation depends on the arguments only; a location's existence decides only where
   its root ends.
3. Scope never widens at runtime.
4. Match results are deterministic; output order is not.
5. Excluded directories are pruned before they are read.
6. Files and directories can both be results.
7. Results stream; they are never collected or ranked.
8. Stdout carries paths only.
9. No configuration or persistent state.
10. A performance change may not alter the match set; every benchmark row checks this first.

## Out of scope

Fuzzy matching, ranking, regex and bracket globs, content search, size and time filters, custom
exclusions, output templates, sorting, an index or daemon, thread-count and depth flags, and an
interactive picker. Adding one needs evidence that the current grammar cannot express a common
need.
