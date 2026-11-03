# zerofind

Find files by name, extension, or path without learning flags.

[![CI](https://github.com/CoderDayton/zerofind/actions/workflows/ci.yml/badge.svg)](https://github.com/CoderDayton/zerofind/actions/workflows/ci.yml)
![Rust 1.88+](https://img.shields.io/badge/rust-1.88%2B%20%C2%B7%202024%20edition-orange)
![License: MIT](https://img.shields.io/badge/license-MIT-blue)

`zf` finds files by name. What you type decides how it matches: `index` is part of a name, `.rs`
an extension, `*.rs` a glob, `src/main` a path. Put a folder in front (`~/Downloads/report`) to
search there; otherwise it searches the current directory. It prints plain paths.

It is quick and small: the first result shows up in 2–4 ms and it uses 4–11 MB of memory, where
`fd` takes 6–30 ms and 150–170 MB (i9-13900K, warm cache). It has no regex, filters or exec;
see [When not to use this](#when-not-to-use-this).

```console
$ zf .rs
src/main.rs
src/search/traversal.rs
tests/query_contract.rs
```

## Features

- **No flags to learn**: the query's shape picks the match mode, and it never guesses again when
  nothing matches.
- **Sensible skipping**: hidden files, `.gitignore`, `.ignore` and `.fdignore` rules, and build
  output such as `node_modules` are skipped. Type a skipped name (`.env`) to find it, or pass
  `--all`. Skip more with `-E`.
- **Stays in bounds**: never leaves the folder you search, never follows symlinked folders, never
  reads file contents.
- **Pipe-safe**: only paths on stdout, `-0` for NUL separators, quiet exit on a closed pipe.
- **Small**: 2.0 MB binary, starts in 2–5 ms, memory stays flat however many files match.

How it is built: [design](docs/design.md).

## Usage

```bash
cargo install --path .
```

```bash
zf index                 # names containing "index"
zf annual report         # names containing both words
zf .env                  # every .env file
zf ~/Downloads/invoice   # "invoice" anywhere under ~/Downloads
zf .pdf --home           # the whole home directory
zf '*.rs' | xargs wc -l  # results are plain paths
```

| Option               | Effect                                        |
| -------------------- | --------------------------------------------- |
| `--home`             | Search the home directory                     |
| `-p, --path DIR`     | Search `DIR`; repeatable                      |
| `--all`              | Include hidden, ignored and generated entries |
| `-E, --exclude PAT`  | Skip a gitignore-style pattern; repeatable    |
| `--files` / `--dirs` | Print only files or only directories          |
| `-0, --print0`       | NUL-separated output                          |

Results come in no particular order; pipe through `sort` if it matters. Query modes, skipping
rules and exit codes are in the [CLI reference](docs/cli.md).

## When not to use this

| Situation                          | Use instead             | Why                                           |
| ---------------------------------- | ----------------------- | --------------------------------------------- |
| Searching inside files             | `rg`, `grep`            | `zf` matches names only and never opens files |
| Regex, size, time or owner filters | `fd`, `find`            | Four fixed word modes, no filter expressions  |
| Running a command per result       | `fd -x`, `find -exec`   | No exec; pipe to `xargs -0` with `-0`         |
| Following symlinked folders        | `fd -L`, `find -L`      | `zf` never descends into them                 |
| Repeated whole-disk lookups        | `plocate`               | No index; each run walks the tree             |
| Sorted or ranked output            | `zf ... \| sort`, `fzf` | Matching is yes/no and order is unspecified   |

## Benchmarks

| Tree                         | Entries |      zf |      fd | zf memory | fd memory |
| ---------------------------- | ------: | ------: | ------: | --------: | --------: |
| `~/.cargo/registry/src`      |  46,253 | 11.6 ms | 20.9 ms |    4.9 MB |    150 MB |
| `~/.rustup/toolchains`       | 258,714 | 14.3 ms | 34.8 ms |   11.4 MB |    170 MB |
| `~/.rustup/toolchains`, cold | 258,714 | 32.9 ms | 52.4 ms |   10.0 MB |    165 MB |
| projects folder              | 844,800 |  4.7 ms | 18.2 ms |    5.7 MB |    150 MB |

Median of 20 runs (7 cold) on an i9-13900K, Linux, btrfs on NVMe, with `fd` 10.5.0 set to skip
the same files. More trees, method and cost model: [performance](docs/performance.md).

```bash
cgs run bench ~/some/tree
```

## Contributing

Results from macOS, Windows and network drives help most.

```bash
cgs check           # fmt + clippy + tests
cgs check-windows   # lint for Windows
```

Benchmarks need `fd`, `python3` and a C compiler.

## Docs

- [CLI reference](docs/cli.md): query grammar, options, filtering, output, exit codes
- [Design](docs/design.md): pipeline, module boundaries, and why each choice was made
- [Performance](docs/performance.md): method, results, cost model, limits

## License

MIT, see [LICENSE](LICENSE).
