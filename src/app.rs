use anyhow::Result;
use ratatui::style::Style;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
};

use crate::config::{
    Config, KeyMap, LABELS_EN, LABELS_JA, Labels, MENU_ACTIONS, UiColors, build_keymap, key_label,
    parse_file_colors,
};
use crate::fs_utils::{
    disk_stats, fmt_datetime, get_file_info, get_git_ahead_behind, get_git_branch,
    get_git_status, get_volume_info, resolve_name, tree_has_children, tree_list_subdirs,
};

mod ops;
pub use ops::tree_build;

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
    FuncCmd { name: "mv",    args: "<name>", desc_ja: "カーソルファイルをリネーム", desc_en: "rename cursor file" },
    FuncCmd { name: "mkdir", args: "<name>", desc_ja: "ディレクトリ作成",           desc_en: "make directory" },
    FuncCmd { name: "proc",  args: "",       desc_ja: "プロセス一覧",               desc_en: "process viewer" },
    FuncCmd { name: "q",     args: "",       desc_ja: "終了 (quit)",               desc_en: "quit" },
    FuncCmd { name: "help",  args: "",       desc_ja: "コマンド一覧",               desc_en: "list commands" },
];

#[derive(Clone, Debug, Default)]
pub struct FuncDialog {
    pub input: Vec<char>, // コマンドライン全体 e.g. ['m','v',' ','f','o','o']
    pub cursor: usize,
    pub selected: usize,  // フィルタリスト内の選択位置
}

/// ディレクトリジャンプダイアログの状態
#[derive(Clone, Debug, Default)]
pub struct DirJumpDialog {
    pub input: Vec<char>,
    pub cursor: usize,
    pub error: Option<String>,
}

/// ファイル名検索の状態
#[derive(Clone, Debug)]
pub struct SearchState {
    pub input: Vec<char>,
    pub cursor: usize,
    pub origin: usize,       // 検索開始前のカーソル位置
    pub matches: Vec<usize>, // マッチしたエントリのインデックス
    pub match_idx: usize,    // matches の中の現在位置
    pub confirmed: bool,     // Enter で確定済み（バーは閉じるがハイライトは維持）
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortMode {
    None,
    Name,
    Ext,
    Size,
    Date,
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
    pub cursor: usize,     // 0=U 1=O 2=C 3=N (j/k/Enter で選択)
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
    pub sort_mode: SortMode,
    pub sort_asc: bool,
    pub show_sort_dialog: bool,
    pub sort_cursor: usize,
    pub git_menu_cursor: usize, // Git メニュー内のカーソル位置 (0-9)
    pub error_msg: Option<String>,
    pub success_msg: Option<String>,
    pub show_help: bool,
    pub run_dialog: Option<RunDialog>,
    pub func_dialog: Option<FuncDialog>,
    pub dir_jump_dialog: Option<DirJumpDialog>,
    pub search: Option<SearchState>,
    pub last_search: String,
    pub git_dialog: Option<GitDialog>,
    pub git_running: bool,
    pub file_dialog: Option<FileDialog>,
    pub show_hidden: bool,
    pub pager: String,
    pub editor: String,
    pub ext_programs: HashMap<String, String>,
    // ── proc モード ──────────────────────────────────────────────────────
    pub proc_mode: bool,
    pub proc_entries: Vec<crate::proc::ProcEntry>,
    pub proc_cursor: usize,
    pub proc_offset: usize,
    pub proc_sort: crate::proc::ProcSortMode,
    pub proc_sort_asc: bool,
    pub proc_signal_menu: Option<crate::proc::ProcSignalMenu>,
    pub proc_detail: crate::proc::ProcDetail,
    pub proc_tree: bool,
    pub proc_tree_rows: Vec<crate::proc::ProcTreeRow>,
    // ── 右パネル ─────────────────────────────────────────────────────────
    pub right_panel: crate::proc::RightPanel,
    pub right_panel_focus: bool,
    pub thread_entries: Vec<crate::proc::ThreadEntry>,
    pub thread_error: Option<String>,
    pub thread_cursor: usize,
    pub thread_offset: usize,
    pub thread_pid: u32,
    pub fd_pid: u32,
    pub fd_proc_name: String,
    pub fd_entries: Vec<crate::proc::FdEntry>,
    pub fd_error: Option<String>,
    pub fd_cursor: usize,
    pub fd_offset: usize,
    // ── root 警告 ─────────────────────────────────────────────────────────
    pub is_root: bool,
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
            sort_mode: SortMode::None,
            sort_asc: true,
            show_sort_dialog: false,
            sort_cursor: 0,
            git_menu_cursor: 0,
            error_msg: None,
            success_msg: None,
            show_help: false,
            run_dialog: None,
            func_dialog: None,
            dir_jump_dialog: None,
            search: None,
            last_search: String::new(),
            git_dialog: None,
            git_running: false,
            file_dialog: None,
            proc_mode: false,
            proc_entries: Vec::new(),
            proc_cursor: 0,
            proc_offset: 0,
            proc_sort: crate::proc::ProcSortMode::Cpu,
            proc_sort_asc: false,
            proc_signal_menu: None,
            proc_detail: crate::proc::ProcDetail::default(),
            proc_tree: false,
            proc_tree_rows: Vec::new(),
            right_panel: crate::proc::RightPanel::None,
            right_panel_focus: false,
            thread_entries: Vec::new(),
            thread_error: None,
            thread_cursor: 0,
            thread_offset: 0,
            thread_pid: 0,
            fd_pid: 0,
            fd_proc_name: String::new(),
            fd_entries: Vec::new(),
            fd_error: None,
            fd_cursor: 0,
            fd_offset: 0,
            is_root: unsafe { libc::geteuid() } == 0,
        };
        app.load_entries()?;
        app.update_file_info();
        Ok(app)
    }

    pub fn load_entries(&mut self) -> Result<()> {
        self.search = None;
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
        // ソートモードに従って dirs / files をそれぞれ並べ替え
        let sort_mode = self.sort_mode;
        let sort_asc = self.sort_asc;
        let sort_fn = |a: &FileEntry, b: &FileEntry| -> std::cmp::Ordering {
            let ord = match sort_mode {
                SortMode::None | SortMode::Name => {
                    a.name.to_lowercase().cmp(&b.name.to_lowercase())
                }
                SortMode::Ext => {
                    let ea = std::path::Path::new(&a.name)
                        .extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    let eb = std::path::Path::new(&b.name)
                        .extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    ea.cmp(&eb).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
                SortMode::Size => a.size.cmp(&b.size),
                SortMode::Date => a.date.cmp(&b.date).then_with(|| a.time_str.cmp(&b.time_str)),
            };
            if sort_asc { ord } else { ord.reverse() }
        };
        dirs.sort_by(sort_fn);
        files.sort_by(sort_fn);
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

    /// 絶対パスで直接ディレクトリに移動する（ディレクトリジャンプ用）。
    pub fn enter_dir_abs(&mut self, path: &std::path::Path) -> Result<()> {
        let canonical = path.canonicalize()
            .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?;
        if !canonical.is_dir() {
            return Err(anyhow::anyhow!("ディレクトリではありません: {}", canonical.display()));
        }
        let prev = self.current_dir.clone();
        self.current_dir = canonical;
        self.cursor = 0;
        if let Err(e) = self.load_entries() {
            self.current_dir = prev;
            return Err(e);
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

}

#[cfg(test)]
mod tests;
