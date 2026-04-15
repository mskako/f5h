use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use std::{collections::HashMap, fs, path::PathBuf};

// ── Config ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub display: DisplayConfig,
    pub keys: HashMap<String, String>,
    pub colors: ColorsConfig,
    /// editor/pager + 拡張子ごとのプログラムをまとめて管理
    /// editor = "vim", pager = "less", pdf = "evince", ...
    pub programs: HashMap<String, String>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct DisplayConfig {
    pub show_hidden: bool,
    /// "en" for English labels; any other value (or empty) = Japanese
    pub lang: String,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct ColorsConfig {
    pub ls_colors: String,
    /// 枠の罫線 (╭─│┴╰…)  デフォルト: cyan
    pub border: String,
    /// セクションタイトル ("ファイル情報" など)  デフォルト: yellow
    pub title: String,
    /// フィールドラベル (インフォメーション・タイトル)  デフォルト: cyan
    pub label: String,
    /// 単位 (Gi / Ki / %)  デフォルト: cyan
    pub unit: String,
    /// 日付・時刻 (ファイルのタイムスタンプ)  デフォルト: white
    pub date: String,
    /// 現在日時 (右上の時計)  デフォルト: white
    pub clock: String,
}

pub fn parse_color(s: &str) -> Color {
    match s.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "darkgray" | "dark_gray" => Color::DarkGray,
        "lightred" | "light_red" => Color::LightRed,
        "lightgreen" | "light_green" => Color::LightGreen,
        "lightyellow" | "light_yellow" => Color::LightYellow,
        "lightblue" | "light_blue" => Color::LightBlue,
        "lightmagenta" | "light_magenta" => Color::LightMagenta,
        "lightcyan" | "light_cyan" => Color::LightCyan,
        "lightgray" | "light_gray" | "gray" => Color::Gray,
        _ => Color::Reset,
    }
}

#[derive(Clone, Copy)]
pub struct UiColors {
    pub border: Style,
    pub title: Style,
    pub label: Style,
    pub unit: Style,
    pub date: Style,
    pub clock: Style,
}

impl UiColors {
    pub fn from_config(c: &ColorsConfig) -> Self {
        let mk = |s: &str, default: Color| {
            if s.is_empty() {
                Style::default().fg(default)
            } else {
                Style::default().fg(parse_color(s))
            }
        };
        UiColors {
            border: mk(&c.border, Color::Cyan),
            title: mk(&c.title, Color::Yellow),
            label: mk(&c.label, Color::Cyan),
            unit: mk(&c.unit, Color::Cyan),
            date: mk(&c.date, Color::White),
            clock: mk(&c.clock, Color::Yellow),
        }
    }
}

// ── Locale labels ─────────────────────────────────────────────────────

pub struct Labels {
    pub vol_info: &'static str,
    pub path_lbl: &'static str,
    pub free: &'static str,
    pub file_info: &'static str,
    pub curr: &'static str,
    pub count_unit: &'static str,
    pub file_lbl: &'static str,
    pub total: &'static str,
    pub kind: &'static str,
    pub used: &'static str,
    pub size_lbl: &'static str,
    pub mtime: &'static str,
    pub ctime: &'static str,
    pub usage: &'static str,
    pub perm: &'static str,
    pub own: &'static str,
    pub birth: &'static str,
    pub atime: &'static str,
    pub f1_help: &'static str,
    pub page_unit: &'static str,
}

pub static LABELS_JA: Labels = Labels {
    vol_info: "ボリューム情報",
    path_lbl: "パス",
    free: " 空き:",
    file_info: "ファイル情報",
    curr: " カレント:",
    count_unit: "個",
    file_lbl: " ファイル: ",
    total: " 合計:",
    kind: " 種別: ",
    used: " 使用中:",
    size_lbl: " サイズ: ",
    mtime: "  修正: ",
    ctime: "  変更: ",
    usage: " 使用率:",
    perm: " 権限: ",
    own: "  所有: ",
    birth: "  作成: ",
    atime: "  参照: ",
    f1_help: "F1:ヘルプ",
    page_unit: "頁",
};

pub static LABELS_EN: Labels = Labels {
    vol_info: "Volume Info",
    path_lbl: "Path",
    free: " Free:",
    file_info: "File Info",
    curr: " Curr:",
    count_unit: "Fs",
    file_lbl: " File: ",
    total: " Total:",
    kind: " Type: ",
    used: " Used:",
    size_lbl: " Size: ",
    mtime: "  Mod: ",
    ctime: "  Chg: ",
    usage: " Usage:",
    perm: " Perm: ",
    own: "  Own: ",
    birth: "  Bth: ",
    atime: "  Acc: ",
    f1_help: "F1:HELP",
    page_unit: "Page",
};

pub fn load_config() -> Config {
    let path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".config/f5h/config.toml");
    match fs::read_to_string(&path) {
        Ok(s) => match toml::from_str(&s) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("warning: failed to parse {}: {}", path.display(), e);
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

// ── Actions / Keymap ──────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    FirstEntry,
    LastEntry,
    PageUp,
    PageDown,
    Enter,
    ParentDir,
    HomeDir,
    TagMove,
    TagAll,
    Quit,
    ColMode1,
    ColMode2,
    ColMode3,
    ColMode5,
    Mkdir,
    DirJump,
    TreeToggle,
    Edit,
    Run,
    Func,
    Git,
    Copy,
    Move,
    Delete,
    Rename,
    Attr,
    Search,
    SearchNext,
    SearchPrev,
}

pub fn action_from_str(s: &str) -> Option<Action> {
    match s {
        "move_up" => Some(Action::MoveUp),
        "move_down" => Some(Action::MoveDown),
        "move_left" => Some(Action::MoveLeft),
        "move_right" => Some(Action::MoveRight),
        "first_entry" => Some(Action::FirstEntry),
        "last_entry" => Some(Action::LastEntry),
        "page_up" => Some(Action::PageUp),
        "page_down" => Some(Action::PageDown),
        "enter" => Some(Action::Enter),
        "parent_dir" => Some(Action::ParentDir),
        "home_dir" => Some(Action::HomeDir),
        "tag_move" => Some(Action::TagMove),
        "tag_all" => Some(Action::TagAll),
        "quit" => Some(Action::Quit),
        "col_mode_1" => Some(Action::ColMode1),
        "col_mode_2" => Some(Action::ColMode2),
        "col_mode_3" => Some(Action::ColMode3),
        "col_mode_5" => Some(Action::ColMode5),
        "mkdir" => Some(Action::Mkdir),
        "dir_jump" => Some(Action::DirJump),
        "tree_toggle" => Some(Action::TreeToggle),
        "edit" => Some(Action::Edit),
        "run" => Some(Action::Run),
        "func" => Some(Action::Func),
        "macro" => Some(Action::Func), // 後方互換
        "git" => Some(Action::Git),
        "copy" => Some(Action::Copy),
        "move" => Some(Action::Move),
        "delete" => Some(Action::Delete),
        "rename" => Some(Action::Rename),
        "attr" => Some(Action::Attr),
        "search" => Some(Action::Search),
        "search_next" => Some(Action::SearchNext),
        "search_prev" => Some(Action::SearchPrev),
        _ => None,
    }
}

pub fn parse_key_str(s: &str) -> Option<(KeyCode, KeyModifiers)> {
    let (mods, key_s) = if let Some(r) = s.strip_prefix("Ctrl+") {
        (KeyModifiers::CONTROL, r)
    } else if let Some(r) = s.strip_prefix("Shift+") {
        (KeyModifiers::SHIFT, r)
    } else if let Some(r) = s.strip_prefix("Alt+") {
        (KeyModifiers::ALT, r)
    } else {
        (KeyModifiers::NONE, s)
    };
    let code = match key_s {
        "Enter" => KeyCode::Enter,
        "Backspace" => KeyCode::Backspace,
        "Tab" => KeyCode::Tab,
        "Esc" => KeyCode::Esc,
        "Space" => KeyCode::Char(' '),
        "Up" => KeyCode::Up,
        "Down" => KeyCode::Down,
        "Left" => KeyCode::Left,
        "Right" => KeyCode::Right,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "Delete" => KeyCode::Delete,
        "Insert" => KeyCode::Insert,
        "F1" => KeyCode::F(1),
        "F2" => KeyCode::F(2),
        "F3" => KeyCode::F(3),
        "F4" => KeyCode::F(4),
        "F5" => KeyCode::F(5),
        "F6" => KeyCode::F(6),
        "F7" => KeyCode::F(7),
        "F8" => KeyCode::F(8),
        "F9" => KeyCode::F(9),
        "F10" => KeyCode::F(10),
        "F11" => KeyCode::F(11),
        "F12" => KeyCode::F(12),
        c if c.chars().count() == 1 => KeyCode::Char(c.chars().next().unwrap()),
        _ => return None,
    };
    Some((code, mods))
}

pub type KeyMap = HashMap<(KeyCode, KeyModifiers), Action>;

pub fn lookup_action(keymap: &KeyMap, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
    if let Some(&action) = keymap.get(&(code, modifiers)) {
        return Some(action);
    }

    if modifiers == KeyModifiers::SHIFT {
        let KeyCode::Char(c) = code else { return None; };
        if let Some(&action) = keymap.get(&(KeyCode::Char(c), KeyModifiers::NONE)) {
            return Some(action);
        }
        if c.is_ascii_uppercase() {
            if let Some(&action) =
                keymap.get(&(KeyCode::Char(c.to_ascii_lowercase()), KeyModifiers::SHIFT))
            {
                return Some(action);
            }
        }
    }

    None
}

pub fn build_keymap(cfg_keys: &HashMap<String, String>) -> KeyMap {
    let defaults: &[(&str, &str)] = &[
        ("move_up", "k"),
        ("move_down", "j"),
        ("move_left", "h"),
        ("move_right", "l"),
        ("first_entry", "g"),
        ("last_entry", "G"),
        ("page_up", "PageUp"),
        ("page_down", "PageDown"),
        ("enter", "Enter"),
        ("parent_dir", "Backspace"),
        ("home_dir", "~"),
        ("tag_move", "Space"),
        ("tag_all", "Home"),
        ("quit", "q"),
        ("col_mode_1", "!"),
        ("col_mode_2", "@"),
        ("col_mode_3", "#"),
        ("col_mode_5", "%"),
        ("mkdir", "K"),
        ("dir_jump", "J"),
        ("tree_toggle", "t"),
        ("edit", "e"),
        ("run", "x"),
        ("func", ":"),
        ("git", "b"),
        ("copy", "c"),
        ("move", "m"),
        ("delete", "d"),
        ("attr", "a"),
        ("search", "/"),
        ("search_next", "n"),
        ("search_prev", "N"),
    ];
    let mut map = KeyMap::new();
    for &(action_str, key_str) in defaults {
        if let (Some(action), Some(key)) = (action_from_str(action_str), parse_key_str(key_str)) {
            map.insert(key, action);
        }
    }
    for (action_str, key_str) in cfg_keys {
        if let (Some(action), Some(key)) = (action_from_str(action_str), parse_key_str(key_str)) {
            map.retain(|_, v| *v != action);
            map.insert(key, action);
        }
    }
    // Arrow keys are always movement aliases and cannot be overridden
    map.insert((KeyCode::Up, KeyModifiers::NONE), Action::MoveUp);
    map.insert((KeyCode::Down, KeyModifiers::NONE), Action::MoveDown);
    map.insert((KeyCode::Left, KeyModifiers::NONE), Action::MoveLeft);
    map.insert((KeyCode::Right, KeyModifiers::NONE), Action::MoveRight);
    map
}

/// メニューバーに表示するアクションとラベルの定義（順番がそのまま表示順）
pub static MENU_ACTIONS: &[(Action, &str, &str)] = &[
    (Action::Edit, "編集", "Edit"),
    (Action::Run, "実行", "Run"),
    (Action::Func, "機能", "Func"),
    (Action::Git, "Git", "Git"),
    (Action::TreeToggle, "ツリー", "Tree"),
    (Action::Search, "検索", "Search"),
    (Action::HomeDir, "HOME", "Home"),
    (Action::Copy, "複写", "Copy"),
    (Action::Move, "移動", "Move"),
    (Action::Delete, "削除", "Del"),
    (Action::Attr, "権限", "Perm"),
    (Action::Quit, "終了", "Quit"),
];

/// KeyMap からアクションに対応するキー表示文字列を逆引きする
pub fn key_label(keymap: &KeyMap, action: Action) -> String {
    for (&(code, mods), &act) in keymap {
        if act != action || mods != KeyModifiers::NONE {
            continue;
        }
        match code {
            // 矢印キーはナビゲーション専用のため除外
            KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => continue,
            KeyCode::Char(c) => return c.to_string(),
            KeyCode::F(n) => return format!("F{}", n),
            KeyCode::Enter => return "RET".to_string(),
            KeyCode::Tab => return "TAB".to_string(),
            KeyCode::Esc => return "ESC".to_string(),
            _ => return "?".to_string(),
        }
    }
    "?".to_string()
}

// ── LS_COLORS parser ──────────────────────────────────────────────────

pub fn parse_ansi(codes: &str) -> Style {
    let mut style = Style::default();
    let mut bold = false;
    let mut fg_code: Option<u32> = None;
    for part in codes.split(';') {
        match part.trim().parse::<u32>().unwrap_or(999) {
            0 => { style = Style::default(); bold = false; fg_code = None; }
            1 => { style = style.add_modifier(Modifier::BOLD); bold = true; }
            2 => style = style.add_modifier(Modifier::DIM),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            n @ 30..=37 => { style = style.fg(ansi_fg(n, false)); fg_code = Some(n); }
            90 => style = style.fg(Color::DarkGray),
            91 => style = style.fg(Color::LightRed),
            92 => style = style.fg(Color::LightGreen),
            93 => style = style.fg(Color::LightYellow),
            94 => style = style.fg(Color::LightBlue),
            95 => style = style.fg(Color::LightMagenta),
            96 => style = style.fg(Color::LightCyan),
            97 => style = style.fg(Color::Gray),
            40 => style = style.bg(Color::Black),
            41 => style = style.bg(Color::Red),
            42 => style = style.bg(Color::Green),
            43 => style = style.bg(Color::Yellow),
            44 => style = style.bg(Color::Blue),
            45 => style = style.bg(Color::Magenta),
            46 => style = style.bg(Color::Cyan),
            47 => style = style.bg(Color::White),
            _ => {}
        }
    }
    // Many terminals render bold + standard color (30-37) as the bright variant.
    // Replicate that convention so colors match `ls` output.
    if bold {
        if let Some(n) = fg_code {
            style = style.fg(ansi_fg(n, true));
        }
    }
    style
}

fn ansi_fg(code: u32, bright: bool) -> Color {
    match (code, bright) {
        (30, false) => Color::Black,
        (31, false) => Color::Red,
        (32, false) => Color::Green,
        (33, false) => Color::Yellow,
        (34, false) => Color::Blue,
        (35, false) => Color::Magenta,
        (36, false) => Color::Cyan,
        (37, false) => Color::White,
        (30, true) => Color::DarkGray,
        (31, true) => Color::LightRed,
        (32, true) => Color::LightGreen,
        (33, true) => Color::LightYellow,
        (34, true) => Color::LightBlue,
        (35, true) => Color::LightMagenta,
        (36, true) => Color::LightCyan,
        (37, true) => Color::Gray,
        _ => Color::Reset,
    }
}

pub fn parse_ls_colors(s: &str) -> HashMap<String, Style> {
    let mut map = HashMap::new();
    for part in s.split(':') {
        if let Some((key, val)) = part.split_once('=') {
            let style = parse_ansi(val);
            if let Some(ext) = key.strip_prefix("*.") {
                map.insert(format!("ext:{}", ext.to_lowercase()), style);
            } else {
                map.insert(key.to_string(), style);
            }
        }
    }
    map
}

fn parse_lscolors_color(ch: char) -> (Option<Color>, bool) {
    match ch {
        'a' => (Some(Color::Black), false),
        'b' => (Some(Color::Red), false),
        'c' => (Some(Color::Green), false),
        'd' => (Some(Color::Yellow), false),
        'e' => (Some(Color::Blue), false),
        'f' => (Some(Color::Magenta), false),
        'g' => (Some(Color::Cyan), false),
        'h' => (Some(Color::Gray), false),
        'A' => (Some(Color::DarkGray), true),
        'B' => (Some(Color::LightRed), true),
        'C' => (Some(Color::LightGreen), true),
        'D' => (Some(Color::LightYellow), true),
        'E' => (Some(Color::LightBlue), true),
        'F' => (Some(Color::LightMagenta), true),
        'G' => (Some(Color::LightCyan), true),
        'H' => (Some(Color::White), true),
        'x' => (None, false),
        _ => (None, false),
    }
}

fn parse_lscolors_pair(fg: char, bg: char) -> Style {
    let (fg, fg_bold) = parse_lscolors_color(fg);
    let (bg, bg_bold) = parse_lscolors_color(bg);
    let mut style = Style::default();
    if let Some(fg) = fg {
        style = style.fg(fg);
    }
    if let Some(bg) = bg {
        style = style.bg(bg);
    }
    if fg_bold || bg_bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

pub fn parse_lsc_colors(s: &str) -> HashMap<String, Style> {
    let mut map = HashMap::new();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < 22 {
        return map;
    }
    let keys = [
        "di", "ln", "so", "pi", "ex", "bd", "cd", "su", "sg", "tw", "ow",
    ];
    for (i, key) in keys.iter().enumerate() {
        let idx = i * 2;
        map.insert(
            (*key).to_string(),
            parse_lscolors_pair(chars[idx], chars[idx + 1]),
        );
    }
    map
}

pub fn parse_file_colors(s: &str) -> HashMap<String, Style> {
    if s.contains('=') {
        parse_ls_colors(s)
    } else {
        parse_lsc_colors(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ls_colors_extension_rule() {
        let colors = parse_ls_colors("di=01;34:*.rs=01;32");
        assert_eq!(
            colors.get("ext:rs").copied(),
            Some(
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD)
            )
        );
        assert_eq!(
            colors.get("di").copied(),
            Some(
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD)
            )
        );
    }

    #[test]
    fn test_parse_lsc_colors_directory_and_link() {
        let colors = parse_lsc_colors("ExfxcxdxCxegedabagacad");
        assert_eq!(
            colors.get("di").copied(),
            Some(
                Style::default()
                    .fg(Color::LightBlue)
                    .add_modifier(Modifier::BOLD)
            )
        );
        assert_eq!(
            colors.get("ln").copied(),
            Some(Style::default().fg(Color::Magenta))
        );
        assert_eq!(
            colors.get("ex").copied(),
            Some(
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD)
            )
        );
    }

    #[test]
    fn test_parse_file_colors_switches_by_format() {
        assert!(parse_file_colors("di=01;34").contains_key("di"));
        assert!(parse_file_colors("ExfxcxdxCxegedabagacad").contains_key("di"));
    }

    #[test]
    fn test_lookup_action_falls_back_from_shifted_uppercase() {
        let mut keymap = KeyMap::new();
        keymap.insert((KeyCode::Char('G'), KeyModifiers::NONE), Action::LastEntry);

        let action = lookup_action(&keymap, KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(action, Some(Action::LastEntry));
    }

    #[test]
    fn test_lookup_action_falls_back_from_shifted_symbol() {
        let mut keymap = KeyMap::new();
        keymap.insert((KeyCode::Char('!'), KeyModifiers::NONE), Action::ColMode1);

        let action = lookup_action(&keymap, KeyCode::Char('!'), KeyModifiers::SHIFT);
        assert_eq!(action, Some(Action::ColMode1));
    }
}
