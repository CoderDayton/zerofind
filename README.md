<div align="center">

# zerofind

Find files by name, extension, or path without learning flags.

![Rust 2024](https://img.shields.io/badge/rust-2024%20edition-orange)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)

</div>

`zf` is a non-interactive filesystem search. The query text decides how it matches: `index` is a
name fragment, `.rs` is an extension, `src/main` is a path. It searches the current directory by
default and streams plain paths to stdout.

A parallel walk (up to 8 workers) feeds a 1,024-slot queue that one writer drains, with no sorting or
buffering stage. On an i9-13900K with a warm page cache, the first result arrives in 2–5 ms across
every benchmark tree, against 5.6–22 ms for `fd` 10.5.0, at 4–26 MB peak memory against
148–172 MB.

It is not a general query language: no regex, globs, size or time filters, or exec. When the
output itself is the bottleneck (100,000 matches to `/dev/null`) `fd` is about 10% faster. See
[When not to use this](#when-not-to-use-this).

```console
$ zf .rs
src/main.rs
src/search/traversal.rs
tests/query_contract.rs
```

## Features

- **First result in 2–5 ms** on every benchmark tree, up to 845,000 entries; results are never
  collected or sorted. [Design](docs/design.md) explains the pipeline.
- **Memory does not grow with matches**: peak RSS is 7.5–7.7 MB with 100,000 matches and 8.2 MB
  with none on the same tree, and 7.6–7.8 MB when the reader is slow.
- **Fixed query grammar**: the mode comes from the query text before the walk and never changes
  because nothing matched.
- **Conservative defaults**: hidden entries, `.gitignore`/`.ignore` rules (also outside git
  repositories and from parent directories) and 8 build/VCS directory names are skipped and
  pruned before descent. `--all` lifts all of them.
- **Stays in bounds**: never follows a symlinked directory, never climbs above the search root,
  never reads file contents.
- **Pipe-safe**: stdout carries paths only; `-0` uses NUL separators; a closed pipe exits 0
  quietly; Ctrl+C exits 130 with only whole paths written.
- **Small**: 1.9 MB stripped binary, about 3–7 ms to start in an empty directory.

## Usage

```bash
cargo install --path .
```

```bash
zf index                 # names containing "index", case-insensitive
zf .pdf --home           # Documents, Desktop, Downloads only
zf hosts -p /etc         # an explicit root
zf ./src/main            # path anchored at the search root
zf .rs | xargs wc -l     # results are plain paths
```

| Query | Mode | Matches | Does not match |
|---|---|---|---|
| `config` | name substring | `app-config.toml`, `Config.json` | `settings.yaml` |
| `.rs` | exact final extension | `main.rs` | `main.rs.bak`, `.rs` |
| `src/lib` | path: whole components, last one by prefix | `src/lib.rs`, `app/src/library.rs` | `src/old-lib.rs` |
| `./src/main` | path anchored at the root | `src/main.rs` | `pkg/src/main.rs` |
| `src/` | directory named exactly `src` | `src`, `app/src` | `src/main.rs`, `srcs` |

| Option | Effect |
|---|---|
| `--home` | Search the platform's Documents, Desktop and Downloads folders |
| `-p, --path DIR` | Search `DIR` (or evaluate one file); excludes `--home` |
| `--all` | Include hidden, ignored and built-in excluded entries |
| `--files` / `--dirs` | Emit only files or only directories; traversal is unchanged |
| `-0, --print0` | NUL-separated output |

Exit status is 0 with matches, 1 with none, 2 for a bad query or root, 3 when some directories
could not be read (other matches still print), 130 on Ctrl+C. Result order is unspecified; pipe
through `sort` when it matters. Every rule, edge case and diagnostic is in the
[CLI reference](docs/cli.md).

## When not to use this

| Situation | Use instead | Why |
|---|---|---|
| Regex, glob, size, time or owner filters | `fd`, `find` | `zf` has three query modes and no filter expressions by design |
| Running a command per result | `fd -x`, `find -exec` | No exec; pipe to `xargs -0` with `-0` |
| Repeated whole-disk lookups | `plocate` | `zf` keeps no index; each run walks the tree |
| Output-bound runs (100k+ matches to `/dev/null`) | `fd` | `fd` measured 18.9 ms vs 20.9 ms for `zf` |
| Sorted or ranked output | `zf ... \| sort`, `fzf` | Matching is binary and order is unspecified |
| Windows or macOS | `fd` | Not yet run there: the Windows build type-checks and CI covers both, but tests and benchmarks have only run on Linux |

## Benchmarks

| Tree | Entries | Query (matches) | zf | fd | zf first | fd first | zf RSS | fd RSS |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| zerofind repo | 42 + ignored `target/` | `.rs` (21) | 2.9 ms | 17.4 ms | 2.2 ms | 17.7 ms | 4.2 MB | 148 MB |
| `~/.cargo/registry/src` | 45,885 | `.rs` (21,374) | 17.1 ms | 20.5 ms | 2.7 ms | 5.8 ms | 6.5 MB | 150 MB |
| `~/.rustup/toolchains` | 258,714 | `.rs` (5,832) | 35.8 ms | 44.0 ms | 2.3 ms | 8.2 ms | 25.5 MB | 172 MB |
| projects folder, mostly ignored | 844,800 | `main` (72) | 16.8 ms | 21.6 ms | 4.6 ms | 22.0 ms | 6.2 MB | 150 MB |
| generated flat | 100,100 | `m` (100,000) | 23.2 ms | 25.6 ms | 3.0 ms | 5.6 ms | 7.5 MB | 150 MB |
| generated deep, 120 levels | 5,040 | `main` (3,600) | 13.3 ms | 23.1 ms | 2.9 ms | 6.6 ms | 6.5 MB | 151 MB |

Median of 10 alternating runs per tool, i9-13900K (32 threads), Linux 7.3, btrfs, warm page cache,
machine load 4–7; entry counts include hidden and ignored entries; `fd` with its default 32 threads and flags that reproduce `zf`'s filtering. Each
row's result sets were checked identical first. With `fd -j 8` the gap narrows (10.3 ms vs 7.9 ms
on the projects folder). Method and cost model: [performance](docs/performance.md).

```bash
cgs run bench ~/some/tree   # any directory
cgs bench-trees             # the two generated rows
```

## Contributing

Results from other machines and filesystems are the most useful contribution, especially macOS,
Windows and network mounts.

```bash
cgs check                  # fmt + clippy + full test suite
cgs check-windows          # type-check and lint every target for Windows
cgs run t symlink          # tests whose names contain a filter
```

Benchmarks need `fd`, `python3` and a C compiler (`benches/peakrss.c` measures peak memory without
the Python parent's footprint).

## Docs

- [CLI reference](docs/cli.md): query grammar, options, filtering, output, exit codes
- [Design](docs/design.md): pipeline, module boundaries, and why each choice was made
- [Performance](docs/performance.md): method, results, cost model, limits
- [Decision 0001](docs/decisions/0001-v1-behavior-choices.md): edge-case behavior and the tests that pin it

## License

MIT, see [LICENSE](LICENSE).
