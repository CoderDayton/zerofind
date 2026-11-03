# 0003: `--exclude` and `.fdignore`, no config file

Status: accepted (2026-11-03).

## Context

Users want to skip folders `zf` searches by default (`~/.cache`, an old backup) and keep rules
they already wrote for `fd`. Ignore files covered lasting rules, but a one-off skip meant
editing a file, and `.fdignore` was not read. Config files are the usual answer in other tools
(fd #362, eza #139, ripgrep #196), and the usual objection is that they change what scripts
get.

## Decision

- **`-E`/`--exclude PATTERN`**, repeatable. Each pattern is one gitignore line anchored at each
  search root. It beats every other rule: `--all` does not lift it, and a query that spells the
  name does not reach it. Excluded directories are pruned. An invalid pattern is exit 2.
  The short flag is `-E`, as in fd; `-e` is fd's extension filter, and fd users would misread it.
- **`.fdignore`** is read everywhere, like `.ignore`, with the `ignore` crate's custom-ignore
  precedence: any `.fdignore` rule outranks any `.ignore` rule, at any depth.
  `tests/walk_oracle.rs` generates `.fdignore` files and checks against the crate.
- **`.rgignore` is not read.** It hides files from content search (data, lockfiles, minified
  code) that a name search should still find.
- **No config file.** Lasting rules go in ignore files (`~/.ignore` covers every search under
  home); per-search rules go in `-E`. A different default belongs in a shell alias, which
  scripts do not see.

## Costs

- One more flag and one more ignore file name. Inside the walk the name is checked from each
  directory listing; only the folders above the root are probed for it, once per root. Warm
  search time over a home directory was unchanged (18.8 ms against 18.9 ms).
- A rule meant only for `zf` has no file of its own; `.fdignore` is shared with fd.
