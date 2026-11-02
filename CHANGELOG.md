# Changelog

Changes to `zf`'s behavior by version, including the generated-directory list.

## 0.1.0 (unreleased)

First release.

- Query words: name, extension, glob and path modes; every word must match.
- Locations in the arguments, `-p` and `--home` set where to search.
- Generated-directory list, version 1:
  - always skipped: `.git`, `.svn`, `.hg`, `.next`, `node_modules`, `__pycache__`
  - `target` beside `Cargo.toml` or `pom.xml`
  - `build`, `dist`, `out`, `coverage` beside a project manifest
  - `vendor` beside `go.mod` or `composer.json`
  - a directory whose name contains `env` and that holds `pyvenv.cfg`
- Git ignore rules apply inside git and jj repositories; `.ignore` applies everywhere.
- The walk adds workers as the tree widens, up to 16; `ZF_TRACE=1` reports each walk on stderr.
