# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- Run dialog (`:`) now shows "--- Press any key to continue ---" after command execution, accepting any key via raw mode

### Fixed

- Run dialog now uses the currently displayed directory as working directory instead of the f5h launch directory
- Git status M/A/? marks now appear correctly on files when navigated into subdirectories

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
