# f5h

A Unix TUI file manager inspired by FILMTN, a DOS-era file manager for PC-98 by K. Ishida (1996).

> 日本語版は [docs/README.ja.md](docs/README.ja.md) をご覧ください。

```
e:Edit  x:Run  ::Func  b:Git  t:Tree  s:Sort  /:Search  ~:Home  J:Jump  c:Copy  m:Move  d:Del  a:Perm  q:Quit
╭─ Volume Info ───────────────── Path ─── 12:34:56 ─── f5h v0.1.3 ─╮
│ /dev/sda1 (ext4)            │ /home/user/src                         main ↑2↓1 │
 Free:          897Gi ├─ File Info ──────────────────────────────────────────────────┤
 Curr:  204800  12Fs │ File: main.rs                                                  │
 Total:         976Gi │ Type: Rust source text                                        │
 Used:          79Gi │ Size: 49152  Blk:96  Inode:1234567  Links:1   Mod: 2026-03-15 12:00:00 │
 Usage:           8 % │ Perm: -rw-r--r--  Own: user:user                                      │
├─────────────────────┴──────────────────────────────────────────────────────────────┤
│*M main.rs      49152  2026-03-15 12:00 │   Cargo.toml    512  2026-01-01 00:00   │
│ A lib.rs        8192  2026-03-10 09:30 │   README.md    2048  2026-03-15 11:00   │
│   tests/        <DIR> 2026-02-01 14:00 │                                         │
╰──── / mai█                                    F1:HELP [N▲]   1/  1 Page ╯
```

## Features

- Original FILMTN-style layout: volume info panel, file info panel, multi-column file list
- Vim-style `hjkl` navigation + page scrolling
- 1/2/3/5 column display modes (`!` `@` `#` `%`)
- File tagging with `Space` (tag + move) and `Home` (toggle all)
- **File operations**: copy (`c`), move (`m`), delete (`d`), chmod (`a`), mkdir (`K`)
  - FILMTN-style conflict dialog with U:if-newer / O:overwrite / C:rename / N:skip + Shift+ batch mode
  - All option dialogs support `j`/`k`/`↑`/`↓` + `Enter` for keyboard-only navigation
- **Sort** (`s`): by name, extension, size, or date; toggle ascending/descending with repeated press
- **Directory jump** (`J`): type any path to navigate directly; `~` expands to `$HOME`
- **Git integration** (`b` key): add, commit, fetch, push, pull (rebase), merge, switch branch, stash
  - Branch name with ahead/behind count (e.g. `main ↑2↓1`) in the path row
  - Per-file git status in the file list prefix column (M / A / ? / D)
- **Incremental filename search** (`/`): partial match with regex support; `n`/`N` for next/prev; highlights persist after `Enter`
- **Func dialog** (`:`): command palette with filtered list, Tab completion — `:mv`, `:q`
- **F1 help overlay**: full key reference
- File coloring from `LS_COLORS` (Linux) or `LSCOLORS` (macOS)
- Symlink-aware navigation
- Extension-to-program associations (open PDFs with Evince, images with feh, etc.)
- Fully configurable UI colors and keybindings via TOML
- Terminal tab title set to `🍥 f5h` on launch

## Requirements

- Rust (edition 2024)
- Linux / macOS
- A [Nerd Fonts](https://www.nerdfonts.com/) compatible terminal (for the git branch icon)

## Installation

```bash
git clone https://github.com/mskako/f5h
cd f5h
cargo build --release
cp target/release/f5h ~/.local/bin/
```

## Key Bindings

All bindings (except arrow keys) are remappable in `config.toml`.

### Navigation

| Key | Action |
|---|---|
| `j` / `↓` | Move down |
| `k` / `↑` | Move up |
| `h` / `←` | Move left (column) |
| `l` / `→` | Move right (column) |
| `g` / `G` | First / last entry |
| `PageUp` / `PageDown` | Previous / next page |
| `Enter` | Enter directory / open file |
| `Backspace` | Parent directory |
| `~` | Home directory |
| `Space` | Tag current entry + move down |
| `Home` | Toggle tag on all entries |
| `!` `@` `#` `%` | 1 / 2 / 3 / 5 column mode |
| `Tab` | Directory tree pane (toggle) |
| `q` | Quit |

### File Operations

| Key | Action |
|---|---|
| `c` | Copy (tagged files or cursor entry) |
| `m` | Move |
| `d` | Delete |
| `a` | Change permissions (octal chmod) |
| `e` | Open in editor |
| `K` | Make directory |
| `J` | Directory jump |
| `s` | Sort dialog |

### Search

| Key | Action |
|---|---|
| `/` | Open incremental search bar |
| `n` | Jump to next match |
| `N` | Jump to previous match |

Type to search (partial match, case-insensitive, regex supported). `Enter` confirms and keeps highlights active. `Esc` cancels and returns to the original position. After confirming, `n`/`N` continue navigating matches. `Esc` in normal mode clears the highlights.

### Sort Dialog (`s`)

| Key | Action |
|---|---|
| `N` | Sort by name |
| `X` | Sort by extension |
| `S` | Sort by size |
| `T` | Sort by date |
| `U` | No sort (filesystem order) |
| `j`/`k`/`↑`/`↓` | Move cursor |
| `Enter` | Apply selected sort |

Pressing the same sort key again toggles ascending/descending order. The current sort is shown in the status bar as `[N▲]` / `[X▼]` etc.

### Func Dialog (`:`)

Press `:` to open the Func dialog — a command palette with a filtered list and Tab completion.

| Command | Action |
|---|---|
| `:mv <name>` | Rename cursor file |
| `:q` / `:quit` | Quit |
| `:help` | Show all commands |

Type to filter commands. `Tab` completes the first match. `↑`/`↓` moves selection. `Enter` executes. `Esc` closes.

### Git Submenu (`b`)

Press `b` to open the git submenu. Use `j`/`k`/`↑`/`↓` to move the cursor and `Enter` to execute, or press the key letter directly.

| Key | Action |
|---|---|
| `a` | `git add` cursor / tagged files |
| `A` | `git add .` |
| `c` | Commit (prompts for message) |
| `f` | `git fetch origin` |
| `p` | `git push` |
| `P` | `git pull --rebase` |
| `m` | `git merge --no-ff @{u}` |
| `s` | Switch branch |
| `t` | Stash push (optional message) |
| `T` | Stash pop |

Remote operations (fetch / push / pull) prompt for an SSH passphrase. Leave blank to use the SSH agent.

### Copy / Move Conflict Dialog

When a destination file already exists. Use `j`/`k`/`↑`/`↓` + `Enter`, or press the key letter directly.

| Key | Action |
|---|---|
| `u` | Copy/move if source is newer |
| `o` | Overwrite |
| `c` | Rename, then copy/move |
| `n` | Skip |
| `U` `O` `N` (uppercase) | Same, but apply to **all remaining** conflicts |
| `Esc` | Abort entire operation |

## File List Prefix

```
*M main.rs      49152  2026-03-15 12:00
^^
||
|+-- git status: M=modified  A=added  ?=untracked  D=deleted
+--- tag marker: * = tagged (shown in yellow)
```

The cursor row is highlighted with reverse-video. The 3-character prefix is intentionally not reversed so tags and git status remain readable.

## Configuration

Config file: `~/.config/f5h/config.toml`

All sections and keys are optional.

### [programs]

```toml
[programs]
editor = "vim"   # fallback: $EDITOR → nano
pager  = "less"  # fallback: $PAGER  → less
pdf    = "evince"
jpg    = "feh"
png    = "feh"
mp4    = "mpv"
```

### [display]

```toml
[display]
show_hidden = false   # show dotfiles
```

### [colors]

```toml
[colors]
border = "cyan"    # box-drawing characters
title  = "yellow"  # section headers and version string
label  = "cyan"    # field labels
unit   = "cyan"    # measurement units (Gi / Ki / B / %)
date   = "white"   # file timestamps
clock  = "white"   # live clock
ls_colors = ""     # empty = use $LS_COLORS / $LSCOLORS
```

Available color names: `black` `darkgray` `red` `lightred` `green` `lightgreen` `yellow` `lightyellow` `blue` `lightblue` `magenta` `lightmagenta` `cyan` `lightcyan` `white` `lightgray`

### [keys]

Default bindings (all remappable):

```toml
[keys]
move_up      = "k"
move_down    = "j"
move_left    = "h"
move_right   = "l"
first_entry  = "g"
last_entry   = "G"
page_up      = "PageUp"
page_down    = "PageDown"
enter        = "Enter"
parent_dir   = "Backspace"
home_dir     = "~"
tag_move     = "Space"
tag_all      = "Home"
quit         = "q"
col_mode_1   = "!"
col_mode_2   = "@"
col_mode_3   = "#"
col_mode_5   = "%"
copy         = "c"
move         = "m"
delete       = "d"
attr         = "a"
edit         = "e"
mkdir        = "K"
dir_jump     = "J"
sort         = "s"
tree_toggle  = "Tab"
run          = "x"
func         = ":"
git          = "b"
search       = "/"
search_next  = "n"
search_prev  = "N"
```

Key string format: `"a"` `"Enter"` `"Backspace"` `"Tab"` `"Space"` `"Esc"` `"PageUp"` `"PageDown"` `"Home"` `"End"` `"F1"`–`"F12"` `"Ctrl+a"` `"Shift+x"` `"Alt+x"`

Arrow keys are hardcoded aliases for `hjkl` and cannot be rebound.

## License

MIT

## Disclaimer

THIS SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND. **Use of file operation commands (copy, move, delete, chmod) is entirely at your own risk. Always keep backups of important data. The authors accept no responsibility for any data loss.**

## Acknowledgements

Inspired by FILMTN for PC-98 DOS by K. Ishida (1996). This project aims to bring a similar compact, keyboard-driven feel to modern Unix terminals.
