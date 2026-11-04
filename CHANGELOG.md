# Changelog

All notable changes to `zf`'s behavior, by version. The generated-directory list is versioned
here too, since changing it changes what a search returns.

## 0.1.0 - Unreleased

First release.

### Added

- Four query modes, picked from each word's shape: name (`index`), extension (`.rs`), glob
  (`*.rs`) and path (`src/main`). Every word must match.
- Case-insensitive, Unicode-aware matching; names that are not valid UTF-8 still match.
- Search where you point: a folder in the arguments (`~/Downloads/report`), `-p DIR`, or
  `--home`. The current directory otherwise.
- `--files` and `--dirs` to limit results to one kind.
- Hidden entries and generated directories are skipped; type a skipped name (`.env`,
  `node_modules`) to find it, or pass `--all` to include everything.
- `.gitignore`, `.git/info/exclude` and global git excludes inside git and jj repositories.
- `.ignore` and `.fdignore` everywhere; `.fdignore` rules take precedence.
- `-E`/`--exclude PATTERN` to skip gitignore-style patterns, even with `--all`.
- Plain paths on stdout, written as they are found; `-0` for NUL-separated output.
- `-n`/`--nearest` sorts results shallowest first, `-m`/`--modified` newest first.
- Control characters in names escaped as `\xNN` on a terminal; names starting with `-` keep
  their `./` prefix.
- Exit status: 0 found, 1 nothing found, 2 usage error, 3 some entries unreadable,
  130 interrupted.
- A walk that adds workers as the tree widens, up to 16; `ZF_TRACE=1` prints a summary of each
  walk to stderr.
- Generated-directory list, version 1:

  | Directory | Skipped when |
  |---|---|
  | `.git`, `.svn`, `.hg`, `.next`, `node_modules`, `__pycache__` | always |
  | `target` | beside `Cargo.toml` or `pom.xml` |
  | `build`, `dist`, `out`, `coverage` | beside a project manifest |
  | `vendor` | beside `go.mod` or `composer.json` |
  | a name containing `env` | it holds `pyvenv.cfg` |

### Fixed

- `-n`/`--nearest` on Windows: names starting with `-` (printed with a `.\` prefix) were sorted
  one level too deep.
