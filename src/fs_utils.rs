use anyhow::Result;
use chrono::{Local, TimeZone};
use crossterm::{
    ExecutableCommand,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
#[cfg(target_os = "macos")]
use std::ffi::CStr;
use std::{
    collections::HashMap,
    fs,
    io::stdout,
    path::{Path, PathBuf},
};
#[cfg(unix)]
use std::{ffi::CString, os::unix::ffi::OsStrExt};

// ── Date / time ────────────────────────────────────────────────────────

/// Returns (YYYY-MM-DD, HH:MM, HH:MM:SS)
pub fn fmt_datetime(secs: u64) -> (String, String, String) {
    let dt = Local
        .timestamp_opt(secs as i64, 0)
        .single()
        .or_else(|| Local.timestamp_opt(0, 0).single())
        .expect("local timestamp for epoch should exist");
    (
        dt.format("%Y-%m-%d").to_string(),
        dt.format("%H:%M").to_string(),
        dt.format("%H:%M:%S").to_string(),
    )
}


pub fn now_str() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

// ── Disk / volume ──────────────────────────────────────────────────────

/// Returns (total, used, avail) in bytes.
pub fn disk_stats(path: &Path) -> (u64, u64, u64) {
    #[cfg(unix)]
    {
        let path_c = match CString::new(path.as_os_str().as_bytes()) {
            Ok(s) => s,
            Err(_) => return (0, 0, 0),
        };
        let mut st = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        let rc = unsafe { libc::statvfs(path_c.as_ptr(), st.as_mut_ptr()) };
        if rc != 0 {
            return (0, 0, 0);
        }
        let st = unsafe { st.assume_init() };
        // All statvfs fields are u32 on macOS and u64 on Linux.
        // Cast everything to u64 explicitly; the cast is a no-op on Linux.
        #[allow(clippy::unnecessary_cast)]
        let block_size = if st.f_frsize > 0 {
            st.f_frsize as u64
        } else {
            st.f_bsize as u64
        };
        #[allow(clippy::unnecessary_cast)]
        let total = (st.f_blocks as u64).saturating_mul(block_size);
        #[allow(clippy::unnecessary_cast)]
        let used = (st.f_blocks as u64)
            .saturating_sub(st.f_bfree as u64)
            .saturating_mul(block_size);
        #[allow(clippy::unnecessary_cast)]
        let avail = (st.f_bavail as u64).saturating_mul(block_size);
        (total, used, avail)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        (0, 0, 0)
    }
}

pub fn get_volume_info(path: &Path) -> String {
    #[cfg(target_os = "macos")]
    {
        let path_c = match CString::new(path.as_os_str().as_bytes()) {
            Ok(s) => s,
            Err(_) => return String::new(),
        };
        let mut st = std::mem::MaybeUninit::<libc::statfs>::uninit();
        let rc = unsafe { libc::statfs(path_c.as_ptr(), st.as_mut_ptr()) };
        if rc != 0 {
            return String::new();
        }
        let st = unsafe { st.assume_init() };
        let source = unsafe { CStr::from_ptr(st.f_mntfromname.as_ptr()) }
            .to_string_lossy()
            .trim()
            .to_string();
        let fstype = unsafe { CStr::from_ptr(st.f_fstypename.as_ptr()) }
            .to_string_lossy()
            .trim()
            .to_string();
        if source.is_empty() {
            String::new()
        } else if fstype.is_empty() {
            source
        } else {
            format!("{} ({})", source, fstype)
        }
    }
    #[cfg(target_os = "linux")]
    {
        get_volume_info_linux(path)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        String::new()
    }
}

#[cfg(target_os = "linux")]
fn decode_mount_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            let oct = &s[i + 1..i + 4];
            if let Ok(v) = u8::from_str_radix(oct, 8) {
                out.push(v as char);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(target_os = "linux")]
fn parse_mountinfo_line(line: &str) -> Option<(PathBuf, String, String)> {
    let (left, right) = line.split_once(" - ")?;
    let left_fields: Vec<&str> = left.split_whitespace().collect();
    let right_fields: Vec<&str> = right.split_whitespace().collect();
    if left_fields.len() < 5 || right_fields.len() < 2 {
        return None;
    }
    let mount_point = PathBuf::from(decode_mount_field(left_fields[4]));
    let fstype = right_fields[0].to_string();
    let source = decode_mount_field(right_fields[1]);
    Some((mount_point, source, fstype))
}

#[cfg(target_os = "linux")]
fn get_volume_info_linux(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let Ok(mountinfo) = fs::read_to_string("/proc/self/mountinfo") else {
        return String::new();
    };

    let best = mountinfo
        .lines()
        .filter_map(parse_mountinfo_line)
        .filter(|(mount_point, _, _)| canonical.starts_with(mount_point))
        .max_by_key(|(mount_point, _, _)| mount_point.components().count());

    match best {
        Some((_, source, fstype)) if !source.is_empty() && !fstype.is_empty() => {
            format!("{} ({})", source, fstype)
        }
        Some((_, source, _)) => source,
        None => String::new(),
    }
}

pub fn get_file_info(path: &std::path::Path) -> String {
    if fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return fs::read_link(path)
            .map(|target| format!("symbolic link to {}", target.display()))
            .unwrap_or_else(|_| "symbolic link".to_string());
    }
    let o = std::process::Command::new("file")
        .arg("-b")
        .arg(path)
        .output();
    if let Ok(o) = o {
        return String::from_utf8_lossy(&o.stdout).trim().to_string();
    }
    String::new()
}

/// Look up a numeric id in `/etc/passwd` or `/etc/group`.
pub fn resolve_name(file: &str, id: u32) -> String {
    fs::read_to_string(file)
        .ok()
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
        .args([
            "-C",
            &path.to_string_lossy(),
            "rev-parse",
            "--abbrev-ref",
            "HEAD",
        ])
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// トラッキングブランチとの ahead / behind コミット数を返す。
/// upstream が未設定の場合は (0, 0)。
pub fn get_git_ahead_behind(path: &Path) -> (u32, u32) {
    let path_s = path.to_string_lossy().into_owned();
    let run = |extra_args: &[&str]| -> u32 {
        let mut args = vec!["-C", &*path_s];
        args.extend_from_slice(extra_args);
        let Ok(o) = std::process::Command::new("git").args(&args).output() else {
            return 0;
        };
        if !o.status.success() { return 0; }
        String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
    };
    let ahead  = run(&["rev-list", "--count", "@{u}..HEAD"]);
    let behind = run(&["rev-list", "--count", "HEAD..@{u}"]);
    (ahead, behind)
}

pub fn get_git_status(path: &Path) -> HashMap<String, char> {
    let mut map = HashMap::new();

    // Resolve git root so paths in --porcelain output are unambiguous
    let Ok(root_out) = std::process::Command::new("git")
        .args([
            "-C",
            &path.to_string_lossy(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()
    else {
        return map;
    };
    if !root_out.status.success() {
        return map;
    }
    let root = PathBuf::from(String::from_utf8_lossy(&root_out.stdout).trim());

    // Prefix to strip: relative path from root to path, e.g. "src/"
    let prefix = path
        .strip_prefix(&root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| format!("{}/", p.display()));

    let Ok(o) = std::process::Command::new("git")
        .args(["-C", &root.to_string_lossy(), "status", "--porcelain"])
        .output()
    else {
        return map;
    };
    if !o.status.success() {
        return map;
    }
    for line in String::from_utf8_lossy(&o.stdout).lines() {
        if line.len() < 3 {
            continue;
        }
        let mut chars = line.chars();
        let x = chars.next().unwrap_or(' ');
        let y = chars.next().unwrap_or(' ');
        let ch = if x == '?' {
            '?'
        } else if x != ' ' {
            x
        } else {
            y
        };
        if ch == ' ' {
            continue;
        }
        let name = line[3..].trim_matches('"');
        let name = if let Some(i) = name.find(" -> ") {
            &name[i + 4..]
        } else {
            name
        };

        // Strip directory prefix; skip entries outside current directory
        let name = match &prefix {
            Some(pfx) => match name.strip_prefix(pfx.as_str()) {
                Some(s) => s,
                None => continue,
            },
            None => name,
        };

        if let Some(first) = name.split('/').next() {
            map.insert(first.to_string(), ch);
        }
    }
    map
}

// ── Tree helpers (pure FS, no TreeNode) ───────────────────────────────

pub fn tree_has_children(path: &PathBuf, show_hidden: bool) -> bool {
    fs::read_dir(path)
        .ok()
        .map(|d| {
            d.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                (show_hidden || !name.starts_with('.'))
                    && e.file_type().map(|t| t.is_dir()).unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

pub fn tree_list_subdirs(path: &PathBuf, show_hidden: bool) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(rd) = fs::read_dir(path) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                dirs.push(e.path());
            }
        }
    }
    dirs.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
            .cmp(
                &b.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase(),
            )
    });
    dirs
}

// ── Shell / process ────────────────────────────────────────────────────

pub fn shell_quote(s: &str) -> String {
    if s.chars()
        .any(|c| " \t\n'\"\\$`!&;|<>(){}[]#~?*".contains(c))
    {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

fn split_command_args(s: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(ch),
        }
    }

    if in_single || in_double {
        anyhow::bail!("引用符が閉じられていません");
    }
    if !cur.is_empty() {
        args.push(cur);
    }
    Ok(args)
}

/// git コマンドをサイレント実行し、stderr をエラーとして返す
pub fn git_command_silent(args: &[&str], dir: &Path) -> Result<()> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(anyhow::anyhow!("{}", if msg.is_empty() { "git error".to_string() } else { msg }))
    }
}

// ── git remote operations (subprocess) ────────────────────────────────

/// SSH パスフレーズを返す一時 askpass スクリプトを作成する。
/// 呼び出し元は使用後に削除する責任を持つ。
#[cfg(unix)]
fn create_askpass_script(passphrase: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!(".f5h_ap_{}.sh", std::process::id()));
    let content = format!("#!/bin/sh\nprintf '%s\\n' {}\n", shell_quote(passphrase));
    fs::write(&path, &content)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

/// git リモートコマンドをサイレント実行する。
/// passphrase が非空なら SSH_ASKPASS 経由でパスフレーズを渡す。
/// 空なら SSH エージェントを使い、エージェントがない場合は即エラー返却する。
fn git_remote_cmd(args: &[&str], dir: &Path, passphrase: &str) -> Result<()> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0");

    #[cfg(unix)]
    let askpass_path: Option<PathBuf> = if !passphrase.is_empty() {
        let p = create_askpass_script(passphrase)?;
        cmd.env("SSH_ASKPASS", &p).env("SSH_ASKPASS_REQUIRE", "force");
        Some(p)
    } else {
        // SSH エージェントを使う。エージェントがなければ即失敗させる
        cmd.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
        None
    };

    let out = cmd.output();

    #[cfg(unix)]
    if let Some(p) = askpass_path {
        let _ = fs::remove_file(&p);
    }

    let out = out?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(anyhow::anyhow!("{}", if msg.is_empty() { "git error".to_string() } else { msg }))
    }
}

/// origin から fetch する
pub fn git_fetch(dir: &Path, passphrase: &str) -> Result<()> {
    git_remote_cmd(&["fetch", "origin"], dir, passphrase)
}

/// 現在のブランチを origin へ push する
pub fn git_push(dir: &Path, passphrase: &str) -> Result<()> {
    let out = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(dir)
        .output()?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(anyhow::anyhow!("{}", if msg.is_empty() { "git error".to_string() } else { msg }));
    }
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if branch.is_empty() {
        return Err(anyhow::anyhow!("HEAD がブランチを指していません (detached HEAD)"));
    }
    git_remote_cmd(&["push", "origin", &branch], dir, passphrase)
}

/// fetch してから fast-forward マージを試みる（non-ff の場合はエラー）
pub fn git_pull(dir: &Path, passphrase: &str) -> Result<()> {
    git_remote_cmd(&["pull", "--ff-only"], dir, passphrase)
}

/// 作業ツリーを stash に退避する（msg 空なら -m なし）
pub fn git_stash_push(msg: &str, dir: &Path) -> Result<()> {
    if msg.trim().is_empty() {
        git_command_silent(&["stash", "push"], dir)
    } else {
        git_command_silent(&["stash", "push", "-m", msg], dir)
    }
}

/// 最新の stash を復元する
pub fn git_stash_pop(dir: &Path) -> Result<()> {
    git_command_silent(&["stash", "pop"], dir)
}

pub fn run_command(cmd: &str, dir: &Path) -> Result<()> {
    use crossterm::event::{self, Event};
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    std::process::Command::new("sh")
        .args(["-c", cmd])
        .current_dir(dir)
        .status()?;
    print!("\n--- Press any key to continue ---");
    let _ = std::io::Write::flush(&mut stdout());
    enable_raw_mode()?;
    // drain buffered input left from the command or previous interaction
    while event::poll(std::time::Duration::ZERO)? {
        let _ = event::read();
    }
    // wait for one keypress
    loop {
        if matches!(event::read()?, Event::Key(_)) {
            break;
        }
    }
    stdout().execute(EnterAlternateScreen)?;
    Ok(())
}

pub fn open_in_program(program: &str, path: &std::path::Path) -> Result<()> {
    let parts = split_command_args(program)?;
    let (cmd, args) = parts
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("プログラム名が空です"))?;
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    std::process::Command::new(cmd)
        .args(args)
        .arg(path)
        .status()?;
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
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
    }
    Ok(())
}

pub fn move_path(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_path(src, dst)?;
    if src.is_dir() {
        fs::remove_dir_all(src)?;
    } else {
        fs::remove_file(src)?;
    }
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── fmt_datetime ───────────────────────────────────────────────────

    #[test]
    fn test_fmt_datetime_epoch() {
        let dt = Local.timestamp_opt(0, 0).single().unwrap();
        let (date, time, full) = fmt_datetime(0);
        assert_eq!(date, dt.format("%Y-%m-%d").to_string());
        assert_eq!(time, dt.format("%H:%M").to_string());
        assert_eq!(full, dt.format("%H:%M:%S").to_string());
    }

    #[test]
    fn test_fmt_datetime_one_day() {
        let dt = Local.timestamp_opt(86400, 0).single().unwrap();
        let (date, _, _) = fmt_datetime(86400);
        assert_eq!(date, dt.format("%Y-%m-%d").to_string());
    }

    #[test]
    fn test_fmt_datetime_year_end() {
        let ts = 364 * 86400;
        let dt = Local.timestamp_opt(ts, 0).single().unwrap();
        let (date, _, _) = fmt_datetime(364 * 86400);
        assert_eq!(date, dt.format("%Y-%m-%d").to_string());
    }

    #[test]
    fn test_fmt_datetime_leap_day() {
        let ts = 789_u64 * 86400;
        let dt = Local.timestamp_opt(ts as i64, 0).single().unwrap();
        let (date, _, _) = fmt_datetime(ts);
        assert_eq!(date, dt.format("%Y-%m-%d").to_string());
    }

    #[test]
    fn test_fmt_datetime_time_components() {
        let ts = 45296;
        let dt = Local.timestamp_opt(ts, 0).single().unwrap();
        let (_, time, full) = fmt_datetime(45296);
        assert_eq!(time, dt.format("%H:%M").to_string());
        assert_eq!(full, dt.format("%H:%M:%S").to_string());
    }

    #[test]
    fn test_disk_stats_returns_consistent_values() {
        let dir = tempdir().unwrap();
        let (total, used, avail) = disk_stats(&dir.path().to_path_buf());
        assert!(total > 0);
        assert!(total >= used);
        assert!(total >= avail);
    }

    #[test]
    fn test_get_file_info_symlink_to_dir() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("link");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let info = get_file_info(&link);
        assert_eq!(info, format!("symbolic link to {}", target.display()));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_mountinfo_line() {
        let line = "36 25 0:32 / / rw,relatime - ext4 /dev/sda1 rw";
        let (mount_point, source, fstype) = parse_mountinfo_line(line).unwrap();
        assert_eq!(mount_point, PathBuf::from("/"));
        assert_eq!(source, "/dev/sda1");
        assert_eq!(fstype, "ext4");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_mountinfo_line_decodes_escapes() {
        let line = "84 36 0:45 / /tmp/my\\040mount rw,relatime - fuse.portal portal rw";
        let (mount_point, source, fstype) = parse_mountinfo_line(line).unwrap();
        assert_eq!(mount_point, PathBuf::from("/tmp/my mount"));
        assert_eq!(source, "portal");
        assert_eq!(fstype, "fuse.portal");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_get_volume_info_not_empty_for_current_dir() {
        let cwd = std::env::current_dir().unwrap();
        assert!(!get_volume_info(&cwd).is_empty());
    }

    // ── shell_quote ────────────────────────────────────────────────────

    #[test]
    fn test_shell_quote_clean_filename() {
        assert_eq!(shell_quote("foo.txt"), "foo.txt");
        assert_eq!(shell_quote("foo-bar.rs"), "foo-bar.rs");
        assert_eq!(shell_quote("foo_bar"), "foo_bar");
    }

    #[test]
    fn test_shell_quote_space() {
        assert_eq!(shell_quote("foo bar.txt"), "'foo bar.txt'");
        assert_eq!(shell_quote("my file"), "'my file'");
    }

    #[test]
    fn test_shell_quote_special_chars() {
        assert_eq!(shell_quote("foo$bar"), "'foo$bar'");
        assert_eq!(shell_quote("a&b"), "'a&b'");
        assert_eq!(shell_quote("a|b"), "'a|b'");
        assert_eq!(shell_quote("a;b"), "'a;b'");
        assert_eq!(shell_quote("a>b"), "'a>b'");
        assert_eq!(shell_quote("a<b"), "'a<b'");
        assert_eq!(shell_quote("a*b"), "'a*b'");
        assert_eq!(shell_quote("a?b"), "'a?b'");
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

    #[test]
    fn test_split_command_args_plain() {
        let parts = split_command_args("vim -p").unwrap();
        assert_eq!(parts, vec!["vim", "-p"]);
    }

    #[test]
    fn test_split_command_args_double_quotes() {
        let parts = split_command_args("open -a \"Visual Studio Code\"").unwrap();
        assert_eq!(parts, vec!["open", "-a", "Visual Studio Code"]);
    }

    #[test]
    fn test_split_command_args_single_quotes() {
        let parts = split_command_args("open -a 'Visual Studio Code'").unwrap();
        assert_eq!(parts, vec!["open", "-a", "Visual Studio Code"]);
    }

    #[test]
    fn test_split_command_args_backslash_escape() {
        let parts = split_command_args("open -a Visual\\ Studio\\ Code").unwrap();
        assert_eq!(parts, vec!["open", "-a", "Visual Studio Code"]);
    }

    #[test]
    fn test_split_command_args_unclosed_quote_errors() {
        assert!(split_command_args("open -a \"Visual Studio Code").is_err());
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
        let names: Vec<String> = result
            .iter()
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

    // ── git remote operations ──────────────────────────────────────────

    /// テスト用に git リポジトリを初期化してコミットを作成する
    fn init_git_repo_for_test(dir: &std::path::Path) {
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .output()
                .unwrap()
        };
        git(&["init"]);
        git(&["commit", "--allow-empty", "-m", "init"]);
    }

    /// トラッキング済みファイルを変更した dirty な git リポジトリを作成する。
    fn make_dirty_repo(dir: &std::path::Path) {
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .output()
                .unwrap()
        };
        init_git_repo_for_test(dir);
        std::fs::write(dir.join("file.txt"), "initial").unwrap();
        git(&["add", "file.txt"]);
        git(&["commit", "-m", "add file"]);
        // トラッキング済みファイルを変更して dirty 状態にする
        std::fs::write(dir.join("file.txt"), "modified").unwrap();
    }

    #[test]
    fn test_git_fetch_errors_on_non_repo() {
        let dir = tempdir().unwrap();
        let err = git_fetch(dir.path(), "").unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_git_push_errors_on_non_repo() {
        let dir = tempdir().unwrap();
        let err = git_push(dir.path(), "").unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_git_pull_errors_on_non_repo() {
        let dir = tempdir().unwrap();
        let err = git_pull(dir.path(), "").unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_git_fetch_errors_without_origin() {
        let dir = tempdir().unwrap();
        init_git_repo_for_test(dir.path());
        let err = git_fetch(dir.path(), "").unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_git_push_errors_without_origin() {
        let dir = tempdir().unwrap();
        init_git_repo_for_test(dir.path());
        let err = git_push(dir.path(), "").unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_git_pull_errors_without_origin() {
        let dir = tempdir().unwrap();
        init_git_repo_for_test(dir.path());
        let err = git_pull(dir.path(), "").unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    // ── git_stash_push / git_stash_pop ────────────────────────────────

    #[test]
    fn test_git_stash_push_no_message_reverts_changes() {
        let dir = tempdir().unwrap();
        make_dirty_repo(dir.path());
        // stash push（メッセージなし）が成功する
        git_stash_push("", dir.path()).unwrap();
        // ファイルが stash 前の状態（"initial"）に戻っている
        let content = std::fs::read_to_string(dir.path().join("file.txt")).unwrap();
        assert_eq!(content, "initial");
    }

    #[test]
    fn test_git_stash_push_with_message_reverts_changes() {
        let dir = tempdir().unwrap();
        make_dirty_repo(dir.path());
        git_stash_push("WIP: my work", dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join("file.txt")).unwrap();
        assert_eq!(content, "initial");
    }

    #[test]
    fn test_git_stash_pop_restores_changes() {
        let dir = tempdir().unwrap();
        make_dirty_repo(dir.path());
        git_stash_push("", dir.path()).unwrap();
        // pop して変更が復元されることを確認
        git_stash_pop(dir.path()).unwrap();
        let content = std::fs::read_to_string(dir.path().join("file.txt")).unwrap();
        assert_eq!(content, "modified");
    }

    #[test]
    fn test_git_stash_push_errors_on_non_repo() {
        let dir = tempdir().unwrap();
        let err = git_stash_push("", dir.path()).unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_git_stash_pop_errors_on_empty_stash() {
        let dir = tempdir().unwrap();
        init_git_repo_for_test(dir.path());
        // stash が空の場合はエラーになる
        let err = git_stash_pop(dir.path()).unwrap_err().to_string();
        assert!(!err.is_empty());
    }

    #[test]
    fn test_git_push_errors_on_detached_head() {
        let dir = tempdir().unwrap();
        init_git_repo_for_test(dir.path());
        // HEAD をデタッチする
        std::process::Command::new("git")
            .args(["checkout", "--detach", "HEAD"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let err = git_push(dir.path(), "").unwrap_err().to_string();
        assert!(!err.is_empty());
    }
}
