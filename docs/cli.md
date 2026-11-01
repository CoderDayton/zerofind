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
zf [OPTIONS] <QUERY>

      --home          Search standard user folders
  -p, --path <DIR>    Search an explicit directory (or evaluate one file)
      --all           Include normally excluded entries
      --files         Return files only
      --dirs          Return directories only
  -0, --print0        Use NUL-delimited output
  -h, --help          Show help
  -V, --version       Show version
```

Rules that apply everywhere:

- No configuration files, environment variables or saved state change behavior.
- The query is interpreted identically whether stdout is a terminal, a pipe or a file.
- `--home` with `--path`, and `--files` with `--dirs`, are usage errors.
- A query starting with `-` needs `--`: `zf -- -draft`.

## Query modes

The mode is chosen from the query text, in this order, before the walk starts:

1. Contains `/` (or `\` on Windows): **path mode**.
2. Starts with one `.` and has no other `.`: **extension mode**.
3. Anything else: **name mode**.

A query that matches nothing keeps its mode; there is no fallback.

All comparisons are case-insensitive using Unicode lowercasing, so `ÄPFEL` matches `äpfel.txt`.
It is lowercasing, not full case folding: `straße` does not match `STRASSE`. Names that are not
valid UTF-8 are compared on their ASCII bytes only and never decoded lossily. Matching is binary;
there is no fuzzy matching or ranking.

### Name mode

Substring of the entry's own name, extension included. Parent directories never match:
`zf index` finds `src/index.ts` but not `index/main.rs`.

### Extension mode

The text after the name's last dot must equal the query. A leading dot starts no extension.

| Query | Matches | Does not match |
|---|---|---|
| `.rs` | `main.rs`, `lib.RS` | `main.rs.bak`, `main.rsx`, `.rs` |
| `.config` | `web.config` | the hidden directory `.config` |

`.tar.gz` has two dots, so it is a name-mode substring query. To find a hidden name use name mode
(`zf config --all`) or a path (`zf ./.config`).

### Path mode

The query splits into components. Every component but the last must equal a path component; the
last is a prefix of the entry's name. Components must be contiguous and end at the entry itself,
so `src/main` matches `src/main.rs` and `app/src/main.ts`, not `src/main.rs/notes.txt` or
`source/main.rs`.

| Form | Meaning |
|---|---|
| `src/main` | anywhere below the root |
| `./src/main` | starting at the root only |
| `src/` or `src/.` | a directory named exactly `src`; descendants are not results |
| `src//main`, `src/./main` | normalized to `src/main` |

Rejected with exit 2: `..` as a component or as the whole query, a leading `/`, a bare `.`, and
a path with no components (`./`). Other roots belong to `--path`.

## Scope

| Scope | Root | Printed paths |
|---|---|---|
| default | the current directory; parents are never walked and the root is not printed | relative, no `./` |
| `--home` | Documents, Desktop, Downloads | absolute |
| `-p DIR` | `DIR` | as given: `etc/hosts`, `./etc/hosts`, `/etc/hosts` |
| `-p FILE` | the file alone, matched by its name | as given |

`--home` uses the platform's configured folders (XDG `user-dirs.dirs` on Linux). A folder that is
missing, set to the home directory itself (xdg's way of disabling it), or inside another chosen
folder is skipped. The conventional names under home are used only when the platform configures
none of the three. If nothing is left, `zf` exits 2; it never searches the whole home directory.

A symlinked `-p` root is followed to its target, which becomes the boundary; output keeps the
name you typed.

## Filtering

| Rule | Default | With `--all` |
|---|---|---|
| Hidden entries (name starts with `.`) | skipped | included |
| `.gitignore`, `.ignore`, `.git/info/exclude`, global git excludes | applied, also outside git repositories and from parent directories of the root | ignored |
| Directories named `.git`, `.svn`, `.hg`, `node_modules`, `target`, `dist`, `build`, `.next` | skipped (list version 1; files with these names are kept) | included |
| Symlinked directories | printed if they match, never entered | same |

Skipped directories are pruned: nothing below them is read. `--files` and `--dirs` only filter
what is printed; directories are still walked. A symlink counts as its target's kind for
`--files`, `--dirs`, `dir/` queries and the excluded-directory list.

A path query that names a hidden component reaches it without `--all`: `zf ./.github/workflows`
enters `.github`. Only the named component is exempt, and only from the hidden rule;
`zf ./.git/config` still finds nothing, because `.git` is on the excluded list.

## Output

One path per line, or per NUL with `-0`. Nothing else is ever written to stdout: no header,
count, color or quoting. Non-UTF-8 names are written as raw bytes.

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
| 130 | interrupted by Ctrl+C; only whole paths were written |

Diagnostics go to stderr as one line each, `zf: <path>: <reason>`, printed when they happen. A
malformed ignore rule prints `zf: warning: ...` and does not change the status.
