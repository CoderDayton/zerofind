# 0002: own walker with growing workers

Status: accepted (2026-11-02).

## Context

`zf` walked with the `ignore` crate's parallel walker, which starts a fixed number of threads
before reading anything. Any fixed count lost somewhere: 8 workers lost 31–40% on a cold page
cache against 16, while 16 started slower and doubled the time on a deep, narrow tree. An idle
crate worker also polls with 1 ms sleeps, and every entry, file or directory, passes through its
work queue.

The crate cannot add workers mid-walk, and the code that combines ignore files across
directories is private to it.

## Decision

`zf` walks with its own scheduler (`search::walk`) and applies ignore files itself
(`search::rules`). It still uses the crate's `Gitignore` matcher for each file's patterns.

- **Growth.** A walk starts on one worker. After each directory, a worker holding more queued
  directories than sleeping workers can take adds workers for the rest, up to
  `min(cores, 16)`. A narrow tree stays on one or two workers; a wide one reaches the cap after
  its first wide directory.
- **Work stealing.** Each worker owns a queue, takes its newest job and so walks its own subtree;
  an idle worker steals another queue's oldest job. Idle workers sleep on a condition variable.
- **Rules.** The crate's semantics are kept exactly: `.ignore` everywhere; `.gitignore`,
  `.git/info/exclude` (through a worktree's `commondir`) and the global git ignore file only
  inside a `.git` or `.jj` repository, never across its top; ignore files above the root apply.
  `tests/walk_oracle.rs` compares the result set with the crate's walker on 200 generated trees
  with random rules, plus nested repositories, worktrees, symlinks and a 60-level rule stack.
  One deliberate difference: a worktree's relative `gitdir:` (`git worktree add
  --relative-paths`) resolves from the worktree, as git does; the crate resolves it from the
  working directory and so drops the shared `info/exclude` (test
  `relative_worktree_link_resolves_from_the_worktree`).
- **`ZF_TRACE=1`** prints one line per walked root to stderr: workers started, directories
  read, entries listed, sleeps and milliseconds. It never changes stdout or the exit status, and
  `--help` does not mention it.

## Why

Measured against the crate's walker capped at 16 and at 8 (median of 20 warm runs, 15 cold; see
[performance](../performance.md#workers)):

- Warm cache: 1.4–2.8x faster on every tree, including start-up (1.6 ms against 3.1–3.8 ms) and
  the deep tree (11.6 ms against 17.6–21.5 ms). Peak memory on the 258,714-entry tree fell from
  23–32 MB to 12 MB.
- Cold cache: a tie on the registry tree (25.8 ms against 25.4 ms), 26% faster on the toolchain
  tree (32.4 ms against 43.8 ms).
- Output is identical on every benchmark tree and on a 2.76-million-entry home directory, with
  and without `--all`.

Rejected on the way:

- One shared queue: 20% slower on the cold registry tree (31 ms against 25 ms). Its workers took
  neighbouring directories and waited on the same metadata reads.
- Workers that keep their jobs private and hand them out only between directories: idle
  workers starved (1,952 sleeps, 70 ms on a warm 15 ms walk).
- Adding one worker per finished directory: on a cold disk, each added worker cost one slow
  read before the next.

## Costs

- `zf` owns about 480 lines of ignore-rule and scheduling code, comments included, that the
  crate provided. The comparison test fails if the two ever disagree.
- An unreadable `.git` file in a folder above the root now warns instead of counting toward exit
  status 3, like every other ignore-file problem (decision 0001 #11).
- `ZF_TRACE` is an undocumented-in-`--help` interface; it is described in
  [the CLI reference](../cli.md#exit-status-and-diagnostics).
