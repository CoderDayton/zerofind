# 0001: v1 behavior choices

Status: accepted (2026-11-01). Edge cases where more than one behavior was reasonable, and the one
`zf` picks. Each decision is pinned by the named test.

| # | Decision | Test |
|---|---|---|
| 1 | An absolute path, `~`, anything through `..`, `.` and `./` are locations, not queries: an existing directory is searched, an existing file evaluated, and `DIR/word` with only `DIR` existing searches `DIR` for `word`. Relative paths (`src/main`, `./src`) stay path queries. A trailing `/.` means the same as a trailing `/` (`src/.` names the directory `src`). | `locations_are_absolute_home_parent_or_here`, `location_splits_into_root_and_term`, `parent_and_tilde_locations`, `trailing_dot_component_is_directory_name` |
| 2 | Case-insensitivity is Unicode lowercasing, not full case folding (`ß` does not match `ss`). Non-UTF-8 names fold ASCII bytes only. | `unicode_case_insensitive`, `non_utf8_names_match_ascii_bytes` |
| 3 | Git ignore rules (`.gitignore`, `.git/info/exclude`, global excludes) apply only inside a git repository, as git reads them; `.ignore` applies everywhere. Ignore files in parent directories of the root apply under the same rule. A dotfiles `~/.gitignore` of `*` therefore hides nothing outside a repository. | `ignore_rules_from_parent_directories_apply`, `gitignore_outside_a_git_repository_is_not_applied`, `dot_ignore_applies_outside_a_git_repository` |
| 4 | Generated directories are recognized by convention: `node_modules`, `__pycache__` and VCS directories always; `target`, `build`, `dist`, `out`, `coverage`, `vendor` only beside a project manifest; a virtual environment by its `pyvenv.cfg`. Files with these names are kept. | `build_directories_need_a_marker`, `target_needs_a_cargo_or_maven_project`, `virtual_environments_are_recognized_by_pyvenv_cfg`, `listed_names_that_are_files_are_kept` |
| 5 | A query that names a hidden or generated entry reaches it. A word spelling the whole name prints it without entering it; a path word enters each component it passes through, whose last component keeps prefix semantics (`./.env` also reaches `.envrc`). Fragments and a bare `*` name nothing. | `a_term_spelling_the_whole_name_names_it`, `path_query_reaches_the_components_it_names`, `naming_a_skipped_directory_finds_it_without_entering_it` |
| 6 | A symlink is an entry: emitted when it matches, never descended. For `--files`/`--dirs`, `dir/` queries and built-in exclusions it takes its target's kind, so a `build` link to a directory is excluded. The target is stat'ed only when one of those needs it. | `symlinked_directory_does_not_escape_root`, `symlink_to_directory_is_excluded_like_one` |
| 7 | `--files` with `--dirs` is a usage error. | `unknown_option_is_usage_error` |
| 8 | `--home` searches the whole home directory under the normal filtering. Roots from locations, `-p` and `--home` combine; repeated or nested roots are searched once. | `home_searches_the_whole_home_directory`, `dedupe_drops_repeats_and_nested_roots`, `overlapping_roots_report_each_entry_once` |
| 9 | Recoverable traversal errors print one `zf: <path>: <reason>` line each as they happen; no trailing count. | `unreadable_directory_continues_and_exits_3` |
| 10 | An output failure other than a broken pipe (e.g. disk full) prints a diagnostic and exits 2. | — |
| 11 | A malformed ignore rule (bad glob), in the tree or in a parent of the root, prints `zf: warning: ...` and does not change the exit status: the walk completed. | `malformed_ignore_file_warns_without_partial_status` |
| 12 | When stdout is a terminal, control characters and invalid UTF-8 print as `\xNN`; pipes, files and `-0` get raw bytes. | `terminal_output_escapes_control_characters`, `piped_output_stays_raw` |
| 13 | A local result whose name starts with `-` keeps its `./`, so it never reads as an option downstream. | `local_name_starting_with_dash_keeps_dot_slash` |
| 14 | A second Ctrl+C exits 130 at once, for a walk stuck in slow I/O. | `second_ctrl_c_asks_for_an_immediate_exit` |
| 15 | A closed stderr drops diagnostics; the search finishes with its normal status. | `closed_stderr_does_not_crash` |
| 16 | Every query word must match (AND); no words list everything visible. | `every_term_must_match`, `no_query_lists_everything_visible` |
| 17 | A word starting with `.` matches names ending with it or equal to it, so `.env` finds dotfiles and `.tar.gz` is an exact compound extension. | `dot_name_matches_extension_and_the_hidden_name`, `compound_extension_is_exact` |
| 18 | A name word containing a dot must start at a word boundary (`main.rs` misses `domain.rs`). Plain words stay substrings. | `dotted_name_starts_at_a_word_boundary` |
| 19 | `*` and `?` make a whole-name glob; brackets are literal. | `glob_matches_the_whole_name`, `glob_components_in_path_queries` |
| 20 | Names and words are lowercased and NFC-normalized, so decomposed names match composed text. | `decomposed_unicode_matches_composed_query` |
| 21 | A word starting with `-` that is not a known short-flag cluster is a query; `-p` needs its value as a separate argument. | `query_starting_with_dash_needs_no_separator`, `dash_words_move_behind_the_separator` |
