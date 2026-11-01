# CLI reference

Everything `zf` accepts and everything it prints, including the edge cases.

- [Grammar](#grammar)
- [Query modes](#query-modes)
- [Scope](#scope)
- [Filtering](#filtering)
- [Output](#output)
- [Exit status and diagnostics](#exit-status-and-diagnostics)

## Grammar

```text
zf [OPTIONS] [QUERY]...

      --home          Search the home directory
  -p, --path <DIR>    Search a directory (or evaluate one file); repeatable
      --all           Also search hidden entries, dependencies, build output and ignored files
      --files         Return files only
      --dirs          Return directories only
  -0, --print0        Use NUL-delimited output
  -h, --help          Show help
  -V, --version       Show version
```

Rules that apply everywhere:

- Each argument is either a [location](#scope) or a query word. Every word must match the
  entry; with no words, everything visible is listed.
- No configuration files or saved state change behavior; `HOME` is read only for `~` and
  `--home`.
- The query is interpreted identically whether stdout is a terminal, a pipe or a file.
- `--files` with `--dirs` is a usage error. Roots from locations, `-p` and `--home` combine;
  a root inside another, or repeated, is searched once.
- A word starting with `-` is a query unless it is a known short flag (`-0`, `-p`, `-h`,
  `-V`): `zf -draft` works. `-p` takes its directory as the next argument, so `-pending` is a
  query. Unknown long options (`--depth`) are usage errors.

## Query modes

Each word's mode is chosen from its text, in this order, before the walk starts:

1. Contains `/` (or `\` on Windows): **path mode**.
2. Contains `*` or `?`: **glob mode**.
3. Starts with `.`: **extension mode**.
4. Anything else: **name mode**.

A word that matches nothing keeps its mode; there is no fallback.

All comparisons are case-insensitive using Unicode lowercasing, so `ÄPFEL` matches `äpfel.txt`.
It is lowercasing, not full case folding: `straße` does not match `STRASSE`. UTF-8 names and
words are also normalized to composed form (NFC), so `café` matches a name stored decomposed, as
macOS and some archives do. Names that are not valid UTF-8 are compared on their ASCII bytes only
and never decoded lossily. Matching is binary; there is no fuzzy matching or ranking.

### Name mode

Substring of the entry's own name, extension included. Parent directories never match:
`zf index` finds `src/index.ts` but not `index/main.rs`.

A word containing a dot reads as a file name, so the match must start the name or follow a
character that is not a letter or digit: `main.rs` matches `main.rs`, `my-main.rs` and
`main.rs.bak`, not `domain.rs`; `config.json` does not match `tsconfig.json`.

### Extension mode

The name must end with the word, dot included, or be exactly the word. The second case is how a
dotfile is found by its name, anywhere below the root.

| Query | Matches | Does not match |
|---|---|---|
| `.rs` | `main.rs`, `lib.RS` | `main.rs.bak`, `main.rsx` |
| `.tar.gz` | `backup.tar.gz` | `backup.tar.gz.part`, `backup.tgz` |
| `.env` | `.env`, `app/.env`, `prod.env` | `.envrc` |
| `.config` | `web.config`, the hidden directory `.config` | |

### Glob mode

The pattern matches the whole name: `*` is any run of characters, `?` exactly one. Every other
character, `[` and `]` included, is literal. Quote globs so the shell leaves them alone:
`zf '*.rs'`. Unquoted, the shell expands them against the current directory first.

### Path mode

The word splits into components. Every component but the last must equal a path component; the
last is a prefix of the entry's name. A component with `*` or `?` is a glob over that whole
component instead. Components must be contiguous and end at the entry itself, so `src/main`
matches `src/main.rs` and `app/src/main.ts`, not `src/main.rs/notes.txt` or `source/main.rs`.

| Form | Meaning |
|---|---|
| `src/main` | anywhere below the root |
| `./src/main` | starting at the root only |
| `src/` or `src/.` | a directory named exactly `src`; descendants are not results |
| `src//main`, `src/./main` | normalized to `src/main` |

An absolute path, `~`, or anything through `..` is a [location](#scope), not a path query.

## Scope

An argument is a **location** when it is absolute, is `~` or starts with `~/`, contains a `..`
component, or is `.` or `./`. Everything else, `src/main` and `./src` included, is a query
word.

| Location | Means |
|---|---|
| an existing directory (`~/Downloads`, `..`, `.`) | search it |
| an existing file (`/etc/hosts`) | evaluate that file alone |
| `DIR/word` where only `DIR` exists (`~/Downloads/report`) | search `DIR` for `word` |
| anything else (`/nope/deeper/x`) | exit 2, naming the path |

| Scope | Root | Printed paths |
|---|---|---|
| default | the current directory; parents are never walked and the root is not printed | relative, no `./` (kept for names starting with `-`) |
| a location | as above | as typed, with `~` expanded: `../c/d.txt`, `/home/me/Downloads/report.pdf` |
| `--home` | the home directory | absolute |
| `-p DIR` | `DIR` | as given: `etc/hosts`, `./etc/hosts`, `/etc/hosts` |
| `-p FILE` | the file alone, matched by its name | as given |

The current directory is the default only when no location, `-p` or `--home` is given. A word
split off a location (`report` in `~/Downloads/report`) is a query word like any other and
applies to every root. `--home` walks the whole home directory under the usual filtering; on
a home with 22k visible directories it takes about 35 ms.

A symlinked root is followed to its target, which becomes the boundary; output keeps the name
you typed.

## Filtering

| Rule | Default | With `--all` |
|---|---|---|
| Hidden entries (name starts with `.`) | skipped | included |
| `.gitignore`, `.git/info/exclude`, global git excludes | applied inside a git repository, including ignore files above the root | ignored |
| `.ignore` | applied everywhere, including files above the root | ignored |
| Generated directories (below) | skipped | included |
| Symlinked directories | printed if they match, never entered | same |

Generated directories are recognized by convention, not by name alone, so a hand-made
`~/Documents/build` is searched:

| Directory | Skipped when |
|---|---|
| `.git`, `.svn`, `.hg`, `.next`, `node_modules`, `__pycache__` | always |
| `target` | beside `Cargo.toml` or `pom.xml` |
| `build`, `dist`, `out`, `coverage` | beside `package.json`, `pyproject.toml`, `setup.py`, `CMakeLists.txt`, `Makefile`, `meson.build`, `build.gradle(.kts)`, `pom.xml`, `go.mod` or `Cargo.toml` |
| `vendor` | beside `go.mod` or `composer.json` |
| any name containing `env` (`venv`, `py-env`) | it holds `pyvenv.cfg` |

Files with these names are kept. Skipped directories are pruned: nothing below them is read.
`--files` and `--dirs` only filter what is printed; directories are still walked. A symlink
counts as its target's kind for `--files`, `--dirs`, `dir/` queries and the generated list.

A query that names a hidden or generated entry reaches it without `--all`:

- A word that spells the whole name (`.env`, `node_modules`, `.*rc`) prints the entry. A
  directory found this way is not entered: `zf node_modules` prints `node_modules`, not every
  nested copy. A fragment (`env`, `node_mod`) or a bare `*` names nothing.
- A path word enters every component it passes through: `zf ./.github/workflows` enters
  `.github`, `zf node_modules/react` enters `node_modules`, `zf ./.git/config` finds
  `.git/config`.

## Output

One path per line, or per NUL with `-0`. Nothing else is ever written to stdout: no header,
count, color or quoting. Pipes, files and `-0` get raw bytes, non-UTF-8 names included.

When stdout is a terminal, control characters and invalid UTF-8 are shown as `\xNN`
(`evil\x1b]0;x\x07.txt`), so a file name cannot drive the terminal. A local result whose name
starts with `-` keeps its `./` (`./-rf`), so `zf rf | xargs rm` cannot pass `rm` an option.

Results are written as they are found. Records collect in a 32 KiB buffer while more are queued,
and the buffer is flushed whenever the queue runs dry, so a lone match is visible immediately.
Order is unspecified and varies between runs.

## Exit status and diagnostics

| Status | When |
|---:|---|
| 0 | at least one match, or the reader closed the pipe |
| 1 | no matches (stdout and stderr are empty) |
| 2 | bad option or query, or the root cannot be read |
| 3 | some entries could not be read; everything readable was still searched and printed |
| 130 | interrupted by Ctrl+C; only whole paths were written. A second Ctrl+C exits at once, for a walk stuck on slow storage |

Diagnostics go to stderr as one line each, `zf: <path>: <reason>`, printed when they happen. A
malformed ignore rule prints `zf: warning: ...` and does not change the status. If stderr is
closed (`2>&1 | head -1`), diagnostics are dropped and the search finishes normally.
