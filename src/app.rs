use anyhow::Result;
use ratatui::style::Style;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::config::{
    Config, KeyMap, LABELS_EN, LABELS_JA, Labels, MENU_ACTIONS, UiColors, build_keymap, key_label,
    parse_file_colors,
};
use crate::fs_utils::{
    copy_path, disk_stats, fmt_datetime, get_file_info, get_git_ahead_behind, get_git_branch,
    get_git_status, get_volume_info, move_path, resolve_name, tree_has_children, tree_list_subdirs,
};

// ── Data structures ────────────────────────────────────────────────────

pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    pub blocks: u64,
    pub inode: u64,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub date: String,            // mtime: YYYY-MM-DD
    pub time_str: String,        // mtime: HH:MM
    pub time_full: String,       // mtime: HH:MM:SS
    pub atime_s: String,         // atime: YYYY-MM-DD HH:MM:SS
    pub ctime_s: String,         // ctime: YYYY-MM-DD HH:MM:SS
    pub birth_s: Option<String>, // birth: YYYY-MM-DD HH:MM:SS (if available)
    pub mode: u32,
}

pub struct RunDialog {
    pub input: Vec<char>, // full command line
    pub cursor: usize,    // char index
}

/// 機能ダイアログの 1 コマンド定義
pub struct FuncCmd {
    pub name: &'static str,    // コマンド名 e.g. "mv"
    pub args: &'static str,    // 引数表示 e.g. "<name>"
    pub desc_ja: &'static str,
    pub desc_en: &'static str,
}

/// 利用可能なコマンド一覧
pub static FUNC_CMDS: &[FuncCmd] = &[
    FuncCmd { name: "mv",   args: "<name>", desc_ja: "カーソルファイルをリネーム", desc_en: "rename cursor file" },
    FuncCmd { name: "q",    args: "",       desc_ja: "終了 (quit)",               desc_en: "quit" },
    FuncCmd { name: "help", args: "",       desc_ja: "コマンド一覧",               desc_en: "list commands" },
];

#[derive(Clone, Debug, Default)]
pub struct FuncDialog {
    pub input: Vec<char>, // コマンドライン全体 e.g. ['m','v',' ','f','o','o']
    pub cursor: usize,
    pub selected: usize,  // フィルタリスト内の選択位置
}

/// ファイル名検索の状態
#[derive(Clone, Debug)]
pub struct SearchState {
    pub input: Vec<char>,
    pub cursor: usize,
    pub origin: usize,      // 検索開始前のカーソル位置
    pub matches: Vec<usize>, // マッチしたエントリのインデックス
    pub match_idx: usize,    // matches の中の現在位置
}

/// push / pull / fetch のどれを実行するか
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteOp {
    Fetch,
    Push,
    Pull,
}

#[derive(Clone, Debug)]
pub enum GitDialogState {
    Menu,
    CommitMsg { input: Vec<char>, cursor: usize },
    SwitchBranch { input: Vec<char>, cursor: usize },
    /// stash push メッセージ入力（空可）
    StashMsg { input: Vec<char>, cursor: usize },
    /// SSH パスフレーズ入力（空 = SSHエージェント試行）
    Passphrase { op: RemoteOp, input: Vec<char>, cursor: usize },
}

#[derive(Clone, Debug)]
pub struct GitDialog {
    pub state: GitDialogState,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogKind {
    DeleteConfirm,
    Rename,
    Copy,
    Move,
    Mkdir,
    Attr,
    CopyNewName,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConflictChoice {
    IfNewer,
    Overwrite,
    Skip,
}

pub struct OverwritePrompt {
    pub dest: PathBuf,
    pub conflict: String,  // filename already existing at dest
    pub todo: Vec<String>, // remaining files after conflict
    pub is_move: bool,
    pub batch: Option<ConflictChoice>, // set by Shift+: apply to all remaining
}

pub struct FileDialog {
    pub kind: DialogKind,
    pub input: Vec<char>,
    pub cursor: usize,
    pub targets: Vec<String>,
    pub dest: Option<PathBuf>, // used by CopyNewName
    pub conflict_rename: bool, // true while C-rename sub-dialog is active
    pub error: Option<String>,
    pub overwrite: Option<OverwritePrompt>,
}

pub struct TreeNode {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub expanded: bool,
    pub has_children: bool,
}

pub struct App {
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub cursor: usize,
    pub tagged: Vec<bool>,
    pub col_mode: u8,
    pub quit: bool,
    pub tree_open: bool,
    pub tree_focus: bool,
    pub tree_cursor: usize,
    pub tree_offset: usize,
    pub tree_nodes: Vec<TreeNode>,
    pub free_bytes: u64,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub volume_info: String,
    pub file_type: String,
    pub owner_s: String,
    pub git_branch: Option<String>,
    pub git_ahead: u32,
    pub git_behind: u32,
    pub git_status: HashMap<String, char>,
    pub ls_colors: HashMap<String, Style>,
    pub ui_colors: UiColors,
    pub labels: &'static Labels,
    pub lang_en: bool,
    pub keymap: KeyMap,
    pub menu_items: Vec<(String, String)>,
    pub error_msg: Option<String>,
    pub success_msg: Option<String>,
    pub show_help: bool,
    pub run_dialog: Option<RunDialog>,
    pub func_dialog: Option<FuncDialog>,
    pub search: Option<SearchState>,
    pub last_search: String,
    pub git_dialog: Option<GitDialog>,
    pub git_running: bool,
    pub file_dialog: Option<FileDialog>,
    pub show_hidden: bool,
    pub pager: String,
    pub editor: String,
    pub ext_programs: HashMap<String, String>,
}

/// src の mtime が dst より新しいか
fn is_src_newer(src: &std::path::Path, dst: &std::path::Path) -> bool {
    match (src.metadata(), dst.metadata()) {
        (Ok(sm), Ok(dm)) => match (sm.modified(), dm.modified()) {
            (Ok(st), Ok(dt)) => st > dt,
            _ => false,
        },
        _ => false,
    }
}

impl App {
    pub fn first_list_entry_index(&self) -> usize {
        self.entries
            .iter()
            .position(|e| e.name != "..")
            .unwrap_or(0)
    }

    pub fn last_list_entry_index(&self) -> usize {
        self.entries.len().saturating_sub(1)
    }

    pub fn new(config: Config) -> Result<Self> {
        let current_dir = std::env::current_dir()?;
        let ls_src = if !config.colors.ls_colors.is_empty() {
            config.colors.ls_colors.clone()
        } else {
            std::env::var("LS_COLORS")
                .ok()
                .filter(|s| !s.is_empty())
                .or_else(|| std::env::var("LSCOLORS").ok().filter(|s| !s.is_empty()))
                .unwrap_or_default()
        };
        let ui_colors = UiColors::from_config(&config.colors);
        let labels: &'static Labels = if config.display.lang == "en" {
            &LABELS_EN
        } else {
            &LABELS_JA
        };
        let lang_en = config.display.lang == "en";

        let keymap = build_keymap(&config.keys);

        // [programs] から editor/pager を取り出し、残りを拡張子マップとして使う
        let mut programs = config.programs;
        let pager = programs
            .remove("pager")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| std::env::var("PAGER").unwrap_or_else(|_| "less".to_string()));
        let editor = programs
            .remove("editor")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string()));
        let ext_programs = programs;

        let menu_items = MENU_ACTIONS
            .iter()
            .map(|&(action, label_ja, label_en)| {
                let key = key_label(&keymap, action);
                let label = if lang_en { label_en } else { label_ja };
                (key, label.to_string())
            })
            .collect();

        let mut app = App {
            current_dir,
            entries: Vec::new(),
            cursor: 0,
            tagged: Vec::new(),
            col_mode: 2,
            quit: false,
            tree_open: false,
            tree_focus: false,
            tree_cursor: 0,
            tree_offset: 0,
            tree_nodes: Vec::new(),
            free_bytes: 0,
            total_bytes: 0,
            used_bytes: 0,
            volume_info: String::new(),
            file_type: String::new(),
            owner_s: String::new(),
            git_branch: None,
            git_ahead: 0,
            git_behind: 0,
            git_status: HashMap::new(),
            show_hidden: config.display.show_hidden,
            pager,
            editor,
            ext_programs,
            ls_colors: parse_file_colors(&ls_src),
            ui_colors,
            labels,
            lang_en,
            keymap,
            menu_items,
            error_msg: None,
            success_msg: None,
            show_help: false,
            run_dialog: None,
            func_dialog: None,
            search: None,
            last_search: String::new(),
            git_dialog: None,
            git_running: false,
            file_dialog: None,
        };
        app.load_entries()?;
        app.update_file_info();
        Ok(app)
    }

    pub fn load_entries(&mut self) -> Result<()> {
        self.entries.clear();
        if self.current_dir.parent().is_some() {
            let parent_mode = fs::metadata(&self.current_dir)
                .map(|m| {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode()
                })
                .unwrap_or(0);
            self.entries.push(FileEntry {
                name: "..".to_string(),
                is_dir: true,
                is_link: false,
                size: 0,
                blocks: 0,
                inode: 0,
                nlink: 0,
                uid: 0,
                gid: 0,
                date: String::new(),
                time_str: String::new(),
                time_full: String::new(),
                atime_s: String::new(),
                ctime_s: String::new(),
                birth_s: None,
                mode: parent_mode,
            });
        }
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.current_dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if !self.show_hidden && name.starts_with('.') {
                continue;
            }
            let link_meta = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let is_link = link_meta.file_type().is_symlink();
            let meta = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) if is_link => link_meta,
                Err(_) => continue,
            };
            let (date, time_str, time_full) = meta
                .modified()
                .ok()
                .and_then(|t| {
                    Some(fmt_datetime(
                        t.duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .ok()?
                            .as_secs(),
                    ))
                })
                .unwrap_or_else(|| {
                    (
                        "--/--/--".to_string(),
                        "--:--".to_string(),
                        "--:--:--".to_string(),
                    )
                });
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let mode = meta.permissions().mode();
            let inode = meta.ino();
            let nlink = meta.nlink();
            let uid = meta.uid();
            let gid = meta.gid();
            let blocks = meta.blocks();

            let atime_s = {
                let secs = meta.atime().max(0) as u64;
                let (d, _, t) = fmt_datetime(secs);
                format!("{} {}", d, t)
            };
            let ctime_s = {
                let secs = meta.ctime().max(0) as u64;
                let (d, _, t) = fmt_datetime(secs);
                format!("{} {}", d, t)
            };
            let birth_s = meta
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                .map(|d| {
                    let (date, _, tf) = fmt_datetime(d.as_secs());
                    format!("{} {}", date, tf)
                });

            let fe = FileEntry {
                name,
                is_dir: meta.is_dir(),
                is_link,
                size: meta.len(),
                blocks,
                inode,
                nlink,
                uid,
                gid,
                date,
                time_str,
                time_full,
                atime_s,
                ctime_s,
                birth_s,
                mode,
            };
            if meta.is_dir() {
                dirs.push(fe);
            } else {
                files.push(fe);
            }
        }
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.entries.extend(dirs);
        self.entries.extend(files);
        self.tagged = vec![false; self.entries.len()];
        let (tot, used, free) = disk_stats(&self.current_dir);
        self.total_bytes = tot;
        self.used_bytes = used;
        self.free_bytes = free;
        self.volume_info = get_volume_info(&self.current_dir);
        self.git_branch = get_git_branch(&self.current_dir);
        let (ahead, behind) = get_git_ahead_behind(&self.current_dir);
        self.git_ahead = ahead;
        self.git_behind = behind;
        self.git_status = get_git_status(&self.current_dir);
        Ok(())
    }

    pub fn update_file_info(&mut self) {
        match self.entries.get(self.cursor) {
            Some(e) => {
                self.owner_s = format!(
                    "{}:{}",
                    resolve_name("/etc/passwd", e.uid),
                    resolve_name("/etc/group", e.gid)
                );
                let path = self.current_dir.join(&e.name);
                self.file_type = get_file_info(&path);
            }
            None => {
                self.file_type = String::new();
                self.owner_s = String::new();
            }
        }
    }

    pub fn enter_dir(&mut self, name: &str) -> Result<()> {
        if name == ".." {
            if let Some(parent) = self.current_dir.parent() {
                let old = self
                    .current_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let prev = self.current_dir.clone();
                self.current_dir = parent.to_path_buf();
                if let Err(e) = self.load_entries() {
                    self.current_dir = prev; // 失敗時はロールバック
                    return Err(e);
                }
                if let Some(i) = self.entries.iter().position(|e| e.name == old) {
                    self.cursor = i;
                }
            }
        } else {
            let nd = self.current_dir.join(name);
            if nd.is_dir() {
                let prev = self.current_dir.clone();
                self.current_dir = nd;
                self.cursor = 0;
                if let Err(e) = self.load_entries() {
                    self.current_dir = prev; // 失敗時はロールバック
                    return Err(e);
                }
            }
        }
        self.update_file_info();
        Ok(())
    }

    /// パターン文字列でエントリ名を検索し、マッチしたインデックスのリストを返す。
    /// 正規表現として解釈を試み、失敗した場合は部分文字列一致にフォールバック。
    pub fn compute_search_matches(&self, pattern: &str) -> Vec<usize> {
        if pattern.is_empty() {
            return vec![];
        }
        use regex::RegexBuilder;
        let re = RegexBuilder::new(pattern).case_insensitive(true).build();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                if e.name == ".." { return false; }
                match &re {
                    Ok(r) => r.is_match(&e.name),
                    Err(_) => e.name.to_lowercase().contains(&pattern.to_lowercase()),
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// load_entries + update_file_info をまとめて実行し、失敗時はエラーメッセージをセット
    pub fn reload(&mut self) {
        match self.load_entries() {
            Ok(()) => self.update_file_info(),
            Err(e) => self.error_msg = Some(e.to_string()),
        }
    }

    pub fn cols(&self) -> usize {
        match self.col_mode {
            1 => 1,
            2 => 2,
            3 => 3,
            5 => 5,
            _ => 2,
        }
    }
    pub fn per_page(&self, lh: usize) -> usize {
        self.cols() * lh
    }
    pub fn current_page(&self, lh: usize) -> usize {
        let pp = self.per_page(lh);
        if pp == 0 { 0 } else { self.cursor / pp }
    }
    pub fn total_pages(&self, lh: usize) -> usize {
        let pp = self.per_page(lh);
        if pp == 0 {
            1
        } else {
            self.entries.len().max(1).div_ceil(pp)
        }
    }

    pub fn move_up(&mut self, lh: usize) {
        if lh == 0 || self.cursor == 0 {
            return;
        }
        let pp = self.per_page(lh);
        let ps = (self.cursor / pp) * pp;
        let col = (self.cursor - ps) / lh;
        let row = (self.cursor - ps) % lh;
        if row > 0 {
            self.cursor -= 1;
        } else if col > 0 {
            self.cursor = (ps + (col - 1) * lh + lh - 1).min(self.entries.len() - 1);
        } else if ps > 0 {
            self.cursor = ps - 1;
        }
        self.update_file_info();
    }

    pub fn move_down(&mut self, lh: usize) {
        if lh == 0 || self.cursor + 1 >= self.entries.len() {
            return;
        }
        let cols = self.cols();
        let pp = cols * lh;
        let ps = (self.cursor / pp) * pp;
        let col = (self.cursor - ps) / lh;
        let row = (self.cursor - ps) % lh;
        if row + 1 < lh {
            if self.cursor + 1 < self.entries.len() {
                self.cursor += 1;
            }
        } else if col + 1 < cols {
            let t = ps + (col + 1) * lh;
            if t < self.entries.len() {
                self.cursor = t;
            }
        } else {
            let t = ps + pp;
            if t < self.entries.len() {
                self.cursor = t;
            }
        }
        self.update_file_info();
    }

    pub fn move_left(&mut self, lh: usize) {
        let cols = self.cols();
        if lh == 0 || cols <= 1 {
            return;
        }
        let pp = cols * lh;
        let ps = (self.cursor / pp) * pp;
        let col = (self.cursor - ps) / lh;
        let row = (self.cursor - ps) % lh;
        if col > 0 {
            self.cursor = (ps + (col - 1) * lh + row).min(self.entries.len() - 1);
        } else if ps > 0 {
            let prev_ps = ps - pp;
            let t = prev_ps + (cols - 1) * lh + row;
            self.cursor = t.min(ps - 1);
        }
        self.update_file_info();
    }

    pub fn move_right(&mut self, lh: usize) {
        let cols = self.cols();
        if lh == 0 || cols <= 1 {
            return;
        }
        let pp = cols * lh;
        let ps = (self.cursor / pp) * pp;
        let col = (self.cursor - ps) / lh;
        let row = (self.cursor - ps) % lh;
        if col + 1 < cols {
            let t = ps + (col + 1) * lh + row;
            if t < self.entries.len() {
                self.cursor = t;
            }
        } else {
            let t = ps + pp;
            if t < self.entries.len() {
                self.cursor = t;
            }
        }
        self.update_file_info();
    }

    pub fn page_up(&mut self, lh: usize) {
        let pp = self.per_page(lh);
        if pp == 0 {
            return;
        }
        self.cursor = self.cursor.saturating_sub(pp);
        self.update_file_info();
    }

    pub fn page_down(&mut self, lh: usize) {
        let pp = self.per_page(lh);
        if pp == 0 {
            return;
        }
        let t = self.cursor + pp;
        self.cursor = if t < self.entries.len() {
            t
        } else {
            self.entries.len().saturating_sub(1)
        };
        self.update_file_info();
    }

    pub fn tag_toggle(&mut self) {
        if self.cursor < self.tagged.len()
            && self.entries.get(self.cursor).map(|e| e.name.as_str()) != Some("..")
        {
            self.tagged[self.cursor] ^= true;
        }
    }
    pub fn tag_toggle_move(&mut self, lh: usize) {
        self.tag_toggle();
        self.move_down(lh);
    }
    pub fn tag_all(&mut self) {
        let all = self
            .entries
            .iter()
            .zip(self.tagged.iter())
            .filter(|(e, _)| e.name != "..")
            .all(|(_, &t)| t);
        for (e, t) in self.entries.iter().zip(self.tagged.iter_mut()) {
            if e.name != ".." {
                *t = !all;
            }
        }
    }
    pub fn dir_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.is_dir && e.name != "..")
            .count()
    }
    pub fn file_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_dir).count()
    }
    pub fn current_total_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| !e.is_dir)
            .map(|e| e.size)
            .sum()
    }

    // ── Tree pane ──────────────────────────────────────────────────────

    pub fn tree_rebuild(&mut self) {
        let (nodes, cur) = tree_build(&self.current_dir, self.show_hidden);
        self.tree_nodes = nodes;
        self.tree_cursor = cur;
        self.tree_offset = 0;
    }

    pub fn tree_clamp_offset(&mut self, lh: usize) {
        if lh == 0 {
            return;
        }
        if self.tree_cursor < self.tree_offset {
            self.tree_offset = self.tree_cursor;
        } else if self.tree_cursor >= self.tree_offset + lh {
            self.tree_offset = self.tree_cursor + 1 - lh;
        }
    }

    pub fn tree_move_up(&mut self, lh: usize) {
        if self.tree_cursor == 0 {
            return;
        }
        self.tree_cursor -= 1;
        self.tree_clamp_offset(lh);
        let path = self.tree_nodes[self.tree_cursor].path.clone();
        self.current_dir = path;
        self.cursor = 0;
        let _ = self.load_entries();
        self.update_file_info();
    }

    pub fn tree_move_down(&mut self, lh: usize) {
        if self.tree_cursor + 1 >= self.tree_nodes.len() {
            return;
        }
        self.tree_cursor += 1;
        self.tree_clamp_offset(lh);
        let path = self.tree_nodes[self.tree_cursor].path.clone();
        self.current_dir = path;
        self.cursor = 0;
        let _ = self.load_entries();
        self.update_file_info();
    }

    pub fn tree_expand(&mut self) {
        if self.tree_cursor >= self.tree_nodes.len() {
            return;
        }
        let node = &self.tree_nodes[self.tree_cursor];
        if node.expanded || !node.has_children {
            return;
        }
        let path = node.path.clone();
        let depth = node.depth;
        self.tree_nodes[self.tree_cursor].expanded = true;
        let subdirs = tree_list_subdirs(&path, self.show_hidden);
        let insert_at = self.tree_cursor + 1;
        for (i, subpath) in subdirs.into_iter().enumerate() {
            let name = subpath
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let has_ch = tree_has_children(&subpath, self.show_hidden);
            self.tree_nodes.insert(
                insert_at + i,
                TreeNode {
                    path: subpath,
                    name,
                    depth: depth + 1,
                    expanded: false,
                    has_children: has_ch,
                },
            );
        }
    }

    pub fn tree_collapse(&mut self, lh: usize) {
        if self.tree_cursor >= self.tree_nodes.len() {
            return;
        }
        if self.tree_nodes[self.tree_cursor].expanded {
            let depth = self.tree_nodes[self.tree_cursor].depth;
            let start = self.tree_cursor + 1;
            let end = self.tree_nodes[start..]
                .iter()
                .position(|n| n.depth <= depth)
                .map(|p| start + p)
                .unwrap_or(self.tree_nodes.len());
            self.tree_nodes[self.tree_cursor].expanded = false;
            self.tree_nodes.drain(start..end);
        } else {
            let depth = self.tree_nodes[self.tree_cursor].depth;
            #[allow(clippy::collapsible_if)]
            if depth > 0 {
                if let Some(pi) = self.tree_nodes[..self.tree_cursor]
                    .iter()
                    .rposition(|n| n.depth < depth)
                {
                    self.tree_cursor = pi;
                    self.tree_clamp_offset(lh);
                    let path = self.tree_nodes[pi].path.clone();
                    self.current_dir = path;
                    self.cursor = 0;
                    let _ = self.load_entries();
                    self.update_file_info();
                }
            }
        }
    }

    // ── File operations ─────────────────────────────────────────────────

    /// Returns tagged filenames (excluding ".."), or the cursor entry if none tagged.
    pub fn collect_op_targets(&self) -> Vec<String> {
        let tagged: Vec<String> = self
            .entries
            .iter()
            .zip(self.tagged.iter())
            .filter(|(e, t)| **t && e.name != "..")
            .map(|(e, _)| e.name.clone())
            .collect();
        if !tagged.is_empty() {
            tagged
        } else if let Some(e) = self.entries.get(self.cursor) {
            if e.name != ".." {
                vec![e.name.clone()]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }

    pub fn resolve_dest(&self, s: &str) -> PathBuf {
        let p = PathBuf::from(s);
        if p.is_absolute() {
            p
        } else {
            self.current_dir.join(p)
        }
    }

    fn process_copy_seq(
        &self,
        files: &[String],
        dest: &std::path::Path,
        is_move: bool,
        batch: Option<ConflictChoice>,
    ) -> Result<Option<OverwritePrompt>> {
        for (i, fname) in files.iter().enumerate() {
            let src = self.current_dir.join(fname);
            let dst = dest.join(fname);
            if dst.exists() {
                match batch {
                    None => {
                        return Ok(Some(OverwritePrompt {
                            dest: dest.to_path_buf(),
                            conflict: fname.clone(),
                            todo: files[i + 1..].to_vec(),
                            is_move,
                            batch: None,
                        }));
                    }
                    Some(ConflictChoice::Overwrite) => {
                        if is_move {
                            move_path(&src, &dst)?;
                        } else {
                            copy_path(&src, &dst)?;
                        }
                    }
                    Some(ConflictChoice::IfNewer) => {
                        if is_src_newer(&src, &dst) {
                            if is_move {
                                move_path(&src, &dst)?;
                            } else {
                                copy_path(&src, &dst)?;
                            }
                        }
                    }
                    Some(ConflictChoice::Skip) => {}
                }
            } else if is_move {
                move_path(&src, &dst)?;
            } else {
                copy_path(&src, &dst)?;
            }
        }
        Ok(None)
    }

    pub fn begin_copy(
        &self,
        dest_str: &str,
        targets: &[String],
    ) -> Result<Option<OverwritePrompt>> {
        let dest = self.resolve_dest(dest_str);
        fs::create_dir_all(&dest)?;
        for fname in targets {
            let src = self.current_dir.join(fname);
            if src.is_dir() {
                let canon_src = src.canonicalize().unwrap_or_else(|_| src.clone());
                let canon_dest = dest.canonicalize().unwrap_or_else(|_| dest.clone());
                if canon_dest.starts_with(&canon_src) {
                    anyhow::bail!("コピー先がコピー元の配下です: {}", fname);
                }
            }
        }
        self.process_copy_seq(targets, &dest, false, None)
    }

    pub fn begin_move(
        &self,
        dest_str: &str,
        targets: &[String],
    ) -> Result<Option<OverwritePrompt>> {
        let dest = self.resolve_dest(dest_str);
        fs::create_dir_all(&dest)?;
        self.process_copy_seq(targets, &dest, true, None)
    }

    // ── Conflict resolution: apply choice to current conflict, continue ──

    fn resume_conflict(
        &self,
        prompt: OverwritePrompt,
        choice: ConflictChoice,
        batch: Option<ConflictChoice>,
    ) -> Result<Option<OverwritePrompt>> {
        let src = self.current_dir.join(&prompt.conflict);
        let dst = prompt.dest.join(&prompt.conflict);
        let do_op = match choice {
            ConflictChoice::IfNewer => is_src_newer(&src, &dst),
            ConflictChoice::Overwrite => true,
            ConflictChoice::Skip => false,
        };
        if do_op {
            if prompt.is_move {
                move_path(&src, &dst)?;
            } else {
                copy_path(&src, &dst)?;
            }
        }
        self.process_copy_seq(&prompt.todo, &prompt.dest, prompt.is_move, batch)
    }

    /// O: 上書き（一件）
    pub fn resume_overwrite(&self, prompt: OverwritePrompt) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(prompt, ConflictChoice::Overwrite, None)
    }
    /// U: タイムスタンプが新しい時のみ（一件）
    pub fn resume_if_newer(&self, prompt: OverwritePrompt) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(prompt, ConflictChoice::IfNewer, None)
    }
    /// N: スキップ（一件）
    pub fn resume_skip(&self, prompt: OverwritePrompt) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(prompt, ConflictChoice::Skip, None)
    }
    /// Shift+O: 上書き（以降全件）
    pub fn resume_overwrite_batch(
        &self,
        prompt: OverwritePrompt,
    ) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(
            prompt,
            ConflictChoice::Overwrite,
            Some(ConflictChoice::Overwrite),
        )
    }
    /// Shift+U: タイムスタンプが新しい時のみ（以降全件）
    pub fn resume_if_newer_batch(
        &self,
        prompt: OverwritePrompt,
    ) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(
            prompt,
            ConflictChoice::IfNewer,
            Some(ConflictChoice::IfNewer),
        )
    }
    /// Shift+N: スキップ（以降全件）
    pub fn resume_skip_batch(&self, prompt: OverwritePrompt) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(prompt, ConflictChoice::Skip, Some(ConflictChoice::Skip))
    }
    /// C: 名前を変更して複写/移動
    pub fn resume_rename(
        &self,
        new_name: &str,
        prompt: OverwritePrompt,
    ) -> Result<Option<OverwritePrompt>> {
        let n = new_name.trim();
        if n.is_empty() {
            anyhow::bail!("名前が空です");
        }
        let src = self.current_dir.join(&prompt.conflict);
        let dst = prompt.dest.join(n);
        if dst.exists() {
            anyhow::bail!("\"{}\" は既に存在します", n);
        }
        if prompt.is_move {
            move_path(&src, &dst)?;
        } else {
            copy_path(&src, &dst)?;
        }
        self.process_copy_seq(&prompt.todo, &prompt.dest, prompt.is_move, prompt.batch)
    }

    pub fn exec_delete(&self, targets: &[String]) -> Result<()> {
        for fname in targets {
            if fname == ".." {
                continue;
            }
            let path = self.current_dir.join(fname);
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
        Ok(())
    }

    pub fn exec_rename(&self, new_name: &str, targets: &[String]) -> Result<()> {
        let new = new_name.trim();
        if new.is_empty() {
            anyhow::bail!("名前が空です");
        }
        if let Some(fname) = targets.first().filter(|f| *f != "..") {
            fs::rename(self.current_dir.join(fname), self.current_dir.join(new))?;
        }
        Ok(())
    }

    pub fn exec_mkdir(&self, name: &str) -> Result<()> {
        let n = name.trim();
        if n.is_empty() {
            anyhow::bail!("名前が空です");
        }
        fs::create_dir_all(self.current_dir.join(n))?;
        Ok(())
    }

    /// 同ディレクトリへの複写: 新ファイル名を指定してコピー
    pub fn exec_copy_newname(
        &self,
        new_name: &str,
        targets: &[String],
        dest_dir: &Path,
    ) -> Result<()> {
        let n = new_name.trim();
        if n.is_empty() {
            anyhow::bail!("名前が空です");
        }
        let src = match targets.first() {
            Some(f) => self.current_dir.join(f),
            None => anyhow::bail!("複写元ファイルが指定されていません"),
        };
        let dst = dest_dir.join(n);
        if dst.exists() {
            anyhow::bail!("\"{}\" は既に存在します", n);
        }
        copy_path(&src, &dst)?;
        Ok(())
    }

    pub fn exec_attr(&self, perm_str: &str, targets: &[String]) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let s = perm_str.trim();
        if s.is_empty() {
            anyhow::bail!("権限が空です");
        }
        let mode =
            u32::from_str_radix(s, 8).map_err(|_| anyhow::anyhow!("無効なオクタル値: {}", s))?;
        for fname in targets {
            if fname == ".." {
                continue;
            }
            let path = self.current_dir.join(fname);
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
        }
        Ok(())
    }
}

// ── Tree build (standalone, not part of App) ──────────────────────────

fn add_tree_node(
    nodes: &mut Vec<TreeNode>,
    path: PathBuf,
    depth: usize,
    ancestors: &std::collections::HashSet<PathBuf>,
    show_hidden: bool,
) {
    let name = if depth == 0 {
        "/".to_string()
    } else {
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let expanded = ancestors.contains(&path);
    let has_children = tree_has_children(&path, show_hidden);
    nodes.push(TreeNode {
        path: path.clone(),
        name,
        depth,
        expanded,
        has_children,
    });
    if expanded {
        for subpath in tree_list_subdirs(&path, show_hidden) {
            add_tree_node(nodes, subpath, depth + 1, ancestors, show_hidden);
        }
    }
}

/// tree_build はテストから直接呼びやすいよう pub のままにしておく
pub fn tree_build(current_dir: &PathBuf, show_hidden: bool) -> (Vec<TreeNode>, usize) {
    let mut ancestors: Vec<PathBuf> = Vec::new();
    let mut p: &std::path::Path = current_dir.as_path();
    loop {
        ancestors.push(p.to_path_buf());
        match p.parent() {
            Some(parent) if parent != p => p = parent,
            _ => break,
        }
    }
    ancestors.reverse();
    let ancestor_set: std::collections::HashSet<PathBuf> = ancestors.into_iter().collect();
    let mut nodes = Vec::new();
    add_tree_node(
        &mut nodes,
        PathBuf::from("/"),
        0,
        &ancestor_set,
        show_hidden,
    );
    let cursor = nodes
        .iter()
        .position(|n| &n.path == current_dir)
        .unwrap_or(0);
    (nodes, cursor)
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ColorsConfig, LABELS_JA, UiColors};
    use tempfile::tempdir;

    /// テスト用の最小構成 App を指定ディレクトリで生成する。
    fn make_test_app(dir: PathBuf) -> App {
        let ui_colors = UiColors::from_config(&ColorsConfig::default());
        let keymap = build_keymap(&HashMap::new());
        let menu_items = MENU_ACTIONS
            .iter()
            .map(|&(action, label_ja, _)| (key_label(&keymap, action), label_ja.to_string()))
            .collect();
        let mut app = App {
            current_dir: dir,
            entries: Vec::new(),
            cursor: 0,
            tagged: Vec::new(),
            col_mode: 2,
            quit: false,
            tree_open: false,
            tree_focus: false,
            tree_cursor: 0,
            tree_offset: 0,
            tree_nodes: Vec::new(),
            free_bytes: 0,
            total_bytes: 0,
            used_bytes: 0,
            volume_info: String::new(),
            file_type: String::new(),
            owner_s: String::new(),
            git_branch: None,
            git_ahead: 0,
            git_behind: 0,
            git_status: HashMap::new(),
            ls_colors: HashMap::new(),
            ui_colors,
            labels: &LABELS_JA,
            lang_en: false,
            keymap,
            menu_items,
            error_msg: None,
            success_msg: None,
            show_help: false,
            run_dialog: None,
            func_dialog: None,
            search: None,
            last_search: String::new(),
            git_dialog: None,
            git_running: false,
            file_dialog: None,
            show_hidden: false,
            pager: "less".to_string(),
            editor: "nano".to_string(),
            ext_programs: HashMap::new(),
        };
        app.load_entries().unwrap();
        app.update_file_info();
        app
    }

    fn canonical(path: &std::path::Path) -> PathBuf {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    }

    // ── load_entries ───────────────────────────────────────────────────

    #[test]
    fn test_load_entries_empty_dir_has_dotdot() {
        let dir = tempdir().unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        // 空ディレクトリでも ".." エントリが必ず存在する
        assert_eq!(app.entries.len(), 1);
        assert_eq!(app.entries[0].name, "..");
        assert!(app.entries[0].is_dir);
    }

    #[test]
    fn test_load_entries_dirs_before_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("aaa.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("bbb")).unwrap();

        let app = make_test_app(dir.path().to_path_buf());
        // 順序: ".." → ディレクトリ → ファイル
        assert_eq!(app.entries[0].name, "..");
        assert_eq!(app.entries[1].name, "bbb");
        assert!(app.entries[1].is_dir);
        assert_eq!(app.entries[2].name, "aaa.txt");
    }

    #[test]
    fn test_load_entries_alphabetical_sort() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("charlie.txt"), "").unwrap();
        std::fs::write(dir.path().join("alpha.txt"), "").unwrap();
        std::fs::write(dir.path().join("bravo.txt"), "").unwrap();

        let app = make_test_app(dir.path().to_path_buf());
        let names: Vec<&str> = app
            .entries
            .iter()
            .skip(1)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha.txt", "bravo.txt", "charlie.txt"]);
    }

    #[test]
    fn test_load_entries_hides_dotfiles_by_default() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("visible.txt"), "").unwrap();
        std::fs::write(dir.path().join(".hidden"), "").unwrap();

        let app = make_test_app(dir.path().to_path_buf());
        let names: Vec<&str> = app.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"visible.txt"));
        assert!(!names.contains(&".hidden"));
    }

    #[test]
    fn test_load_entries_shows_dotfiles_when_enabled() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join(".hidden"), "").unwrap();

        let mut app = make_test_app(dir.path().to_path_buf());
        app.show_hidden = true;
        app.load_entries().unwrap();

        let names: Vec<&str> = app.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&".hidden"));
    }

    // ── file_count / dir_count / current_total_bytes ───────────────────

    #[test]
    fn test_file_and_dir_count() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("mydir")).unwrap();

        let app = make_test_app(dir.path().to_path_buf());
        assert_eq!(app.file_count(), 2);
        assert_eq!(app.dir_count(), 1); // ".." は除外される
    }

    #[test]
    fn test_current_total_bytes() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello").unwrap(); // 5 bytes
        std::fs::write(dir.path().join("b.txt"), "world!").unwrap(); // 6 bytes

        let app = make_test_app(dir.path().to_path_buf());
        assert_eq!(app.current_total_bytes(), 11);
    }

    // ── enter_dir ──────────────────────────────────────────────────────

    #[test]
    fn test_enter_subdir_resets_cursor() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        std::fs::write(dir.path().join("subdir").join("file.txt"), "hi").unwrap();

        let mut app = make_test_app(dir.path().to_path_buf());
        app.cursor = app.entries.iter().position(|e| e.name == "subdir").unwrap();
        app.enter_dir("subdir").unwrap();

        assert_eq!(app.current_dir, dir.path().join("subdir"));
        assert_eq!(app.cursor, 0);
        assert!(app.entries.iter().any(|e| e.name == "file.txt"));
    }

    #[test]
    fn test_enter_parent_dir_restores_cursor() {
        let dir = tempdir().unwrap();
        let child = dir.path().join("child");
        std::fs::create_dir(&child).unwrap();

        let mut app = make_test_app(child);
        app.enter_dir("..").unwrap();

        assert_eq!(app.current_dir, dir.path().to_path_buf());
        // カーソルが "child" エントリを指している
        let cur = &app.entries[app.cursor];
        assert_eq!(cur.name, "child");
    }

    // ── タグ操作 ───────────────────────────────────────────────────────

    #[test]
    fn test_tag_toggle() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("foo.txt"), "").unwrap();

        let mut app = make_test_app(dir.path().to_path_buf());
        // index 1 = "foo.txt" (.." が index 0)
        app.cursor = 1;
        assert!(!app.tagged[1]);
        app.tag_toggle();
        assert!(app.tagged[1]);
        app.tag_toggle();
        assert!(!app.tagged[1]);
    }

    #[test]
    fn test_tag_all_tags_and_untags() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();

        let mut app = make_test_app(dir.path().to_path_buf());
        // 全タグ OFF → 全タグ ON
        app.tag_all();
        assert!(!app.tagged[0]);
        assert!(app.tagged[1..].iter().all(|&t| t));
        // 全タグ ON → 全タグ OFF
        app.tag_all();
        assert!(app.tagged.iter().all(|&t| !t));
    }

    #[test]
    fn test_tag_toggle_move_advances_cursor() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();

        let mut app = make_test_app(dir.path().to_path_buf());
        app.cursor = 0;
        app.tag_toggle_move(10);
        assert!(!app.tagged[0]); // ".." はタグ対象外
        assert_eq!(app.cursor, 1); // カーソルが進む
    }

    #[test]
    fn test_tag_toggle_ignores_dotdot() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("foo.txt"), "").unwrap();

        let mut app = make_test_app(dir.path().to_path_buf());
        app.cursor = 0;
        app.tag_toggle();
        assert!(!app.tagged[0]);
    }

    // ── カーソル移動 ───────────────────────────────────────────────────

    #[test]
    fn test_move_down_and_up() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();

        let mut app = make_test_app(dir.path().to_path_buf()); // 3 entries: .., a, b
        assert_eq!(app.cursor, 0);
        app.move_down(10);
        assert_eq!(app.cursor, 1);
        app.move_down(10);
        assert_eq!(app.cursor, 2);
        app.move_down(10); // 末尾 → 動かない
        assert_eq!(app.cursor, 2);
        app.move_up(10);
        assert_eq!(app.cursor, 1);
        app.move_up(10);
        assert_eq!(app.cursor, 0);
        app.move_up(10); // 先頭 → 動かない
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_page_down_and_up() {
        let dir = tempdir().unwrap();
        for i in 0..9 {
            std::fs::write(dir.path().join(format!("{:02}.txt", i)), "").unwrap();
        }
        // エントリ数: ".." + 9ファイル = 10; col_mode=2, lh=3 → per_page=6
        let mut app = make_test_app(dir.path().to_path_buf());
        let lh = 3;
        assert_eq!(app.current_page(lh), 0);
        app.page_down(lh);
        assert_eq!(app.current_page(lh), 1);
        app.page_up(lh);
        assert_eq!(app.current_page(lh), 0);
    }

    #[test]
    fn test_first_and_last_page() {
        let dir = tempdir().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("{}.txt", i)), "").unwrap();
        }
        let mut app = make_test_app(dir.path().to_path_buf());
        let lh = 2;
        let last = app.entries.len() - 1;
        app.cursor = last;
        assert_eq!(app.current_page(lh), app.total_pages(lh) - 1);
        app.cursor = 0;
        assert_eq!(app.current_page(lh), 0);
    }

    #[test]
    fn test_first_list_entry_index_skips_dotdot() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();

        let app = make_test_app(dir.path().to_path_buf());
        assert_eq!(app.first_list_entry_index(), 1);
    }

    #[test]
    fn test_first_list_entry_index_at_root_is_zero() {
        let app = make_test_app(PathBuf::from("/"));
        assert_eq!(app.first_list_entry_index(), 0);
    }

    #[test]
    fn test_last_list_entry_index_points_to_last_entry() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();

        let app = make_test_app(dir.path().to_path_buf());
        assert_eq!(app.last_list_entry_index(), app.entries.len() - 1);
    }

    // ── cols / per_page ────────────────────────────────────────────────

    #[test]
    fn test_cols_and_per_page() {
        let dir = tempdir().unwrap();
        let mut app = make_test_app(dir.path().to_path_buf());

        app.col_mode = 1;
        assert_eq!(app.cols(), 1);
        assert_eq!(app.per_page(10), 10);
        app.col_mode = 2;
        assert_eq!(app.cols(), 2);
        assert_eq!(app.per_page(10), 20);
        app.col_mode = 3;
        assert_eq!(app.cols(), 3);
        assert_eq!(app.per_page(10), 30);
        app.col_mode = 5;
        assert_eq!(app.cols(), 5);
        assert_eq!(app.per_page(10), 50);
    }

    // ── tree_build ─────────────────────────────────────────────────────

    #[test]
    fn test_tree_build_root_node() {
        let dir = tempdir().unwrap();
        let dir = canonical(dir.path());
        // tempfile が作るディレクトリは /tmp/.tmpXXX のように隠しディレクトリに
        // なることがあるため show_hidden=true で確実に祖先パスを辿れるようにする
        let (nodes, cursor) = tree_build(&dir, true);
        assert_eq!(nodes[0].name, "/");
        assert_eq!(nodes[0].depth, 0);
        assert_eq!(nodes[cursor].path, dir);
    }

    // ── collect_op_targets ─────────────────────────────────────────────

    #[test]
    fn test_collect_op_targets_cursor() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        let mut app = make_test_app(dir.path().to_path_buf());
        app.cursor = 1; // "a.txt"
        let targets = app.collect_op_targets();
        assert_eq!(targets, vec!["a.txt"]);
    }

    #[test]
    fn test_collect_op_targets_tagged() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "").unwrap();
        let mut app = make_test_app(dir.path().to_path_buf());
        app.tagged[2] = true; // tag "b.txt"
        let targets = app.collect_op_targets();
        assert_eq!(targets, vec!["b.txt"]);
    }

    #[test]
    fn test_collect_op_targets_excludes_dotdot() {
        let dir = tempdir().unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        // cursor=0 is ".."
        let targets = app.collect_op_targets();
        assert!(targets.is_empty());
    }

    // ── exec_delete ────────────────────────────────────────────────────

    #[test]
    fn test_exec_delete_file() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("del.txt");
        std::fs::write(&f, "x").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        app.exec_delete(&["del.txt".to_string()]).unwrap();
        assert!(!f.exists());
    }

    #[test]
    fn test_exec_delete_dir_recursive() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("f.txt"), "x").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        app.exec_delete(&["sub".to_string()]).unwrap();
        assert!(!sub.exists());
    }

    #[test]
    fn test_exec_delete_permission_error_propagated() {
        use std::os::unix::fs::PermissionsExt;
        // root は権限を無視するのでスキップ
        let uid: u32 = std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        if uid == 0 {
            return;
        }

        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        let inner = sub.join("locked");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("file.txt"), "x").unwrap();
        // inner を mode 000 にして中を削除不可能にする
        std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o000)).unwrap();

        let app = make_test_app(dir.path().to_path_buf());
        let result = app.exec_delete(&["sub".to_string()]);

        // クリーンアップできるよう権限を戻す
        std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(result.is_err(), "権限エラーが伝播すること");
    }

    #[test]
    fn test_exec_delete_skips_dotdot() {
        let dir = tempdir().unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        // ".." should be silently skipped, not cause an error
        app.exec_delete(&["..".to_string()]).unwrap();
    }

    // ── exec_rename ────────────────────────────────────────────────────

    #[test]
    fn test_exec_rename() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "x").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        app.exec_rename("new.txt", &["old.txt".to_string()])
            .unwrap();
        assert!(!dir.path().join("old.txt").exists());
        assert!(dir.path().join("new.txt").exists());
    }

    #[test]
    fn test_exec_rename_empty_name_errors() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("foo.txt"), "").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        assert!(app.exec_rename("  ", &["foo.txt".to_string()]).is_err());
    }

    // ── exec_mkdir ─────────────────────────────────────────────────────

    #[test]
    fn test_exec_mkdir() {
        let dir = tempdir().unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        app.exec_mkdir("newdir").unwrap();
        assert!(dir.path().join("newdir").is_dir());
    }

    #[test]
    fn test_exec_mkdir_empty_name_errors() {
        let dir = tempdir().unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        assert!(app.exec_mkdir("").is_err());
    }

    // ── exec_attr ──────────────────────────────────────────────────────

    #[test]
    fn test_exec_attr_chmod() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let f = dir.path().join("f.txt");
        std::fs::write(&f, "x").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        app.exec_attr("644", &["f.txt".to_string()]).unwrap();
        let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644);
    }

    #[test]
    fn test_exec_attr_invalid_octal_errors() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), "").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        assert!(app.exec_attr("999", &["f.txt".to_string()]).is_err());
        assert!(app.exec_attr("xyz", &["f.txt".to_string()]).is_err());
    }

    // ── begin_copy / process_copy_seq ──────────────────────────────────

    #[test]
    fn test_begin_copy_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        let dst_dir = dir.path().join("dst");
        let app = make_test_app(dir.path().to_path_buf());
        let r = app
            .begin_copy(&dst_dir.to_string_lossy(), &["a.txt".to_string()])
            .unwrap();
        assert!(r.is_none()); // no conflict
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"hello");
        assert!(dir.path().join("a.txt").exists()); // original still there
    }

    #[test]
    fn test_begin_copy_dir_recursive() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("src_dir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("f.txt"), b"x").unwrap();
        let dst_dir = dir.path().join("dst");
        let app = make_test_app(dir.path().to_path_buf());
        let r = app
            .begin_copy(&dst_dir.to_string_lossy(), &["src_dir".to_string()])
            .unwrap();
        assert!(r.is_none());
        assert!(dst_dir.join("src_dir").join("f.txt").exists());
    }

    #[test]
    fn test_begin_copy_detects_conflict() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"src").unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("a.txt"), b"existing").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let r = app
            .begin_copy(&dst_dir.to_string_lossy(), &["a.txt".to_string()])
            .unwrap();
        assert!(r.is_some());
        let prompt = r.unwrap();
        assert_eq!(prompt.conflict, "a.txt");
        assert!(prompt.todo.is_empty());
        // existing file NOT overwritten yet
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"existing");
    }

    #[test]
    fn test_begin_copy_cyclic_guard() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        // copying "sub" into "sub/inner" would be cyclic
        let inner = sub.join("inner");
        let r = app.begin_copy(&inner.to_string_lossy(), &["sub".to_string()]);
        assert!(r.is_err());
    }

    // ── resume_overwrite / resume_if_newer / resume_skip / batch / rename ──

    fn make_conflict_prompt(
        dst_dir: &std::path::Path,
        conflict: &str,
        todo: Vec<String>,
        is_move: bool,
    ) -> OverwritePrompt {
        OverwritePrompt {
            dest: dst_dir.to_path_buf(),
            conflict: conflict.to_string(),
            todo,
            is_move,
            batch: None,
        }
    }

    #[test]
    fn test_resume_overwrite() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"new").unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("a.txt"), b"old").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
        let r = app.resume_overwrite(prompt).unwrap();
        assert!(r.is_none());
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"new");
    }

    #[test]
    fn test_resume_skip() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"new").unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("a.txt"), b"old").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
        let r = app.resume_skip(prompt).unwrap();
        assert!(r.is_none());
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"old"); // not overwritten
    }

    #[test]
    fn test_resume_overwrite_batch_applies_to_remaining() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"new_a").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"new_b").unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("a.txt"), b"old_a").unwrap();
        std::fs::write(dst_dir.join("b.txt"), b"old_b").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec!["b.txt".to_string()], false);
        let r = app.resume_overwrite_batch(prompt).unwrap();
        assert!(r.is_none()); // no more conflicts — batch processed b.txt automatically
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"new_a");
        assert_eq!(std::fs::read(dst_dir.join("b.txt")).unwrap(), b"new_b");
    }

    #[test]
    fn test_resume_skip_batch_applies_to_remaining() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"new_a").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"new_b").unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("a.txt"), b"old_a").unwrap();
        std::fs::write(dst_dir.join("b.txt"), b"old_b").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec!["b.txt".to_string()], false);
        let r = app.resume_skip_batch(prompt).unwrap();
        assert!(r.is_none()); // all skipped — no conflict returned
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"old_a"); // unchanged
        assert_eq!(std::fs::read(dst_dir.join("b.txt")).unwrap(), b"old_b"); // unchanged
    }

    #[test]
    fn test_resume_if_newer_overwrites_when_src_newer() {
        let dir = tempdir().unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        // write old dst first, then new src
        std::fs::write(dst_dir.join("a.txt"), b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dir.path().join("a.txt"), b"new").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
        let r = app.resume_if_newer(prompt).unwrap();
        assert!(r.is_none());
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"new");
    }

    #[test]
    fn test_resume_if_newer_skips_when_src_older() {
        let dir = tempdir().unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        // write new dst first, then old src
        std::fs::write(dir.path().join("a.txt"), b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dst_dir.join("a.txt"), b"newer").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
        let r = app.resume_if_newer(prompt).unwrap();
        assert!(r.is_none());
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"newer"); // not overwritten
    }

    #[test]
    fn test_resume_rename_copies_with_new_name() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("a.txt"), b"existing").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
        let r = app.resume_rename("a_copy.txt", prompt).unwrap();
        assert!(r.is_none());
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"existing"); // original untouched
        assert_eq!(std::fs::read(dst_dir.join("a_copy.txt")).unwrap(), b"data");
    }

    #[test]
    fn test_resume_rename_errors_if_new_name_exists() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("a.txt"), b"existing").unwrap();
        std::fs::write(dst_dir.join("b.txt"), b"also").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
        assert!(app.resume_rename("b.txt", prompt).is_err());
    }

    #[test]
    fn test_resume_skip_next_conflict_propagates() {
        // skipping first conflict still returns next conflict
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"new_a").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"new_b").unwrap();
        let dst_dir = dir.path().join("dst");
        std::fs::create_dir(&dst_dir).unwrap();
        std::fs::write(dst_dir.join("a.txt"), b"old_a").unwrap();
        std::fs::write(dst_dir.join("b.txt"), b"old_b").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec!["b.txt".to_string()], false);
        let r = app.resume_skip(prompt).unwrap();
        assert!(r.is_some()); // b.txt conflict returned
        assert_eq!(r.unwrap().conflict, "b.txt");
        assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"old_a"); // unchanged
    }

    // ── exec_copy_newname ──────────────────────────────────────────────

    #[test]
    fn test_exec_copy_newname_basic() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("orig.txt"), b"data").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        app.exec_copy_newname(
            "copy.txt",
            &["orig.txt".to_string()],
            &dir.path().to_path_buf(),
        )
        .unwrap();
        assert!(dir.path().join("orig.txt").exists()); // 元は残る
        assert_eq!(std::fs::read(dir.path().join("copy.txt")).unwrap(), b"data");
    }

    #[test]
    fn test_exec_copy_newname_same_name_errors() {
        // 同名を入力した場合（dst.exists()チェックで弾く）
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("orig.txt"), b"data").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let result = app.exec_copy_newname(
            "orig.txt",
            &["orig.txt".to_string()],
            &dir.path().to_path_buf(),
        );
        assert!(result.is_err(), "同名コピーはエラーになること");
        // 元ファイルが壊れていないこと
        assert_eq!(std::fs::read(dir.path().join("orig.txt")).unwrap(), b"data");
    }

    #[test]
    fn test_exec_copy_newname_dest_exists_errors() {
        // 新名前のファイルが既に存在する場合もエラー
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("src.txt"), b"src").unwrap();
        std::fs::write(dir.path().join("existing.txt"), b"existing").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        let result = app.exec_copy_newname(
            "existing.txt",
            &["src.txt".to_string()],
            &dir.path().to_path_buf(),
        );
        assert!(result.is_err());
        // 既存ファイルが上書きされていないこと
        assert_eq!(
            std::fs::read(dir.path().join("existing.txt")).unwrap(),
            b"existing"
        );
    }

    #[test]
    fn test_exec_copy_newname_empty_name_errors() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("orig.txt"), "").unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        assert!(
            app.exec_copy_newname("  ", &["orig.txt".to_string()], &dir.path().to_path_buf())
                .is_err()
        );
    }

    // ── begin_move ─────────────────────────────────────────────────────

    #[test]
    fn test_begin_move_file() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), b"data").unwrap();
        let dst_dir = dir.path().join("dst");
        let app = make_test_app(dir.path().to_path_buf());
        let r = app
            .begin_move(&dst_dir.to_string_lossy(), &["f.txt".to_string()])
            .unwrap();
        assert!(r.is_none());
        assert!(!dir.path().join("f.txt").exists());
        assert_eq!(std::fs::read(dst_dir.join("f.txt")).unwrap(), b"data");
    }

    // ── permission error handling ───────────────────────────────────────

    #[test]
    fn test_enter_dir_permission_denied_rolls_back() {
        // root はパーミッション制限を無視するためスキップ
        let is_root = std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u32>().ok())
            == Some(0);
        if is_root {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let sub = dir.path().join("locked");
        std::fs::create_dir(&sub).unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000)).unwrap();

        let mut app = make_test_app(dir.path().to_path_buf());
        let original_dir = app.current_dir.clone();

        let result = app.enter_dir("locked");
        // パーミッションを戻してから assert（後片付け保証）
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(
            result.is_err(),
            "enter_dir should fail on permission-denied directory"
        );
        assert_eq!(
            app.current_dir, original_dir,
            "current_dir should be rolled back on failure"
        );
    }

    #[test]
    fn test_reload_sets_error_msg_on_invalid_dir() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();

        let mut app = make_test_app(sub.clone());
        assert!(app.error_msg.is_none());

        std::fs::remove_dir(&sub).unwrap();
        app.reload();

        assert!(
            app.error_msg.is_some(),
            "reload() should set error_msg when directory no longer exists"
        );
    }

    #[test]
    fn test_tree_rebuild_updates_nodes() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let dir = canonical(dir.path());

        let mut app = make_test_app(dir.clone());
        app.show_hidden = true; // 同上: 隠しパスを確実に辿るため
        assert!(app.tree_nodes.is_empty());
        app.tree_rebuild();
        assert!(!app.tree_nodes.is_empty());
        assert_eq!(app.tree_nodes[app.tree_cursor].path, dir);
    }

    #[test]
    fn test_system_symlink_dirs_are_enterable() {
        for name in ["tmp", "etc"] {
            let path = PathBuf::from("/").join(name);
            if !path.exists()
                || !std::fs::symlink_metadata(&path)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
            {
                continue;
            }

            let mut app = make_test_app(PathBuf::from("/"));
            let idx = app.entries.iter().position(|e| e.name == name).unwrap();
            assert!(
                app.entries[idx].is_link,
                "{} should be recognized as symlink",
                name
            );
            assert!(
                app.entries[idx].is_dir,
                "{} should be treated as enterable directory",
                name
            );

            app.enter_dir(name).unwrap();
            assert_eq!(app.current_dir, PathBuf::from("/").join(name));
            assert!(
                !app.entries.is_empty(),
                "{} target should be readable after enter",
                name
            );
        }
    }

    // ── GitDialog / GitDialogState ─────────────────────────────────────

    #[test]
    fn test_git_dialog_stash_msg_initial_state() {
        let state = GitDialogState::StashMsg { input: vec![], cursor: 0 };
        if let GitDialogState::StashMsg { input, cursor } = state {
            assert!(input.is_empty());
            assert_eq!(cursor, 0);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_git_dialog_stash_msg_with_content() {
        let chars: Vec<char> = "wip".chars().collect();
        let state = GitDialogState::StashMsg { input: chars.clone(), cursor: 3 };
        if let GitDialogState::StashMsg { input, cursor } = state {
            assert_eq!(input, chars);
            assert_eq!(cursor, 3);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn test_git_dialog_none_by_default() {
        let dir = tempdir().unwrap();
        let app = make_test_app(dir.path().to_path_buf());
        assert!(app.git_dialog.is_none());
    }

    #[test]
    fn test_git_dialog_open_sets_menu_state() {
        let dir = tempdir().unwrap();
        let mut app = make_test_app(dir.path().to_path_buf());
        app.git_dialog = Some(GitDialog { state: GitDialogState::Menu });
        assert!(matches!(
            app.git_dialog.as_ref().unwrap().state,
            GitDialogState::Menu
        ));
    }

    #[test]
    fn test_git_dialog_transition_to_stash_msg() {
        let dir = tempdir().unwrap();
        let mut app = make_test_app(dir.path().to_path_buf());
        app.git_dialog = Some(GitDialog {
            state: GitDialogState::StashMsg { input: vec![], cursor: 0 },
        });
        assert!(matches!(
            app.git_dialog.as_ref().unwrap().state,
            GitDialogState::StashMsg { .. }
        ));
    }
}
