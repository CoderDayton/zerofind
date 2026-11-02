# Performance

Measured speed, memory and start-up of `zf` against `fd`, how the numbers were taken, and what
limits them.

- [Method](#method)
- [Results](#results)
- [Output-bound runs](#output-bound-runs)
- [Memory](#memory)
- [Start-up and build profile](#start-up-and-build-profile)
- [Worker count](#worker-count)
- [Cost model](#cost-model)
- [Limits](#limits)

**On every measured tree `zf` finished first and printed its first result first: 2–8 ms to the
first result against 5.4–33 ms for `fd` 10.5.0, at 4–26 MB peak memory against 148–173 MB.
Writing 100,000 matches to `/dev/null`, `zf` and `fd -j 8` tie within noise. All numbers come
from one Linux machine with a warm page cache; cold-disk and other platforms are unmeasured.**

## Method

- Machine: Intel i9-13900K (32 threads), Linux 7.3 (CachyOS), btrfs, 62 GB RAM, shared with other
  work at load 2–7. Numbers vary by about ±2 ms within a session; between sessions both tools
  shift together, by up to 2x on the small trees.
- Versions: `zf` release build (fat LTO, one codegen unit), `fd` 10.5.0, rustc 1.99.1.
- `benches/compare.py` runs each query once per tool and compares the sorted result sets
  (`fd`'s trailing `/` on directories stripped). A row is timed only if the sets are identical.
- Then 20 rounds alternate `zf` and `fd`; the table shows medians from one pass.
- `benches/peakrss.c` launches each run and reports wall time and peak RSS via `wait4`. Launching
  from Python directly would floor every RSS reading at the Python process's ~14 MB.
- Time to first result is measured from the harness's spawn to the first byte on the pipe.
- `fd` runs with `-i` and one anchored `-E` per directory `zf` skips as generated, found by
  walking the tree with the same marker rules first, so both apply the same filtering; it keeps
  its default thread count (32) unless noted.
- The page cache is warm; dropping it needs root.

Reproduce:

```bash
cgs run bench ~/some/tree   # any tree: start-up, name, extension and directory queries
cgs bench-trees             # generated flat (100k matches) and deep (120 levels) trees
```

## Results

| Tree | Entries | Query (matches) | zf | fd | zf first | fd first | zf RSS | fd RSS |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| zerofind repo | 50 + `.git/` and ignored `target/` | `.rs` (21) | 7.7 ms | 32.6 ms | 7.6 ms | 33.4 ms | 4.2 MB | 148 MB |
| `~/.cargo/registry/src` | 46,253 | `main` (104) | 14.4 ms | 18.2 ms | 2.3 ms | 18.4 ms | 7.0 MB | 148 MB |
| `~/.cargo/registry/src` | 46,253 | `.rs` (21,589) | 15.8 ms | 18.3 ms | 2.3 ms | 6.0 ms | 6.7 MB | 150 MB |
| `~/.rustup/toolchains` | 258,714 | `.rs` (5,854) | 32.1 ms | 38.6 ms | 2.2 ms | 9.0 ms | 26.3 MB | 173 MB |
| projects folder, mostly ignored | 844,800 | `main` (72) | 16.7 ms | 24.7 ms | 5.2 ms | 24.4 ms | 6.5 MB | 150 MB |
| generated flat | 100,100 | `m` (100,000) | 18.4 ms | 25.0 ms | 2.9 ms | 5.4 ms | 7.5 MB | 150 MB |
| generated deep, 120 levels | 5,040 | `main` (3,600) | 26.3 ms | 41.3 ms | 3.0 ms | 15.0 ms | 6.2 MB | 151 MB |

Entry counts include hidden and ignored entries; both tools prune most of the projects folder.
On the small repo both tools spend most of the time starting.

On small result sets `fd`'s first result arrives at about its total time; on large ones (21,589
matches) it arrives early. `zf` writes each match as soon as it is found, on every tree.

With `fd -j 8` (the same worker count as `zf`) on the projects folder: `zf` 9.6 ms, `fd` 11.7 ms,
`fd` peak memory 52 MB instead of 150 MB.

## Output-bound runs

100,000 matches on the generated flat tree to `/dev/null`, median of 15 alternating runs timed
from Python (spawn overhead included, the same for each):

| Tool | Time |
|---|---:|
| `zf` | 18.8 ms |
| `fd -j 8` | 19.8 ms |
| `fd` (32 threads) | 25.4 ms |

A second pass gave 19.4 ms for both `zf` and `fd -j 8`, so the two tie within noise. Into a pipe
that is read (the generated flat row above) `zf` is ahead, 18.4 ms vs 25.0 ms.

## Memory

Peak RSS on the generated flat tree is the same whatever the match count and reader speed
(`zzz`, `m1` and `m`; median of 10 runs with a fast reader and 5 with a slow one, maximum in
parentheses):

| Matches | Fast reader | Slow reader (sleeps 0.5 ms per 4 KiB) |
|---:|---:|---:|
| 0 | 7.1 MB (7.7) | n/a |
| 11,200 | 6.8 MB (7.5) | 6.7 MB (7.2) |
| 100,000 | 6.7 MB (7.1) | 6.5 MB (7.3) |

The slow reader stretches the 100,000-match run to 178 ms without raising memory.

A worker whose 16 KiB buffer is full waits for the writer, so output never holds more than two
buffers per worker plus the 32 KiB output buffer (under 300 KB at 8 workers), whatever the
result count. Memory does grow with tree size, independent of matches: the 258,714-entry
toolchain tree peaks at 23–28 MB.

## Start-up and build profile

`zf` in an empty directory takes 2.6–7.7 ms across sessions (4.3–4.5 MB peak); `fd` 15–33 ms
with 32 threads and about 7 ms with 8 (54 MB).

The release profile uses fat LTO, one codegen unit and stripped symbols. Profiles run
round-robin (start-up median of 25 runs, walks median of 20; rebuild after touching `lib.rs`,
two rounds):

| Profile | Start-up | Walk only, 100k entries | 100k matches | Binary | Rebuild |
|---|---:|---:|---:|---:|---:|
| cargo default | 5.4 ms | 16.9 ms | 19.7 ms | 3.14 MiB | 0.6 s |
| LTO + 16 CGU + strip | 5.0 ms | 16.9 ms | 19.0 ms | 2.12 MiB | 9.3 s |
| LTO + 1 CGU + strip | 5.2 ms | 16.9 ms | 19.1 ms | 1.99 MiB | 8.4 s |

Run time is the same for all three within noise. LTO with one codegen unit gives the smallest
binary (37% smaller than the default) and rebuilds faster than with 16; its cost is an
incremental release rebuild of 8.4 s instead of 0.6 s. Debug and test builds are
unaffected.

## Worker count

`zf` walks with `min(cores, 8)` workers. Builds capped at 2, 4, 8, 16 and 32 workers, run
round-robin for 15 rounds per tree at load 3–5 (median wall time; every build returned the same
results):

| Tree (matches) | 2 | 4 | 8 | 16 | 32 |
|---|---:|---:|---:|---:|---:|
| empty directory (start-up) | 2.6 ms | 2.4 ms | 3.4 ms | 4.6 ms | 5.2 ms |
| zerofind repo `.rs` (21) | 8.0 ms | 7.7 ms | 7.5 ms | 8.3 ms | 9.7 ms |
| `~/.cargo/registry/src` `.rs` (21,589) | 33.0 ms | 21.3 ms | 14.5 ms | 11.7 ms | 20.6 ms |
| `~/.rustup/toolchains` `.rs` (5,854) | 67.9 ms | 47.3 ms | 32.7 ms | 29.3 ms | 37.2 ms |
| projects folder `main` (72) | 23.0 ms | 12.9 ms | 8.5 ms | 7.5 ms | 10.0 ms |
| generated flat `m` (100,000) | 44.9 ms | 28.1 ms | 18.0 ms | 18.1 ms | 22.1 ms |
| generated deep `main` (3,600) | 29.4 ms | 20.7 ms | 26.2 ms | 45.9 ms | 48.2 ms |

Below 8, the large trees slow down 1.5–1.6x at 4 workers and 2.1–2.7x at 2. 16 workers gain
1.0–3.4 ms on the three large walks and tie on 100,000 matches, but lose 20 ms on the deep tree,
start 1.2 ms slower, and peak at 33 MB instead of 25 MB on the toolchain tree. 32 workers lose on
every tree. Peak memory grows with workers (14, 18, 25, 33 and 37 MB on the toolchain tree).
Time to the first result is 2–6 ms at every count.

Measured on the 32-thread machine with a warm cache. A machine with 8 or fewer cores uses all of
them. A cold disk or network mount, where workers mostly wait on reads, might favor more; this is
unmeasured.

## Cost model

- **Walk**: directory reads dominate. Pruning ignored directories is the biggest saving; on the
  projects folder most of the 844,800 entries are never read.
- **Per entry**: the walker's ignore-rule checks cost about 3 ms per 100,000 entries outside a
  git repository and about 5 ms more inside one (measured with rule handling stubbed out); they
  run inside the `ignore` crate. `zf`'s own work is one comparison per query word. ASCII names are compared without allocating;
  non-ASCII names are lowercased and NFC-normalized into a temporary buffer. Directories with a
  generated-directory name cost one or two extra `stat`s for their marker files. Symlinks are
  stat'ed only when a kind filter, a `dir/` query or a generated-directory name needs their
  target's kind.
- **Per match**: one uncontended lock and a copy into the worker's own buffer, then one copy
  into the 32 KiB output buffer. Nothing is allocated or freed per match. A shared channel cost
  about 0.8 µs of CPU per match in handoff: on 100,000 matches outside a git repository,
  20.3 ms and 137 ms of CPU against 10.1 ms and 50 ms now.
- **Flush**: the writer flushes whenever every worker buffer is empty, so a pipe gets large
  writes under load and single records when matches are sparse.

## Limits

- No index: every run walks the tree, so cost grows with the unpruned tree size. A cold disk or a
  network mount makes the walk, not `zf`, the bottleneck; this is unmeasured.
- Worker count is fixed at `min(cores, 8)`, the best overall count in the
  [sweep](#worker-count); large walks would gain 10–19% from 16 workers, at a cost on deep
  trees and memory.
- Writing to the terminal is bounded by the terminal emulator, not by `zf`.
