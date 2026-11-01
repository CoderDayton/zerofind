# Performance

Measured speed, memory and start-up of `zf` against `fd`, how the numbers were taken, and what
limits them.

- [Method](#method)
- [Results](#results)
- [Output-bound runs](#output-bound-runs)
- [Memory](#memory)
- [Start-up and build profile](#start-up-and-build-profile)
- [Cost model](#cost-model)
- [Limits](#limits)

**On every measured tree `zf` finished first and printed its first result first: 2–5 ms to the
first result against 5.6–22 ms for `fd` 10.5.0, at 4–26 MB peak memory against 148–172 MB. The
exception is output-bound work: writing 100,000 matches to `/dev/null`, `fd` took 18.9 ms and
`zf` 20.9 ms. All numbers come from one Linux machine with a warm page cache; cold-disk and other
platforms are unmeasured.**

## Method

- Machine: Intel i9-13900K (32 threads), Linux 7.3 (CachyOS), btrfs, 62 GB RAM, shared with other
  work at load 4–7. Numbers vary by about ±2 ms between runs.
- Versions: `zf` release build (LTO, one codegen unit), `fd` 10.5.0, rustc 1.99.1.
- `benches/compare.py` runs each query once per tool and compares the sorted result sets
  (`fd`'s trailing `/` on directories stripped). A row is timed only if the sets are identical.
- Then 10 rounds alternate `zf` and `fd`; the table shows medians.
- `benches/peakrss.c` launches each run and reports wall time and peak RSS via `wait4`. Launching
  from Python directly would floor every RSS reading at the Python process's ~14 MB.
- Time to first result is measured from the harness's spawn to the first byte on the pipe.
- `fd` runs with `--no-require-git -i` and `-E` for `zf`'s excluded directories, so both apply the
  same filtering; it keeps its default thread count (32) unless noted.
- The page cache is warm; dropping it needs root.

Reproduce:

```bash
cgs run bench ~/some/tree   # any tree: start-up, name, extension and directory queries
cgs bench-trees             # generated flat (100k matches) and deep (120 levels) trees
```

## Results

| Tree | Entries | Query (matches) | zf | fd | zf first | fd first | zf RSS | fd RSS |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| zerofind repo | 42 + ignored `target/` | `.rs` (21) | 2.9 ms | 17.4 ms | 2.2 ms | 17.7 ms | 4.2 MB | 148 MB |
| `~/.cargo/registry/src` | 45,885 | `main` (101) | 15.0 ms | 19.4 ms | 2.8 ms | 19.5 ms | 6.7 MB | 150 MB |
| `~/.cargo/registry/src` | 45,885 | `.rs` (21,374) | 17.1 ms | 20.5 ms | 2.7 ms | 5.8 ms | 6.5 MB | 150 MB |
| `~/.rustup/toolchains` | 258,714 | `.rs` (5,832) | 35.8 ms | 44.0 ms | 2.3 ms | 8.2 ms | 25.5 MB | 172 MB |
| projects folder, mostly ignored | 844,800 | `main` (72) | 16.8 ms | 21.6 ms | 4.6 ms | 22.0 ms | 6.2 MB | 150 MB |
| generated flat | 100,100 | `m` (100,000) | 23.2 ms | 25.6 ms | 3.0 ms | 5.6 ms | 7.5 MB | 150 MB |
| generated deep, 120 levels | 5,040 | `main` (3,600) | 13.3 ms | 23.1 ms | 2.9 ms | 6.6 ms | 6.5 MB | 151 MB |

Entry counts include hidden and ignored entries; both tools prune most of the projects folder.

On small result sets `fd`'s first result arrives at about its total time; on large ones (21,374
matches) it arrives early. `zf` writes each match as soon as it is found, on every tree.

With `fd -j 8` (the same worker count as `zf`) on the projects folder: `zf` 7.9 ms, `fd` 10.3 ms,
`fd` peak memory 50 MB instead of 150 MB.

## Output-bound runs

100,000 matches to `/dev/null`, median of 15:

| Tool | Time |
|---|---:|
| `fd` (32 threads) | 18.9 ms |
| `fd -j 8` | 20.2 ms |
| `zf` | 20.9 ms |

Here the cost is per-record work, not the walk; `fd` is ahead by about 10%. Into a pipe that is
read (the generated flat row above) `zf` is ahead, 23.2 ms vs 25.6 ms.

## Memory

Peak RSS on the generated flat tree is the same whatever the match count and reader speed:

| Matches | Fast reader | Slow reader (sleeps 0.5 ms per 4 KiB) |
|---:|---:|---:|
| 0 | 8.2 MB | n/a |
| 11,200 | 7.9 MB | 7.6 MB |
| 100,000 | 7.7 MB | 7.8 MB |

The bounded queue blocks workers when the reader falls behind, so memory tracks the queue size,
not the result count. Memory does grow with tree size, independent of matches: the 258,714-entry
toolchain tree peaks at 23–28 MB.

## Start-up and build profile

`zf` in an empty directory takes 2.6–6.9 ms across runs (4.3 MB peak); `fd` 15–32 ms with 32
threads and about 7 ms with 8 (54 MB).

The release profile uses fat LTO, one codegen unit and stripped symbols:

| Profile | Start-up | Walk only, 100k entries | 100k matches | Binary |
|---|---:|---:|---:|---:|
| cargo default | 6.8 ms | 15.8–16.0 ms | 21.3–21.6 ms | 3.04 MB |
| LTO + 1 CGU + strip | 4.4–4.5 ms | 14.5–15.1 ms | 21.0–21.3 ms | 1.89 MB |

The cost is build time: an incremental release rebuild goes from 0.6 s to 8.5 s. Debug and test
builds are unaffected.

## Cost model

- **Walk**: directory reads dominate. Pruning ignored directories is the biggest saving; on the
  projects folder most of the 844,800 entries are never read.
- **Per entry**: one name comparison. ASCII names are compared without allocating; non-ASCII names
  are lowercased into a temporary buffer. Symlinks are stat'ed only when a kind filter, a `dir/`
  query or an excluded name needs their target's kind.
- **Per match**: one copy into a 254-byte channel record and one copy into the 32 KiB output
  buffer. Paths up to 254 bytes cross threads with no heap memory. With a heap path per match,
  glibc's cross-thread free cost about 0.7 µs and one context switch per match: 88 ms and
  ~100,000 context switches for 100,000 matches, against 21 ms and under 2,000 now.
- **Flush**: the writer flushes whenever the queue is empty, so a pipe sees 26–32 KB writes under
  load and single records when matches are sparse.

## Limits

- No index: every run walks the tree, so cost grows with the unpruned tree size. A cold disk or a
  network mount makes the walk, not `zf`, the bottleneck; this is unmeasured.
- Worker count is fixed at `min(cores, 8)`. More threads helped `fd` on output-bound runs but cost
  it 100 MB of memory and 8–25 ms of start-up.
- Writing to the terminal is bounded by the terminal emulator, not by `zf`.
