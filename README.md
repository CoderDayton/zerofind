<div align="center">

# zerofind

Find files by name, extension, or path without learning flags.

![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B%20%C2%B7%202024%20edition-orange)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)

</div>

`zf` is a non-interactive filesystem search. The query text decides how it matches: `index` is a
name fragment, `.rs` is an extension, `*.rs` a glob, `src/main` a path, and several words must
all match. A location in the arguments (`~/Downloads/report`) says where to look; otherwise it
searches the current directory. It streams plain paths to stdout.

A parallel walk, which adds workers only as the tree widens (up to 16), fills per-worker buffers
that one writer drains, with no sorting or collection stage. On an i9-13900K with a warm page
cache, the first result arrives in 2–4 ms across every benchmark tree, against 5.8–30 ms for
`fd` 10.5.0, at 4–11 MB peak memory against 148–170 MB.

It is not a general query language: no regex, size or time filters, or exec. See
[When not to use this](#when-not-to-use-this).

```console
$ zf .rs
src/main.rs
src/search/traversal.rs
tests/query_contract.rs
```

## Features

- **First result in 2–4 ms** on every benchmark tree, up to 845,000 entries, and 7–8 ms with a
  cold page cache; results are never collected or sorted. [Design](docs/design.md) explains the
  pipeline.
- **Memory does not grow with matches**: peak RSS is 3.9 MB (at most 4.3) with 100,000 matches
  and 4.0 MB (at most 4.2) with none on the same tree, and 4.2 MB (at most 4.4) when the reader
  is slow.
- **Fixed query grammar**: each word's mode comes from its text before the walk and never
  changes because nothing matched.
- **Conservative defaults**: hidden entries, `.gitignore` rules inside git (or jj) repositories, `.ignore`
  rules everywhere (both also from parent directories) and generated directories are skipped and
  pruned before descent. `target`, `build` and `dist` count as generated only beside a project
  manifest, so `~/Documents/build` is searched. Spelling a skipped name (`.env`, `node_modules`)
  reaches it; `--all` lifts everything.
- **Stays in bounds**: never follows a symlinked directory, never climbs above the search root,
  never reads file contents.
- **Pipe-safe**: stdout carries paths only; `-0` uses NUL separators; names starting with `-`
  keep their `./`; a closed pipe exits 0 quietly; Ctrl+C exits 130 with only whole paths written.
- **Terminal-safe**: on a terminal, control characters in names print as `\xNN`, so a file name
  cannot drive the terminal.
- **Small**: 2.0 MB stripped binary, 2–5 ms to start in an empty directory.

## Usage

```bash
cargo install --path .
```

```bash
zf index                 # names containing "index", case-insensitive
zf annual report         # names containing both words
zf .env                  # every .env file, and *.env
zf ~/Downloads/invoice   # "invoice" anywhere under ~/Downloads
zf .pdf --home           # the whole home directory
zf ./src/main            # path anchored at the search root
zf '*.rs' | xargs wc -l  # results are plain paths
```

| Query | Mode | Matches | Does not match |
|---|---|---|---|
| `config` | name substring | `app-config.toml`, `Config.json` | `settings.yaml` |
| `main.rs` | dotted name, from a word start | `main.rs`, `my-main.rs` | `domain.rs` |
| `.rs` | exact extension, or that exact name | `main.rs` | `main.rs.bak` |
| `.env` | same | `.env`, `app/.env`, `prod.env` | `.envrc` |
| `*.rs` | whole-name glob (`*`, `?`) | `main.rs` | `main.rs.bak` |
| `src/lib` | path: whole components, last one by prefix | `src/lib.rs`, `app/src/library.rs` | `src/old-lib.rs` |
| `./src/main` | path anchored at the root | `src/main.rs` | `pkg/src/main.rs` |
| `src/` | directory named exactly `src` | `src`, `app/src` | `src/main.rs`, `srcs` |

| Option | Effect |
|---|---|
| `--home` | Search the home directory |
| `-p, --path DIR` | Search `DIR` (or evaluate one file); repeatable, for relative directories a location cannot express |
| `--all` | Include hidden, ignored and generated entries |
| `--files` / `--dirs` | Emit only files or only directories; traversal is unchanged |
| `-0, --print0` | NUL-separated output |

Exit status is 0 with matches, 1 with none, 2 for a bad query or root, 3 when some directories
could not be read (other matches still print), 130 on Ctrl+C. Result order is unspecified; pipe
through `sort` when it matters. Every rule, edge case and diagnostic is in the
[CLI reference](docs/cli.md).

## When not to use this

| Situation | Use instead | Why |
|---|---|---|
| Regex, size, time or owner filters | `fd`, `find` | `zf` has four word modes and no filter expressions by design |
| Running a command per result | `fd -x`, `find -exec` | No exec; pipe to `xargs -0` with `-0` |
| Repeated whole-disk lookups | `plocate` | `zf` keeps no index; each run walks the tree |
| Sorted or ranked output | `zf ... \| sort`, `fzf` | Matching is binary and order is unspecified |
| Windows or macOS | `fd` | Not yet run there: the Windows build type-checks and CI covers both, but tests and benchmarks have only run on Linux |

## Benchmarks

| Tree | Entries | Query (matches) | zf | fd | zf first | fd first | zf RSS | fd RSS |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| zerofind repo | 47 + `.git/` and ignored files | `.rs` (25) | 3.7 ms | 23.3 ms | 4.1 ms | 24.6 ms | 4.0 MB | 152 MB |
| `~/.cargo/registry/src` | 46,253 | `.rs` (21,589) | 11.6 ms | 20.9 ms | 2.1 ms | 6.3 ms | 4.9 MB | 150 MB |
| `~/.rustup/toolchains` | 258,714 | `.rs` (5,854) | 14.3 ms | 34.8 ms | 2.0 ms | 6.9 ms | 11.4 MB | 170 MB |
| projects folder, mostly ignored | 844,800 | `main` (72) | 4.7 ms | 18.2 ms | 2.2 ms | 18.7 ms | 5.7 MB | 150 MB |
| generated flat | 100,100 | `m` (100,000) | 16.0 ms | 29.8 ms | 2.3 ms | 5.8 ms | 4.4 MB | 150 MB |
| generated deep, 120 levels | 5,040 | `main` (3,600) | 12.8 ms | 33.1 ms | 2.0 ms | 10.9 ms | 4.3 MB | 153 MB |
| `~/.rustup/toolchains`, cold cache | 258,714 | `.rs` (5,854) | 32.9 ms | 52.4 ms | 7.4 ms | 17.1 ms | 10.0 MB | 165 MB |

Median of 20 alternating runs per tool in one pass (7 for the cold row, page cache dropped before
each run), i9-13900K (32 threads), Linux 7.3, btrfs on NVMe, machine load 2–5; entry counts
include hidden and ignored entries; `fd` with its default 32 threads and flags that reproduce
`zf`'s filtering. Each row's result sets were checked identical first. On the small repo both
tools spend most of the time starting (`zf` took 2.7 ms in an empty directory in the same pass,
`fd` 22 ms). 100,000 matches to `/dev/null` take 12.5 ms, against 20.6 ms for `fd -j 8`. Method,
cold-cache runs and cost model: [performance](docs/performance.md).

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
- [Decision 0002](docs/decisions/0002-own-walker.md): why `zf` schedules its own walk and applies ignore files itself

## License

MIT, see [LICENSE](LICENSE).
