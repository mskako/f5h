# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Git submenu dialog (`b` key) with the following operations:
  - `a` / `A` — `git add` (cursor/tagged files) / `git add .`
  - `c` — commit with message input
  - `f` — `git fetch origin`
  - `p` / `P` — push / pull (fetch + rebase)
  - `m` — `git merge --no-ff @{u}` (merge tracking branch after fetch)
  - `s` — switch branch
  - `t` / `T` — stash push (optional message) / stash pop
- SSH passphrase input dialog for remote operations (fetch / push / pull); empty input uses SSH agent
- Ahead/behind commit count displayed next to branch name in the path row (e.g. `↑3↓1`)

### Changed

- Pull (`P`) changed from `--ff-only` to `--rebase`; handles diverged branches without erroring out
- Git submenu dialog widened (36 → 50 columns) to prevent description truncation
- Error dialog now displays full multi-line git output (up to 10 lines, capped by terminal height)
- Replaced libgit2 dependency with subprocess `git` calls for remote operations

### Fixed

- Error dialog border is now fully red (was inconsistently red/cyan)
- Dialog backgrounds no longer bleed through the underlying file list (`clear_rect` added)

## [0.1.2] - 2026-04-08

### Added

- `lookup_action` key dispatch with fallback for Shift+symbol keys (e.g. `!` sent as `Shift+1` by some terminals)
- `first_list_entry_index` / `last_list_entry_index` methods extracted for reuse and testability
- Tests for `lookup_action` fallback behavior and list entry index helpers
- Macro command dialog (`:`) — vi-style command input; `:q` / `:quit` quits the application
- Menu bar now shows `g:先頭` and `G:末尾` entries for first/last navigation

### Changed

- Run dialog key changed from `:` to `x`; `:` is now assigned to the macro command dialog
- Run dialog now shows "--- Press any key to continue ---" after command execution, accepting any key via raw mode
- `fmt_datetime` now uses `chrono::Local` for correct local-timezone date/time formatting (replaces manual UTC calculation)
- `g` / `G` (FirstEntry / LastEntry) now skip `..` — cursor lands on the first real entry, not the parent directory link
- Tests for `fmt_datetime` now compare against `chrono::Local` output, removing timezone-dependent hardcoded values
- Removed unused `is_leap` helper (superseded by chrono)

### Fixed

- Run dialog now uses the currently displayed directory as working directory instead of the f5h launch directory
- Git status M/A/? marks now appear correctly on files when navigated into subdirectories
- Replace `let_chains` with stable equivalents to support Rust 1.85/1.86 (stabilized in 1.88)

## [0.1.1] - 2026-03-21

### Added

- macOS `LSCOLORS` support as a fallback when `LS_COLORS` is not set
- Tests covering macOS/Linux color parsing, symlink directory navigation, and quoted program arguments

### Changed

- Clock display color changed from white to yellow (default)
- Replaced `df`-based volume size detection with `statvfs`
- Replaced `df`-based volume source/filesystem detection with `statfs` on macOS and `/proc/self/mountinfo` parsing on Linux
- Treated symlinks to directories as enterable directories while keeping symlink-specific file info display
- Normalized tree tests for macOS temporary-directory path differences
- Ignored `..` in tag toggling and tag-all operations
- Improved external program launching to respect quoted arguments and backslash escapes in `[programs]`

### Fixed

- Symlink file info no longer depends on platform-specific `file` output for directory links such as `/tmp`
- `if-newer` tests no longer rely on sub-second filesystem timestamp resolution
- Fixed statvfs field type mismatch on macOS (all fields cast to u64)

## [0.1.0] - 2026-03-20

### Added

- FILMTN-style layout: volume info panel, file info panel, multi-column file list
- Vim-style `hjkl` navigation, page scrolling (`PageUp` / `PageDown`), jump to first/last (`g` / `G`)
- 1 / 2 / 3 / 5 column display modes (`!` `@` `#` `%`)
- File tagging: `Space` (tag + move down), `Home` (toggle all)
- File operations: copy (`c`), move (`m`), delete (`d`), rename (`n`), chmod (`a`), mkdir (`K`)
- FILMTN-style conflict dialog on copy/move: U(if-newer) / O(overwrite) / C(rename) / N(skip) + Shift for batch apply + Esc to abort
- Same-directory copy → new name sub-dialog
- Open file in editor (`e`) or associated program (`Enter`)
- Directory tree pane (`Tab` to toggle)
- Git integration: branch display in path row (Nerd Font icon), per-file status prefix (M / A / ? / D)
- `LS_COLORS`-based file coloring with cursor reverse-video highlight
- Configurable keybindings, editor, pager, per-extension programs, and UI colors via `~/.config/f5h/config.toml`
- Permission error handling: error overlay on failed directory navigation or file reload
