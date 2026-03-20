use anyhow::Result;
use chrono::Local;
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use std::{collections::HashMap, fs, io::stdout, path::{Path, PathBuf}};

// ── Date / time ────────────────────────────────────────────────────────

/// Returns (YYYY-MM-DD, HH:MM, HH:MM:SS)
pub fn fmt_datetime(secs: u64) -> (String, String, String) {
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600)  / 60;
    let s =  secs % 60;
    let mut days = (secs / 86400) as i64;
    let mut yr   = 1970i64;
    loop {
        let dy = if is_leap(yr) { 366 } else { 365 };
        if days < dy { break; }
        days -= dy; yr += 1;
    }
    let md = [31, if is_leap(yr) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1usize;
    for (i, &d) in md.iter().enumerate() { if days < d { mo = i + 1; break; } days -= d; }
    (
        format!("{:04}-{:02}-{:02}", yr, mo, days + 1),
        format!("{:02}:{:02}", h, m),
        format!("{:02}:{:02}:{:02}", h, m, s),
    )
}

pub fn is_leap(y: i64) -> bool { (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 }

pub fn now_str() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ── Disk / volume ──────────────────────────────────────────────────────

/// Returns (total, used, avail) in bytes.
pub fn disk_stats(path: &PathBuf) -> (u64, u64, u64) {
    let o = std::process::Command::new("df")
        .args(["--output=size,used,avail", "-B1"]).arg(path).output();
    if let Ok(o) = o {
        let s = String::from_utf8_lossy(&o.stdout);
        if let Some(l) = s.lines().nth(1) {
            let v: Vec<u64> = l.split_whitespace()
                .filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 3 { return (v[0], v[1], v[2]); }
        }
    }
    (0, 0, 0)
}

pub fn get_volume_info(path: &PathBuf) -> String {
    let o = std::process::Command::new("df")
        .args(["--output=source,fstype"]).arg(path).output();
    if let Ok(o) = o {
        let s = String::from_utf8_lossy(&o.stdout);
        if let Some(l) = s.lines().nth(1) {
            let parts: Vec<&str> = l.split_whitespace().collect();
            if parts.len() >= 2 { return format!("{} ({})", parts[0], parts[1]); }
        }
    }
    String::new()
}

pub fn get_file_info(path: &std::path::Path) -> String {
    let o = std::process::Command::new("file").arg("-b").arg(path).output();
    if let Ok(o) = o { return String::from_utf8_lossy(&o.stdout).trim().to_string(); }
    String::new()
}

/// Look up a numeric id in `/etc/passwd` or `/etc/group`.
pub fn resolve_name(file: &str, id: u32) -> String {
    fs::read_to_string(file).ok()
        .and_then(|s| {
            s.lines()
             .find(|l| l.split(':').nth(2).and_then(|f| f.parse::<u32>().ok()) == Some(id))
             .and_then(|l| l.split(':').next().map(|n| n.to_string()))
        })
        .unwrap_or_else(|| id.to_string())
}

// ── Git ────────────────────────────────────────────────────────────────

pub fn get_git_branch(path: &Path) -> Option<String> {
    let o = std::process::Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output().ok()?;
    if !o.status.success() { return None; }
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

pub fn get_git_status(path: &Path) -> HashMap<String, char> {
    let mut map = HashMap::new();
    let Ok(o) = std::process::Command::new("git")
        .args(["-C", &path.to_string_lossy(), "status", "--porcelain"])
        .output() else { return map; };
    if !o.status.success() { return map; }
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        if line.len() < 3 { continue; }
        let mut chars = line.chars();
        let x = chars.next().unwrap_or(' ');
        let y = chars.next().unwrap_or(' ');
        let ch = if x == '?' { '?' } else if x != ' ' { x } else { y };
        if ch == ' ' { continue; }
        let name = line[3..].trim_matches('"');
        let name = if let Some(i) = name.find(" -> ") { &name[i + 4..] } else { name };
        if let Some(first) = name.split('/').next() {
            map.insert(first.to_string(), ch);
        }
    }
    map
}

// ── Tree helpers (pure FS, no TreeNode) ───────────────────────────────

pub fn tree_has_children(path: &PathBuf, show_hidden: bool) -> bool {
    fs::read_dir(path).ok()
        .map(|d| d.flatten().any(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            (show_hidden || !name.starts_with('.'))
                && e.file_type().map(|t| t.is_dir()).unwrap_or(false)
        }))
        .unwrap_or(false)
}

pub fn tree_list_subdirs(path: &PathBuf, show_hidden: bool) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(rd) = fs::read_dir(path) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') { continue; }
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                dirs.push(e.path());
            }
        }
    }
    dirs.sort_by(|a, b| {
        a.file_name().unwrap_or_default().to_string_lossy().to_lowercase()
            .cmp(&b.file_name().unwrap_or_default().to_string_lossy().to_lowercase())
    });
    dirs
}

// ── Shell / process ────────────────────────────────────────────────────

pub fn shell_quote(s: &str) -> String {
    if s.chars().any(|c| " \t\n'\"\\$`!&;|<>(){}[]#~?*".contains(c)) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

pub fn run_command(cmd: &str) -> Result<()> {
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    std::process::Command::new("sh").args(["-c", cmd]).status()?;
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    Ok(())
}

pub fn open_in_program(program: &str, path: &std::path::Path) -> Result<()> {
    let mut parts = program.split_whitespace();
    let cmd = parts.next().ok_or_else(|| anyhow::anyhow!("プログラム名が空です"))?;
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    std::process::Command::new(cmd).args(parts).arg(path).status()?;
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    Ok(())
}

// ── File operations ─────────────────────────────────────────────────────

pub fn copy_path(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_path(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = dst.parent() { fs::create_dir_all(parent)?; }
        fs::copy(src, dst)?;
    }
    Ok(())
}

pub fn move_path(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    if fs::rename(src, dst).is_ok() { return Ok(()); }
    copy_path(src, dst)?;
    if src.is_dir() { fs::remove_dir_all(src)?; } else { fs::remove_file(src)?; }
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── is_leap ────────────────────────────────────────────────────────

    #[test]
    fn test_is_leap_divisible_by_400() { assert!( is_leap(2000)); }
    #[test]
    fn test_is_leap_divisible_by_100_not_400() { assert!(!is_leap(1900)); }
    #[test]
    fn test_is_leap_divisible_by_4_not_100()  { assert!( is_leap(1996)); }
    #[test]
    fn test_is_leap_not_divisible_by_4()      { assert!(!is_leap(1997)); }

    // ── fmt_datetime ───────────────────────────────────────────────────

    #[test]
    fn test_fmt_datetime_epoch() {
        let (date, time, full) = fmt_datetime(0);
        assert_eq!(date, "1970-01-01");
        assert_eq!(time, "00:00");
        assert_eq!(full, "00:00:00");
    }

    #[test]
    fn test_fmt_datetime_one_day() {
        let (date, _, _) = fmt_datetime(86400);
        assert_eq!(date, "1970-01-02");
    }

    #[test]
    fn test_fmt_datetime_year_end() {
        // 1970-12-31: day 364 (0-indexed) of 1970
        let (date, _, _) = fmt_datetime(364 * 86400);
        assert_eq!(date, "1970-12-31");
    }

    #[test]
    fn test_fmt_datetime_leap_day() {
        // 1972 is a leap year: 1972-02-29 exists
        // Days from epoch to 1972-02-29:
        //   1970: 365 + 1971: 365 + Jan: 31 + Feb1-28: 28 = 365+365+31+28 = 789
        let (date, _, _) = fmt_datetime(789 * 86400);
        assert_eq!(date, "1972-02-29");
    }

    #[test]
    fn test_fmt_datetime_time_components() {
        // 1970-01-01 12:34:56 = 12*3600 + 34*60 + 56 = 45296 secs
        let (_, time, full) = fmt_datetime(45296);
        assert_eq!(time, "12:34");
        assert_eq!(full, "12:34:56");
    }

    // ── shell_quote ────────────────────────────────────────────────────

    #[test]
    fn test_shell_quote_clean_filename() {
        assert_eq!(shell_quote("foo.txt"),    "foo.txt");
        assert_eq!(shell_quote("foo-bar.rs"), "foo-bar.rs");
        assert_eq!(shell_quote("foo_bar"),    "foo_bar");
    }

    #[test]
    fn test_shell_quote_space() {
        assert_eq!(shell_quote("foo bar.txt"), "'foo bar.txt'");
        assert_eq!(shell_quote("my file"),     "'my file'");
    }

    #[test]
    fn test_shell_quote_special_chars() {
        assert_eq!(shell_quote("foo$bar"),  "'foo$bar'");
        assert_eq!(shell_quote("a&b"),      "'a&b'");
        assert_eq!(shell_quote("a|b"),      "'a|b'");
        assert_eq!(shell_quote("a;b"),      "'a;b'");
        assert_eq!(shell_quote("a>b"),      "'a>b'");
        assert_eq!(shell_quote("a<b"),      "'a<b'");
        assert_eq!(shell_quote("a*b"),      "'a*b'");
        assert_eq!(shell_quote("a?b"),      "'a?b'");
    }

    #[test]
    fn test_shell_quote_single_quote() {
        // "it's" → 'it'\''s'
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_quote_tab_and_newline() {
        assert_eq!(shell_quote("a\tb"), "'a\tb'");
        assert_eq!(shell_quote("a\nb"), "'a\nb'");
    }

    // ── tree_has_children ──────────────────────────────────────────────

    #[test]
    fn test_tree_has_children_empty_dir() {
        let dir = tempdir().unwrap();
        assert!(!tree_has_children(&dir.path().to_path_buf(), false));
    }

    #[test]
    fn test_tree_has_children_files_only() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("foo.txt"), "").unwrap();
        assert!(!tree_has_children(&dir.path().to_path_buf(), false));
    }

    #[test]
    fn test_tree_has_children_with_subdir() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(tree_has_children(&dir.path().to_path_buf(), false));
    }

    #[test]
    fn test_tree_has_children_hidden_subdir_hidden_off() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        // show_hidden=false: 非表示ディレクトリは無視
        assert!(!tree_has_children(&dir.path().to_path_buf(), false));
    }

    #[test]
    fn test_tree_has_children_hidden_subdir_hidden_on() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();
        // show_hidden=true: 非表示ディレクトリも対象
        assert!(tree_has_children(&dir.path().to_path_buf(), true));
    }

    // ── tree_list_subdirs ──────────────────────────────────────────────

    #[test]
    fn test_tree_list_subdirs_sorted() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("charlie")).unwrap();
        std::fs::create_dir(dir.path().join("alpha")).unwrap();
        std::fs::create_dir(dir.path().join("bravo")).unwrap();
        std::fs::write(dir.path().join("not_a_dir.txt"), "").unwrap();

        let result = tree_list_subdirs(&dir.path().to_path_buf(), false);
        let names: Vec<String> = result.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
    }

    #[test]
    fn test_tree_list_subdirs_excludes_files() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        std::fs::write(dir.path().join("file.txt"), "").unwrap();

        let result = tree_list_subdirs(&dir.path().to_path_buf(), false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_name().unwrap(), "subdir");
    }

    // ── copy_path ──────────────────────────────────────────────────────

    #[test]
    fn test_copy_path_copies_file() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        std::fs::write(&src, b"hello").unwrap();
        copy_path(&src, &dst).unwrap();
        assert!(dst.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello");
        assert!(src.exists()); // 元は残る
    }

    #[test]
    fn test_copy_path_copies_dir_recursively() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("a.txt"), b"aaa").unwrap();
        std::fs::create_dir(src.join("sub")).unwrap();
        std::fs::write(src.join("sub").join("b.txt"), b"bbb").unwrap();
        copy_path(&src, &dst).unwrap();
        assert!(dst.join("a.txt").exists());
        assert!(dst.join("sub").join("b.txt").exists());
    }

    // ── move_path ──────────────────────────────────────────────────────

    #[test]
    fn test_move_path_moves_file() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        std::fs::write(&src, b"hello").unwrap();
        move_path(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"hello");
    }

    #[test]
    fn test_move_path_moves_dir() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("f.txt"), b"x").unwrap();
        move_path(&src, &dst).unwrap();
        assert!(!src.exists());
        assert!(dst.join("f.txt").exists());
    }

    #[test]
    fn test_tree_list_subdirs_hidden_flag() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("visible")).unwrap();
        std::fs::create_dir(dir.path().join(".hidden")).unwrap();

        let no_hidden = tree_list_subdirs(&dir.path().to_path_buf(), false);
        assert_eq!(no_hidden.len(), 1);

        let with_hidden = tree_list_subdirs(&dir.path().to_path_buf(), true);
        assert_eq!(with_hidden.len(), 2);
    }
}
