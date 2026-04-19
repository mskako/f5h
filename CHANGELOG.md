# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.2.0] - 2026-04-19

### Added

- **Process viewer** (`:proc` in func dialog): top/ps-like process list
  - 7-row FILMTN-style header with two panels:
    - Left: load averages, CPU core count, memory used/total/%, swap, uptime
    - Right: PID, PPID, TTY, user, stat, %CPU, %MEM, VSZ, RSS, command name, full cmdline, start time, elapsed time, CWD, thread count, open FD count
  - Scrollable process list: PID / ユーザ / %CPU / %MEM / VSZ / RSS / STAT / コマンド
  - Row color by STAT: R=yellow / T=cyan / D=green / Z=magenta / S and others=white; CPU ≥ 50% overrides to red
  - Sort (`s`): cycle CPU → MEM → PID → USER → CMD; same key toggles ascending/descending; indicator shown in column header
  - Signal menu (`x`): SIGHUP(1) / SIGINT(2) / SIGKILL(9) / SIGTERM(15) / SIGCONT(18) / SIGSTOP(20); `j`/`k`/`Enter` or direct key
  - FD list (`f`/`Enter`): fd number, type tag (stdin/stdout/stderr/file/socket/pipe/anon), target path; `r` refresh, `Esc`/`q` back
  - F1 help overlay specific to proc/FD mode
  - Mode-aware menu bar: switches to proc/FD-specific key hints when in proc mode
  - Auto-refresh every 500ms (paused while FD list is open)
  - Reads `/proc` filesystem directly — no `ps` subprocess; TTY decoded from tty_nr (pts/N, ttyN, ttySN)
  - Header labels in cyan, values in white; load averages in green; command name in yellow

### Fixed

- Func dialog: selecting a command with `↑`/`↓` (without typing) and pressing `Enter` incorrectly showed "不明なコマンド" — the Enter handler now resolves the selected candidate by index from the filtered list when the input field is empty
- Proc mode top border: trailing `─` of `f5h v0.1` was rendered in yellow instead of cyan
- Proc mode top border: clock is now right-aligned (consistent with file mode)
- Proc mode top border: gap in the border line between count label and fill `─` characters is fixed

## [0.1.3] - 2026-04-16

### Added

- **Sort dialog** (`s` key): sort by name (N), extension (X), size (S), date (T), or none (U); same key toggles ascending/descending; current sort shown as `[X▲]` in the status bar
- **Directory jump** (`J` key): dialog pre-filled with current path; `~` expands to `$HOME`; errors shown inline
- **F1 help overlay**: full key reference; dismissed by any key
- **Success overlay** for git operations: green-bordered dialog shown on completion of add, commit, merge, stash, switch
- **Terminal tab title**: set to `🍥 f5h` on launch, reset on exit (OSC 0 escape sequence)
- **Git submenu** (`b` key) with the following operations:
  - `a` / `A` — `git add` (cursor/tagged files) / `git add .`
  - `c` — commit with message input
  - `f` — `git fetch origin`
  - `p` / `P` — push / pull (fetch + rebase)
  - `m` — `git merge --no-ff @{u}`
  - `s` — switch branch
  - `t` / `T` — stash push (optional message) / stash pop
- SSH passphrase input dialog for remote operations; empty input uses SSH agent
- Ahead/behind commit count next to branch name in the path row (e.g. `main ↑2↓1`)
- Dialog cursor navigation: `j`/`k`/`↑`/`↓` and `Enter` work in the sort dialog, delete confirm dialog, copy/move conflict dialog, and git menu

### Changed

- **Search confirm**: `Enter` now confirms search and keeps match highlights active; `n`/`N` continue navigating matches; `Esc` clears the highlight
- **Menu bar**: added ソート/Sort and 検索/Search entries; renamed HOME → ホーム/Home; removed 名前/Name entry; added ジャンプ/Jump entry
- **Tree pane width**: proportional to terminal width (~20%, clamped 10–40 columns) instead of a fixed 22 columns
- Pull (`P`) changed from `--ff-only` to `--rebase`
- Error dialog now displays full multi-line git output (up to 10 lines)
- Replaced libgit2 dependency with subprocess `git` calls for remote operations

### Fixed

- Sort dialog border no longer has a blank row before the bottom border
- Sort dialog cursor now correctly highlights the selected row (was tracking applied mode instead of cursor position)
- Delete confirm Y/N highlight now follows `h`/`l`/`←`/`→` correctly (was reversed)

## [0.1.2] - 2026-04-08

### Added

- `lookup_action` key dispatch with fallback for Shift+symbol keys (e.g. `!` sent as `Shift+1` by some terminals)
- `first_list_entry_index` / `last_list_entry_index` methods extracted for reuse and testability
- Tests for `lookup_action` fallback behavior and list entry index helpers
- Macro command dialog (`:`) — vi-style command input; `:q` / `:quit` quits the application
- Menu bar now shows `g:先頭` and `G:末尾` entries for first/last navigation

### Changed

- Run dialog key changed from `:` to `x`; `:` is now assigned to the macro command dialog
- Run dialog now shows "--- Press any key to continue ---" after command execution
- `fmt_datetime` now uses `chrono::Local` for correct local-timezone formatting
- `g` / `G` now skip `..` — cursor lands on the first real entry
- Removed unused `is_leap` helper

### Fixed

- Run dialog now uses the currently displayed directory as working directory
- Git status M/A/? marks now appear correctly on files in subdirectories
- Replace `let_chains` with stable equivalents (Rust 1.85/1.86 compatibility)

## [0.1.1] - 2026-03-21

### Added

- macOS `LSCOLORS` support as a fallback when `LS_COLORS` is not set
- Tests covering macOS/Linux color parsing, symlink directory navigation, and quoted program arguments

### Changed

- Clock display color changed from white to yellow (default)
- Replaced `df`-based volume detection with `statvfs` / `statfs` / `/proc/self/mountinfo`
- Symlinks to directories treated as enterable while keeping symlink-specific file info
- Ignored `..` in tag toggling and tag-all operations
- Improved external program launching to respect quoted arguments and backslash escapes

### Fixed

- Symlink file info no longer depends on platform-specific `file` output
- `if-newer` tests no longer rely on sub-second filesystem timestamp resolution
- Fixed statvfs field type mismatch on macOS

## [0.1.0] - 2026-03-20

### Added

- FILMTN-style layout: volume info panel, file info panel, multi-column file list
- Vim-style `hjkl` navigation, page scrolling, jump to first/last
- 1 / 2 / 3 / 5 column display modes (`!` `@` `#` `%`)
- File tagging: `Space` (tag + move down), `Home` (toggle all)
- File operations: copy (`c`), move (`m`), delete (`d`), rename (`n`), chmod (`a`), mkdir (`K`)
- FILMTN-style conflict dialog: U/O/C/N + Shift for batch + Esc to abort
- Same-directory copy → new name sub-dialog
- Open file in editor (`e`) or associated program (`Enter`)
- Directory tree pane (`Tab` to toggle)
- Git integration: branch display, per-file status prefix (M / A / ? / D)
- `LS_COLORS`-based file coloring with cursor reverse-video highlight
- Configurable keybindings, editor, pager, per-extension programs, and UI colors via `~/.config/f5h/config.toml`
- Permission error handling: error overlay on failed navigation or file reload
