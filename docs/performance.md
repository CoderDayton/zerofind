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
first result against 5.4–34 ms for `fd` 10.5.0, at 4–26 MB peak memory against 150–177 MB. The
exception is output-bound work: writing 100,000 matches to `/dev/null`, `fd -j 8` took 19.8 ms
and `zf` 22.1 ms. All numbers come from one Linux machine with a warm page cache; cold-disk and
other platforms are unmeasured.**

## Method

- Machine: Intel i9-13900K (32 threads), Linux 7.3 (CachyOS), btrfs, 62 GB RAM, shared with other
  work at load 2–7. Numbers vary by about ±2 ms within a session; between sessions both tools
  shift together, by up to 2x on the small trees.
- Versions: `zf` release build (LTO, one codegen unit), `fd` 10.5.0, rustc 1.99.1.
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
| zerofind repo | 51 + `.git/` and ignored `target/` | `.rs` (22) | 8.2 ms | 33.6 ms | 7.8 ms | 34.3 ms | 4.4 MB | 152 MB |
| `~/.cargo/registry/src` | 46,253 | `main` (104) | 14.4 ms | 20.2 ms | 2.4 ms | 20.4 ms | 6.8 MB | 150 MB |
| `~/.cargo/registry/src` | 46,253 | `.rs` (21,589) | 15.9 ms | 19.5 ms | 2.4 ms | 5.6 ms | 6.7 MB | 150 MB |
| `~/.rustup/toolchains` | 258,714 | `.rs` (5,854) | 31.5 ms | 39.3 ms | 2.2 ms | 7.9 ms | 26.2 MB | 177 MB |
| projects folder, mostly ignored | 844,800 | `main` (72) | 14.1 ms | 22.9 ms | 4.0 ms | 23.0 ms | 6.5 MB | 150 MB |
| generated flat | 100,100 | `m` (100,000) | 21.5 ms | 27.5 ms | 2.9 ms | 5.4 ms | 7.8 MB | 150 MB |
| generated deep, 120 levels | 5,040 | `main` (3,600) | 24.0 ms | 37.4 ms | 3.1 ms | 12.2 ms | 6.6 MB | 152 MB |

Entry counts include hidden and ignored entries; both tools prune most of the projects folder.
On the small repo both tools spend most of the time starting.

On small result sets `fd`'s first result arrives at about its total time; on large ones (21,589
matches) it arrives early. `zf` writes each match as soon as it is found, on every tree.

With `fd -j 8` (the same worker count as `zf`) on the projects folder: `zf` 9.6 ms, `fd` 12.0 ms,
`fd` peak memory 52 MB instead of 150 MB.

## Output-bound runs

100,000 matches on the generated flat tree to `/dev/null`, median of 15 alternating runs timed
from Python (spawn overhead included, the same for each):

| Tool | Time |
|---|---:|
| `fd -j 8` | 19.8 ms |
| `zf` | 22.1 ms |
| `fd` (32 threads) | 22.4 ms |

Here the cost is per-record work, not the walk; `fd -j 8` is ahead by about 10%. Into a pipe
that is read (the generated flat row above) `zf` is ahead, 21.5 ms vs 27.5 ms.

## Memory

Peak RSS on the generated flat tree is the same whatever the match count and reader speed
(`zzz`, `m1` and `m`; median of 10 runs with a fast reader and 5 with a slow one, maximum in
parentheses):

| Matches | Fast reader | Slow reader (sleeps 0.5 ms per 4 KiB) |
|---:|---:|---:|
| 0 | 7.1 MB (7.6) | n/a |
| 11,200 | 7.2 MB (7.6) | 7.0 MB (7.4) |
| 100,000 | 7.1 MB (7.7) | 6.8 MB (7.4) |

The slow reader stretches the 100,000-match run to 186 ms without raising memory.

The bounded queue blocks workers when the reader falls behind, so memory tracks the queue size,
not the result count. Memory does grow with tree size, independent of matches: the 258,714-entry
toolchain tree peaks at 23–28 MB.

## Start-up and build profile

`zf` in an empty directory takes 2.6–7.7 ms across sessions (4.3–4.5 MB peak); `fd` 15–33 ms
with 32 threads and about 7 ms with 8 (54 MB).

The release profile uses fat LTO, one codegen unit and stripped symbols. Two alternating rounds
per profile (start-up median of 25 runs, walks median of 15; ranges span the rounds):

| Profile | Start-up | Walk only, 100k entries | 100k matches | Binary |
|---|---:|---:|---:|---:|
| cargo default | 4.9–6.8 ms | 17.0–17.6 ms | 21.8–21.9 ms | 3.19 MiB |
| LTO + 1 CGU + strip | 6.1–6.5 ms | 16.9–17.1 ms | 21.2–21.6 ms | 2.02 MiB |

The start-up difference is within run-to-run noise; the walks gain under 1 ms. The clear gain is
a 37% smaller binary. The cost is build time: an incremental release rebuild goes from
0.55–0.67 s to 8.6 s. Debug and test builds are unaffected.

## Worker count

`zf` walks with `min(cores, 8)` workers. Builds capped at 2, 4, 8, 16 and 32 workers, run
round-robin for 15 rounds per tree (median wall time; every build returned the same results):

| Tree (matches) | 2 | 4 | 8 | 16 | 32 |
|---|---:|---:|---:|---:|---:|
| empty directory (start-up) | 5.4 ms | 5.5 ms | 5.5 ms | 6.8 ms | 7.9 ms |
| zerofind repo `.rs` (22) | 8.4 ms | 7.6 ms | 8.2 ms | 8.3 ms | 10.3 ms |
| `~/.cargo/registry/src` `.rs` (21,589) | 35.2 ms | 22.0 ms | 14.8 ms | 12.7 ms | 31.3 ms |
| `~/.rustup/toolchains` `.rs` (5,854) | 71.8 ms | 47.8 ms | 33.0 ms | 30.1 ms | 35.0 ms |
| projects folder `main` (72) | 24.0 ms | 16.5 ms | 11.0 ms | 9.5 ms | 11.6 ms |
| generated flat `m` (100,000) | 47.8 ms | 30.0 ms | 21.4 ms | 26.5 ms | 101.1 ms |
| generated deep `main` (3,600) | 33.1 ms | 27.9 ms | 25.0 ms | 32.4 ms | 42.3 ms |

Below 8, the large trees slow down 1.4–1.5x at 4 workers and 2.2–2.4x at 2. 16 workers gain 1.5–2.9 ms on the three large
walks but lose 5 ms on 100,000 matches and 7 ms on the deep tree, start 1.3 ms slower, and peak
at 33 MB instead of 26 MB on the toolchain tree. 32 workers contend on the single output queue
and lose everywhere, up to 101 ms on 100,000 matches. Peak memory grows with workers (13, 18,
26, 33 and 47 MB on the toolchain tree). Time to the first result is 2–6 ms at every count.

Measured on the 32-thread machine with a warm cache. A machine with 8 or fewer cores uses all of
them. A cold disk or network mount, where workers mostly wait on reads, might favor more; this is
unmeasured.

## Cost model

- **Walk**: directory reads dominate. Pruning ignored directories is the biggest saving; on the
  projects folder most of the 844,800 entries are never read.
- **Per entry**: one comparison per query word. ASCII names are compared without allocating;
  non-ASCII names are lowercased and NFC-normalized into a temporary buffer. Directories with a
  generated-directory name cost one or two extra `stat`s for their marker files. Symlinks are
  stat'ed only when a kind filter, a `dir/` query or a generated-directory name needs their
  target's kind.
- **Per match**: one copy into a 254-byte channel record and one copy into the 32 KiB output
  buffer. Paths up to 254 bytes cross threads with no heap memory. With a heap path per match,
  glibc's cross-thread free cost about 0.7 µs and one context switch per match: 88 ms and
  ~100,000 context switches for 100,000 matches, against 21 ms and under 2,000 now.
- **Flush**: the writer flushes whenever the queue is empty, so a pipe sees 26–32 KB writes under
  load and single records when matches are sparse.

## Limits

- No index: every run walks the tree, so cost grows with the unpruned tree size. A cold disk or a
  network mount makes the walk, not `zf`, the bottleneck; this is unmeasured.
- Worker count is fixed at `min(cores, 8)`, the best overall count in the
  [sweep](#worker-count); large walks would gain 9–14% from 16 workers, at a cost on
  result-heavy and deep trees.
- Writing to the terminal is bounded by the terminal emulator, not by `zf`.
