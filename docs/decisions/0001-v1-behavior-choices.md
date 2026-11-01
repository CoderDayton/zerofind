# 0001: v1 behavior choices

Status: accepted (2026-11-01). Edge cases where more than one behavior was reasonable, and the one
`zf` picks. Each decision is pinned by the named test.

| # | Decision | Test |
|---|---|---|
| 1 | `zf .`, `zf ..` and queries starting with `/` are usage errors (exit 2); other roots belong to `--path`. A trailing `/.` means the same as a trailing `/` (`src/.` names the directory `src`). | `degenerate_queries_are_rejected`, `parent_components_are_rejected`, `trailing_dot_component_is_directory_name` |
| 2 | Case-insensitivity is Unicode lowercasing, not full case folding (`ß` does not match `ss`). Non-UTF-8 names fold ASCII bytes only. | `unicode_case_insensitive`, `non_utf8_names_match_ascii_bytes` |
| 3 | Git ignore rules (`.gitignore`, `.git/info/exclude`, global excludes) apply only inside a git repository, as git reads them; `.ignore` applies everywhere. Ignore files in parent directories of the root apply under the same rule. A dotfiles `~/.gitignore` of `*` therefore hides nothing outside a repository. | `ignore_rules_from_parent_directories_apply`, `gitignore_outside_a_git_repository_is_not_applied`, `dot_ignore_applies_outside_a_git_repository` |
| 4 | Built-in exclusions (`EXCLUDED_DIRS_V1`) match directory names exactly and never apply to files. | `exclusions_apply_to_directories_only` |
| 5 | The hidden exemption covers path queries only. Intermediate components need equality at the matching position; the last component keeps prefix semantics (`./.env` also reaches `.envrc`). | `only_path_queries_grant_the_exemption`, `path_query_names_its_hidden_components` |
| 6 | A symlink is an entry: emitted when it matches, never descended. For `--files`/`--dirs`, `dir/` queries and built-in exclusions it takes its target's kind, so a `build` link to a directory is excluded. The target is stat'ed only when one of those needs it. | `symlinked_directory_does_not_escape_root`, `symlink_to_directory_is_excluded_like_one` |
| 7 | `--files` with `--dirs` is a usage error. | `missing_query_or_unknown_option_is_usage_error` |
| 8 | `--home`: folders resolving to home itself (xdg "disabled") are skipped; nested or repeated folders are searched once; conventional names are used only when the platform configures none of the three. | `home_*` |
| 9 | Recoverable traversal errors print one `zf: <path>: <reason>` line each as they happen; no trailing count. | `unreadable_directory_continues_and_exits_3` |
| 10 | An output failure other than a broken pipe (e.g. disk full) prints a diagnostic and exits 2. | — |
| 11 | A malformed ignore rule (bad glob), in the tree or in a parent of the root, prints `zf: warning: ...` and does not change the exit status: the walk completed. | `malformed_ignore_file_warns_without_partial_status` |
| 12 | When stdout is a terminal, control characters and invalid UTF-8 print as `\xNN`; pipes, files and `-0` get raw bytes. | `terminal_output_escapes_control_characters`, `piped_output_stays_raw` |
| 13 | A local result whose name starts with `-` keeps its `./`, so it never reads as an option downstream. | `local_name_starting_with_dash_keeps_dot_slash` |
| 14 | A second Ctrl+C exits 130 at once, for a walk stuck in slow I/O. | `second_ctrl_c_asks_for_an_immediate_exit` |
| 15 | A closed stderr drops diagnostics; the search finishes with its normal status. | `closed_stderr_does_not_crash` |
