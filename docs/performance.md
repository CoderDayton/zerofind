# Performance

Measured speed, memory and start-up of `zf` against `fd`, how the numbers were taken, and what
limits them.

- [Method](#method)
- [Results](#results)
- [Cold cache](#cold-cache)
- [Output-bound runs](#output-bound-runs)
- [Memory](#memory)
- [Start-up and build profile](#start-up-and-build-profile)
- [Workers](#workers)
- [Cost model](#cost-model)
- [Limits](#limits)

**On every measured tree `zf` finished first and printed its first result first: 2–4 ms to the
first result against 5.8–30 ms for `fd` 10.5.0, at 4–11 MB peak memory against 148–170 MB. With a
cold page cache `zf` finishes 14–37% sooner and its first result arrives 2–7x sooner. Writing
100,000 matches to `/dev/null`, `zf` takes 12.5 ms and `fd -j 8` 20.6 ms. All numbers come from
one Linux machine on NVMe; network mounts and other platforms are unmeasured.**

## Method

- Machine: Intel i9-13900K (32 threads), Linux 7.3 (CachyOS), btrfs on NVMe, 62 GB RAM, shared
  with other work at load 2–5. Numbers vary by about ±2 ms within a session (up to ±4 ms at load
  4–5); between sessions both tools shift together, by up to 2x on the small trees.
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
- The page cache is warm except in [Cold cache](#cold-cache). Binaries are loaded from disk once
  before timing: a binary just written by the compiler reads about 0.5 MB higher in RSS until
  then.

Reproduce:

```bash
cgs run bench ~/some/tree   # any tree: start-up, name, extension and directory queries
cgs bench-trees             # generated flat (100k matches) and deep (120 levels) trees
python3 -I benches/compare.py --cold --runs 5 ~/some/tree   # drops the cache via sudo per run
```

## Results

| Tree | Entries | Query (matches) | zf | fd | zf first | fd first | zf RSS | fd RSS |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| zerofind repo | 47 + `.git/` and ignored files | `.rs` (25) | 3.7 ms | 23.3 ms | 4.1 ms | 24.6 ms | 4.0 MB | 152 MB |
| `~/.cargo/registry/src` | 46,253 | `main` (104) | 18.4 ms | 28.5 ms | 3.0 ms | 29.7 ms | 5.0 MB | 148 MB |
| `~/.cargo/registry/src` | 46,253 | `.rs` (21,589) | 11.6 ms | 20.9 ms | 2.1 ms | 6.3 ms | 4.9 MB | 150 MB |
| `~/.rustup/toolchains` | 258,714 | `.rs` (5,854) | 14.3 ms | 34.8 ms | 2.0 ms | 6.9 ms | 11.4 MB | 170 MB |
| projects folder, mostly ignored | 844,800 | `main` (72) | 4.7 ms | 18.2 ms | 2.2 ms | 18.7 ms | 5.7 MB | 150 MB |
| generated flat | 100,100 | `m` (100,000) | 16.0 ms | 29.8 ms | 2.3 ms | 5.8 ms | 4.4 MB | 150 MB |
| generated deep, 120 levels | 5,040 | `main` (3,600) | 12.8 ms | 33.1 ms | 2.0 ms | 10.9 ms | 4.3 MB | 153 MB |

Entry counts include hidden and ignored entries; both tools prune most of the projects folder.
A second pass gave 14.9 ms against 22.8 ms for the registry `main` row.

On small result sets `fd`'s first result arrives at about its total time; on large ones (21,589
matches) it arrives early. `zf` writes each match as soon as it is found, on every tree.

## Cold cache

The page cache dropped (`echo 3 > /proc/sys/vm/drop_caches`) before every run, `zf` and `fd`
binaries included; Samsung 980 PRO / Crucial P310 NVMe, btrfs. Median of 7 alternating runs;
match sets checked as above.

| Tree, query (matches) | zf | fd | zf first | fd first | zf RSS | fd RSS |
|---|---:|---:|---:|---:|---:|---:|
| `~/.cargo/registry/src`, `main` (104) | 25.5 ms | 29.8 ms | 8.3 ms | 31.4 ms | 5.4 MB | 150 MB |
| `~/.cargo/registry/src`, `.rs` (21,589) | 25.8 ms | 33.1 ms | 7.1 ms | 14.4 ms | 4.7 MB | 148 MB |
| `~/.rustup/toolchains`, `main` (96) | 31.9 ms | 47.9 ms | 7.3 ms | 50.0 ms | 9.7 MB | 164 MB |
| `~/.rustup/toolchains`, `.rs` (5,854) | 32.9 ms | 52.4 ms | 7.4 ms | 17.1 ms | 10.0 MB | 165 MB |

Start-up in an empty directory: `zf` 4.2 ms, `fd` 19.5 ms. Reads wait on the disk, so the walk
reaches 16 workers early (see [Workers](#workers)); `fd`'s 32 threads do not make up for its
per-entry queueing.

## Output-bound runs

100,000 matches on the generated flat tree to `/dev/null`, median of 15 alternating runs timed
from Python (spawn overhead included, the same for each):

| Tool | Time |
|---|---:|
| `zf` | 12.5 ms |
| `fd -j 8` | 20.6 ms |
| `fd` (32 threads) | 24.2 ms |

Into a pipe that is read (the generated flat row above) `zf` takes 16.0 ms against 29.8 ms.

## Memory

Peak RSS on the generated flat tree is the same whatever the match count and reader speed
(`zzz`, `m1` and `m`; median of 10 runs with a fast reader and 5 with a slow one, maximum in
parentheses):

| Matches | Fast reader | Slow reader (sleeps 0.5 ms per 4 KiB) |
|---:|---:|---:|
| 0 | 4.0 MB (4.2) | n/a |
| 11,200 | 3.9 MB (4.2) | 3.9 MB (4.6) |
| 100,000 | 3.9 MB (4.3) | 4.2 MB (4.4) |

The slow reader stretches the 100,000-match run to 170 ms without raising memory.

A worker whose 16 KiB buffer is full waits for the writer, so output never holds more than two
buffers per worker plus the 32 KiB output buffer (under 600 KB at 16 workers), whatever the
result count. Memory does grow with tree size, independent of matches: the 258,714-entry
toolchain tree peaks at 10–12 MB.

## Start-up and build profile

`zf` in an empty directory takes 1.6–5.2 ms across sessions (3.7–4.2 MB peak): it reads the
directory on the calling thread and starts no workers. `fd` takes 16–33 ms with 32 threads
(146–152 MB) and 7.1 ms with 8 (49 MB).

The release profile uses fat LTO, one codegen unit and stripped symbols. Profiles run
round-robin (start-up median of 25 runs, walks median of 20; rebuild after touching `lib.rs`,
two rounds), measured with the previous fixed-size walker:

| Profile | Start-up | Walk only, 100k entries | 100k matches | Binary | Rebuild |
|---|---:|---:|---:|---:|---:|
| cargo default | 5.4 ms | 16.9 ms | 19.7 ms | 3.14 MiB | 0.6 s |
| LTO + 16 CGU + strip | 5.0 ms | 16.9 ms | 19.0 ms | 2.12 MiB | 9.3 s |
| LTO + 1 CGU + strip | 5.2 ms | 16.9 ms | 19.1 ms | 1.99 MiB | 8.4 s |

Run time is the same for all three within noise. LTO with one codegen unit gives the smallest
binary (37% smaller than the default) and rebuilds faster than with 16; its cost is an
incremental release rebuild of 8.4 s instead of 0.6 s. Debug and test builds are
unaffected.

## Workers

A walk starts on one worker and adds workers while a worker holds more queued directories than
sleeping workers can take, up to `min(cores, 16)`. Each worker walks its own subtree and steals
from others when idle ([design](design.md#workers-grow-with-the-work)). `ZF_TRACE=1` shows the
result: 2 workers on the deep tree, 6 on the zerofind repo, 16 on the flat and toolchain trees.

Against the `ignore` crate's walker it replaced, at a fixed 16 and 8 workers (median of 20
alternating warm runs, load 2–4; every build returned the same results; peak RSS in
parentheses):

| Tree (matches) | zf | fixed 16 | fixed 8 |
|---|---:|---:|---:|
| empty directory (start-up) | 1.6 ms | 3.8 ms | 3.1 ms |
| zerofind repo `.rs` (25) | 1.6 ms | 3.4 ms | 2.8 ms |
| `~/.cargo/registry/src` `.rs` (21,589) | 8.9 ms | 12.5 ms | 14.5 ms |
| `~/.rustup/toolchains` `.rs` (5,854) | 13.6 ms (12 MB) | 28.4 ms (32 MB) | 34.2 ms (23 MB) |
| projects folder `main` (72) | 4.0 ms | 6.5 ms | 7.9 ms |
| generated flat `m` (100,000) | 9.3 ms | 15.4 ms | 18.7 ms |
| generated deep `main` (3,600) | 11.6 ms | 21.5 ms | 17.6 ms |

A second pass kept the order on every row (1.4–2.8x against fixed 16). With a cold cache (median
of 15): registry 25.8 ms against 25.4 ms for fixed 16, toolchains 32.4 ms against 43.8 ms,
projects folder 11.5 ms against 11.8 ms; an earlier cold pass had fixed 8 at 37–38 ms and 60 ms
on the first two. A single shared queue instead of per-worker queues was 20% slower on the cold
registry (31 ms), because its workers took neighbouring directories and waited on the same reads.

The cap stays at 16: with the previous walker, 32 workers lost to 16 on every warm tree.

## Cost model

- **Walk**: directory reads dominate. Pruning ignored directories is the biggest saving; on the
  projects folder most of the 844,800 entries are never read.
- **Per directory**: the listing is read whole, then its ignore files (only those it lists)
  are loaded. Subdirectories go to the worker's own queue under one uncontended lock; files never
  enter a queue.
- **Per entry**: ignore-rule and visibility checks cost about 2.5 ms per 100,000 entries outside
  a git repository and 11 ms inside one (generated flat tree, rules on against `--all`; the
  replaced walker paid 7.7 ms and 14 ms). The rule check walks only the directories above the
  entry that have ignore files, and skips those whose rules cannot apply there: a project repo
  inside a dotfiles repo (`~/.gitignore` of `*`) walks 100,000 files in 9.4–11.2 ms instead of
  13.2–13.5 ms. `-E` patterns, when given, add one match per entry. `zf`'s own work is one
  comparison per query word. ASCII names
  are compared without allocating; non-ASCII names are lowercased and NFC-normalized into a
  temporary buffer. Directories with a generated-directory name cost one or two extra `stat`s
  for their marker files. Symlinks are stat'ed only when a kind filter, a `dir/` query or a
  generated-directory name needs their target's kind.
- **Per match**: one uncontended lock and a copy into the worker's own buffer, then one copy
  into the 32 KiB output buffer. Nothing is allocated or freed per match. A shared channel cost
  about 0.8 µs of CPU per match in handoff (measured with the previous walker: 100,000 matches
  outside a git repository took 20.3 ms and 137 ms of CPU, against 10.1 ms and 50 ms with
  per-worker buffers).
- **Flush**: the writer flushes whenever every worker buffer is empty, so a pipe gets large
  writes under load and single records when matches are sparse.

## Limits

- No index: every run walks the tree, so cost grows with the unpruned tree size. On a cold cache
  the disk is the bottleneck; network mounts are unmeasured.
- Workers stop at `min(cores, 16)`. A network mount, where each read waits longer, might gain
  from more; this is unmeasured.
- A tree with an ignore file at each of N levels costs up to N rule checks per entry below
  them, as it did with the `ignore` crate.
- Writing to the terminal is bounded by the terminal emulator, not by `zf`.
