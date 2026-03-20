# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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
