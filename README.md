# f5h

A Unix TUI file manager inspired by FILMTN, a DOS-era file manager for PC-98 by K. Ishida (1996).

> 日本語版は [docs/README.ja.md](docs/README.ja.md) をご覧ください。

```
e:Edit  ::Run  t:Tree  c:Copy  m:Move  d:Del  n:Name  a:Perm  q:Quit
╭─ Volume Info ───────────────── Path ─── 12:34:56 ─── f5h v0.1 ─╮
│ /dev/sda1 (ext4)            │ /home/user/src                                  main │
 Free:          897Gi ├─ File Info ──────────────────────────────────────────────────┤
 Curr:  204800  12Fs │ File: main.rs                                                  │
 Total:         976Gi │ Type: Rust source text                                        │
 Used:          79Gi │ Size: 49152  Blk:96  Inode:1234567  Links:1   Mod: 2026-03-15 12:00:00  Chg: 2026-03-15 12:00:00 │
 Usage:           8 % │ Perm: -rw-r--r--  Own: user:user             Bth: 2026-01-01 00:00:00  Acc: 2026-03-15 11:59:00 │
├─────────────────────┴──────────────────────────────────────────────────────────────┤
│*M main.rs      49152  2026-03-15 12:00 │   Cargo.toml    512  2026-01-01 00:00   │
│ A lib.rs        8192  2026-03-10 09:30 │   README.md    2048  2026-03-15 11:00   │
│   tests/        <DIR> 2026-02-01 14:00 │                                         │
╰──────────────────────────────────────────────────── F1:HELP    1/  1 Page ╯
```

## Features

- Original FILMTN-style layout: volume info panel, file info panel, multi-column file list
- Vim-style `hjkl` navigation + page scrolling
- 1/2/3/5 column display modes (`!` `@` `#` `%`)
- File tagging with `Space` (tag + move) and `Home` (toggle all)
- **File operations**: copy (`c`), move (`m`), delete (`d`), rename (`n`), chmod (`a`), mkdir (`K`), open in editor (`e`)
  - FILMTN-style conflict dialog with U:if-newer / O:overwrite / C:rename / N:skip + Shift+ batch mode
- Git integration: branch display (Nerd Font icon) and per-file status in the prefix column
- File coloring from `LS_COLORS` (GNU/Linux) or `LSCOLORS` (macOS)
- Symlink-aware navigation: symlinks to directories can be entered with `Enter`
- Extension-to-program associations (open PDFs with Evince, images with feh, etc.)
- Fully configurable UI colors: border, title, label, unit, date
- Configurable keybindings via TOML
- Directory tree pane (`Tab`)

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
| `/` | Home directory |
| `Space` | Tag current entry + move down |
| `Home` | Toggle tag on all entries |
| `q` | Quit |
| `!` `@` `#` `%` | 1 / 2 / 3 / 5 column mode |
| `Tab` | Directory tree pane (toggle) |
| `c` | Copy (tagged files or cursor entry) |
| `m` | Move |
| `d` | Delete |
| `n` | Rename |
| `a` | Change permissions (octal chmod) |
| `e` | Open in editor |
| `K` | Make directory |
| `J` | Directory jump *(not yet implemented)* |

Arrow keys are hardcoded aliases for `hjkl` and cannot be rebound.

When opening a file, f5h looks up the file's extension in `[programs]`, then falls back to the configured pager.
Symlinks to directories are treated as enterable directories, while the file info pane shows the symlink target.

### Copy / Move Conflict Dialog

When a destination file already exists, f5h shows a FILMTN-style conflict dialog:

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

The cursor row is highlighted with terminal reverse-video. The 3-character prefix area is intentionally not reversed so tags and git status remain readable.

## Configuration

Config file: `~/.config/f5h/config.toml`

All sections and keys are optional. Omitted values fall back to their defaults.

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

### [colors]

```toml
[colors]
border = "cyan"    # box-drawing characters (╭ ─ │ ┴ ╰ …)
title  = "yellow"  # section headers ("ファイル情報" etc.) and version string
label  = "cyan"    # field labels (" 空き:" " 権限:" etc.) — "information title" in original FILMTN
unit   = "cyan"    # measurement units (Gi / Ki / B / %)
date   = "white"   # file timestamps (修正/変更/作成/参照)
clock  = "white"   # live clock in the top-right corner
ls_colors = ""     # color string; empty = use $LS_COLORS on Linux or $LSCOLORS on macOS
```

### [display]

```toml
[display]
show_hidden = false   # show dotfiles (. prefixed entries)
```

Available color names:

| Normal | Bright |
|---|---|
| `black` | `darkgray` |
| `red` | `lightred` |
| `green` | `lightgreen` |
| `yellow` | `lightyellow` |
| `blue` | `lightblue` |
| `magenta` | `lightmagenta` |
| `cyan` | `lightcyan` |
| `white` | `lightgray` / `gray` |

### [keys]

```toml
[keys]
move_up    = "k"
move_down  = "j"
move_left  = "h"
move_right = "l"
first_entry = "g"
last_entry  = "G"
page_up    = "PageUp"
page_down  = "PageDown"
enter      = "Enter"
parent_dir = "Backspace"
home_dir   = "/"
tag_move   = "Space"
tag_all    = "Home"
quit       = "q"
col_mode_1 = "!"
col_mode_2 = "@"
col_mode_3 = "#"
col_mode_5 = "%"
copy       = "c"
move       = "m"
delete     = "d"
rename     = "n"
attr       = "a"
edit       = "e"
mkdir      = "K"
dir_jump   = "J"
tree_toggle = "Tab"
```

Key string format:

| Type | Examples |
|---|---|
| Regular character | `"a"` `"Z"` `"1"` `"/"` |
| Special keys | `"Enter"` `"Backspace"` `"Tab"` `"Space"` `"Esc"` |
| Arrow keys | `"Up"` `"Down"` `"Left"` `"Right"` |
| Page / position | `"PageUp"` `"PageDown"` `"Home"` `"End"` |
| Function keys | `"F1"` … `"F12"` |
| With Ctrl | `"Ctrl+a"` `"Ctrl+Space"` |

`[programs]` holds both `editor` / `pager` and extension-specific handlers.

## Roadmap

- `J`: directory jump dialog
- Sort modes
- Wildcard filter
- `F1`: help overlay
- Copy/Move dialog: directory tree picker on Enter with blank input

## License

MIT

## Disclaimer

THIS SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

**Use of the file operation commands (copy, move, delete, rename, chmod) is entirely at your own risk. Always keep backups of important data. The authors accept no responsibility for any data loss.**

## Acknowledgements

Inspired by FILMTN for PC-98 DOS by K. Ishida (1996). This project aims to bring the similar compact, keyboard-driven feel to modern Unix terminals.
