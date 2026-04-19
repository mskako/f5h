use anyhow::Result;

// ── Data types ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ProcEntry {
    pub pid: u32,
    pub user: String,
    pub cpu: f32,
    pub mem: f32,
    pub vsz: u64, // KB
    pub rss: u64, // KB
    pub stat: String,
    pub command: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcSortMode {
    Cpu,
    Mem,
    Pid,
    User,
    Command,
}

impl ProcSortMode {
    /// デフォルトの昇順フラグ（false = 降順 = 大きい値が上）
    pub fn default_asc(self) -> bool {
        match self {
            ProcSortMode::Cpu | ProcSortMode::Mem => false,
            _ => true,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProcSortMode::Cpu => "CPU",
            ProcSortMode::Mem => "MEM",
            ProcSortMode::Pid => "PID",
            ProcSortMode::User => "USER",
            ProcSortMode::Command => "CMD",
        }
    }
}

/// システム全体の情報
pub struct SysInfo {
    pub load_1: f32,
    pub load_5: f32,
    pub load_15: f32,
    pub mem_used: u64,   // bytes
    pub mem_total: u64,  // bytes
    pub swap_used: u64,  // bytes
    pub swap_total: u64, // bytes
    pub cpu_online: u32,
    pub uptime_secs: u64,
}

/// 選択中プロセスの詳細情報
#[derive(Clone, Debug, Default)]
pub struct ProcDetail {
    pub pid: u32,
    pub ppid: u32,
    pub tty: String,
    pub user: String,
    pub stat: String,
    pub command: String,
    pub cpu: f32,
    pub mem: f32,
    pub vsz: u64, // KB
    pub rss: u64, // KB
    pub cmdline: String,
    pub cwd: String,
    pub started_str: String,
    pub elapsed_str: String,
    pub fd_count: usize,
    pub threads: u32,
}

pub struct ProcSignalMenu {
    pub cursor: usize,
    pub pid: u32,
    pub proc_name: String,
}

/// シグナル一覧: (signal番号, 直接入力キー, 名前表示, 説明)
pub const SIGNAL_ITEMS: &[(i32, char, &str, &str)] = &[
    (1,  'h', "SIGHUP  ( 1)", "reload / restart"),
    (2,  'i', "SIGINT  ( 2)", "interrupt (Ctrl+C)"),
    (9,  'k', "SIGKILL ( 9)", "force kill"),
    (15, 't', "SIGTERM (15)", "terminate"),
    (18, 'c', "SIGCONT (18)", "continue"),
    (20, 's', "SIGSTOP (20)", "stop"),
];

// ── FD 一覧 ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FdType {
    Stdin,
    Stdout,
    Stderr,
    File,
    Socket,
    Pipe,
    Anon,
    Other,
}

impl FdType {
    pub fn tag(&self) -> &'static str {
        match self {
            FdType::Stdin  => "stdin ",
            FdType::Stdout => "stdout",
            FdType::Stderr => "stderr",
            FdType::File   => "file  ",
            FdType::Socket => "socket",
            FdType::Pipe   => "pipe  ",
            FdType::Anon   => "anon  ",
            FdType::Other  => "other ",
        }
    }
}

#[derive(Clone, Debug)]
pub struct FdEntry {
    pub fd: u32,
    pub fd_type: FdType,
    pub target: String,
}

pub fn get_fd_list(pid: u32) -> Vec<FdEntry> {
    let fd_dir = format!("/proc/{}/fd", pid);
    let mut entries = Vec::new();
    let dir = match std::fs::read_dir(&fd_dir) {
        Ok(d) => d,
        Err(_) => return entries,
    };
    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let fd: u32 = match name.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let target = std::fs::read_link(entry.path())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "?".to_string());
        let fd_type = classify_fd(fd, &target);
        entries.push(FdEntry { fd, fd_type, target });
    }
    entries.sort_by_key(|e| e.fd);
    entries
}

fn classify_fd(fd: u32, target: &str) -> FdType {
    if target.starts_with("socket:") {
        FdType::Socket
    } else if target.starts_with("pipe:") {
        FdType::Pipe
    } else if target.starts_with("anon_inode:") {
        FdType::Anon
    } else if target.starts_with('/') {
        match fd {
            0 => FdType::Stdin,
            1 => FdType::Stdout,
            2 => FdType::Stderr,
            _ => FdType::File,
        }
    } else {
        FdType::Other
    }
}

// ── Process list ───────────────────────────────────────────────────────

pub fn get_proc_list() -> Vec<ProcEntry> {
    let mem_total_kb = read_mem_total_kb();
    let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    let clk_tck = if clk_tck <= 0.0 { 100.0 } else { clk_tck };
    let uptime: f64 = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse().ok()))
        .unwrap_or(1.0);

    let dir = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();

    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let pid: u32 = match name.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let stat_str = match std::fs::read_to_string(format!("/proc/{}/stat", pid)) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let (comm, state, utime, stime, starttime_ticks) = parse_stat_line(&stat_str);

        let (uid, vsz_kb, rss_kb) = read_proc_status_fields(pid);
        let user = uid_to_username(uid);

        let total_ticks = (utime + stime) as f64;
        let elapsed = (uptime - starttime_ticks as f64 / clk_tck).max(1.0);
        let cpu = (total_ticks / clk_tck / elapsed * 100.0) as f32;

        let mem = if mem_total_kb > 0 {
            (rss_kb as f64 / mem_total_kb as f64 * 100.0) as f32
        } else {
            0.0
        };

        entries.push(ProcEntry { pid, user, cpu, mem, vsz: vsz_kb, rss: rss_kb, stat: state, command: comm });
    }

    entries
}

fn parse_stat_line(s: &str) -> (String, String, u64, u64, u64) {
    let comm_start = s.find('(').unwrap_or(0);
    let comm_end = s.rfind(')').unwrap_or(0);
    let comm = if comm_end > comm_start {
        s[comm_start + 1..comm_end].to_string()
    } else {
        String::new()
    };
    let after = if comm_end + 1 < s.len() { &s[comm_end + 1..] } else { "" };
    let fields: Vec<&str> = after.split_whitespace().collect();
    // after ')': state(0) ppid(1) ... utime(11) stime(12) ... starttime(19)
    let state = fields.first().unwrap_or(&"?").to_string();
    let utime: u64 = fields.get(11).and_then(|s| s.parse().ok()).unwrap_or(0);
    let stime: u64 = fields.get(12).and_then(|s| s.parse().ok()).unwrap_or(0);
    let starttime: u64 = fields.get(19).and_then(|s| s.parse().ok()).unwrap_or(0);
    (comm, state, utime, stime, starttime)
}

fn read_proc_status_fields(pid: u32) -> (u32, u64, u64) {
    let s = match std::fs::read_to_string(format!("/proc/{}/status", pid)) {
        Ok(s) => s,
        Err(_) => return (0, 0, 0),
    };
    let mut uid: u32 = 0;
    let mut vsz: u64 = 0;
    let mut rss: u64 = 0;
    for line in s.lines() {
        if line.starts_with("Uid:") {
            uid = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if line.starts_with("VmSize:") {
            vsz = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        } else if line.starts_with("VmRSS:") {
            rss = line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        }
    }
    (uid, vsz, rss)
}

fn uid_to_username(uid: u32) -> String {
    if let Ok(s) = std::fs::read_to_string("/etc/passwd") {
        for line in s.lines() {
            let mut parts = line.splitn(4, ':');
            let name = parts.next().unwrap_or("");
            let _ = parts.next();
            if parts.next().and_then(|v| v.parse::<u32>().ok()) == Some(uid) {
                return name.to_string();
            }
        }
    }
    uid.to_string()
}

fn read_mem_total_kb() -> u64 {
    if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
        for line in s.lines() {
            if line.starts_with("MemTotal:") {
                return line.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }
    }
    0
}

pub fn sort_proc_entries(entries: &mut Vec<ProcEntry>, mode: ProcSortMode, asc: bool) {
    match mode {
        ProcSortMode::Cpu => {
            entries.sort_by(|a, b| {
                a.cpu.partial_cmp(&b.cpu).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        ProcSortMode::Mem => {
            entries.sort_by(|a, b| {
                a.mem.partial_cmp(&b.mem).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        ProcSortMode::Pid => {
            entries.sort_by_key(|e| e.pid);
        }
        ProcSortMode::User => {
            entries.sort_by(|a, b| a.user.cmp(&b.user));
        }
        ProcSortMode::Command => {
            entries.sort_by(|a, b| a.command.cmp(&b.command));
        }
    }
    if !asc {
        entries.reverse();
    }
}

// ── プロセス詳細 ────────────────────────────────────────────────────────

pub fn get_proc_detail(pid: u32, entries: &[ProcEntry]) -> ProcDetail {
    let entry = entries.iter().find(|e| e.pid == pid);

    let cmdline = read_proc_cmdline(pid);
    let cwd = read_proc_cwd(pid);
    let (started_str, elapsed_str) = read_proc_times(pid);
    let fd_count = count_proc_fds(pid);
    let threads = read_proc_threads(pid);
    let (ppid, tty) = read_proc_ppid_tty(pid);

    if let Some(e) = entry {
        ProcDetail {
            pid,
            ppid,
            tty,
            user: e.user.clone(),
            stat: e.stat.clone(),
            command: e.command.clone(),
            cpu: e.cpu,
            mem: e.mem,
            vsz: e.vsz,
            rss: e.rss,
            cmdline,
            cwd,
            started_str,
            elapsed_str,
            fd_count,
            threads,
        }
    } else {
        ProcDetail {
            pid,
            ppid,
            tty,
            cmdline,
            cwd,
            started_str,
            elapsed_str,
            fd_count,
            threads,
            ..Default::default()
        }
    }
}

fn read_proc_ppid_tty(pid: u32) -> (u32, String) {
    let stat = match std::fs::read_to_string(format!("/proc/{}/stat", pid)) {
        Ok(s) => s,
        Err(_) => return (0, "?".to_string()),
    };
    let after_comm = match stat.rfind(')') {
        Some(i) => &stat[i + 1..],
        None => return (0, "?".to_string()),
    };
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // after ')': state(0) ppid(1) pgrp(2) session(3) tty_nr(4) ...
    let ppid: u32 = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let tty_nr: i32 = fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    (ppid, decode_tty(tty_nr))
}

fn decode_tty(tty_nr: i32) -> String {
    if tty_nr == 0 {
        return "?".to_string();
    }
    let tty_nr = tty_nr as u32;
    let minor = (tty_nr & 0xff) | ((tty_nr >> 20) << 8);
    let major = (tty_nr >> 8) & 0xfff;
    match major {
        136 => format!("pts/{}", minor),
        4 if minor < 64 => format!("tty{}", minor),
        4 => format!("ttyS{}", minor - 64),
        _ => format!("{}:{}", major, minor),
    }
}

fn read_proc_cmdline(pid: u32) -> String {
    match std::fs::read(format!("/proc/{}/cmdline", pid)) {
        Ok(bytes) => {
            bytes
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).to_string())
                .collect::<Vec<_>>()
                .join(" ")
        }
        Err(_) => String::new(),
    }
}

fn read_proc_cwd(pid: u32) -> String {
    std::fs::read_link(format!("/proc/{}/cwd", pid))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "?".to_string())
}

fn count_proc_fds(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{}/fd", pid))
        .map(|d| d.count())
        .unwrap_or(0)
}

fn read_proc_threads(pid: u32) -> u32 {
    if let Ok(s) = std::fs::read_to_string(format!("/proc/{}/status", pid)) {
        for line in s.lines() {
            if line.starts_with("Threads:") {
                return line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
            }
        }
    }
    1
}

/// /proc/{pid}/stat からプロセス開始時刻・経過時間を計算する
fn read_proc_times(pid: u32) -> (String, String) {
    let stat = match std::fs::read_to_string(format!("/proc/{}/stat", pid)) {
        Ok(s) => s,
        Err(_) => return ("?".to_string(), "?".to_string()),
    };
    // comm は括弧で囲まれている。最後の ')' 以降がフィールド列
    let after_comm = match stat.rfind(')') {
        Some(i) => &stat[i + 1..],
        None => return ("?".to_string(), "?".to_string()),
    };
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // ')' の後: state(0) ppid(1) ... starttime(19)
    let starttime_ticks: u64 = fields
        .get(19)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let uptime_secs: f64 = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0);

    let clk_tck = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    if clk_tck <= 0.0 {
        return ("?".to_string(), "?".to_string());
    }

    let start_secs_since_boot = starttime_ticks as f64 / clk_tck;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let boot_time = now_secs - uptime_secs;
    let start_unix = boot_time + start_secs_since_boot;
    let elapsed = (now_secs - start_unix).max(0.0) as u64;

    let started_str = format_unix_timestamp(start_unix as i64);
    let elapsed_str = format_elapsed(elapsed);
    (started_str, elapsed_str)
}

pub fn format_elapsed(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{}d {:02}:{:02}:{:02}", d, h, m, s)
    } else {
        format!("{:02}:{:02}:{:02}", h, m, s)
    }
}

fn format_unix_timestamp(unix_secs: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(unix_secs, 0).single() {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => "?".to_string(),
    }
}

// ── System info ────────────────────────────────────────────────────────

pub fn get_sys_info() -> SysInfo {
    let (load_1, load_5, load_15) = read_loadavg();
    let (mem_used, mem_total, swap_used, swap_total) = read_meminfo();
    let cpu_online = read_cpu_count();
    let uptime_secs = read_uptime();
    SysInfo { load_1, load_5, load_15, mem_used, mem_total, swap_used, swap_total, cpu_online, uptime_secs }
}

fn read_loadavg() -> (f32, f32, f32) {
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/proc/loadavg") {
        let mut p = s.split_whitespace();
        let l1 = p.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let l5 = p.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let l15 = p.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
        return (l1, l5, l15);
    }
    (0.0, 0.0, 0.0)
}

fn read_meminfo() -> (u64, u64, u64, u64) {
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
        let mut total: u64 = 0;
        let mut available: u64 = 0;
        let mut swap_total: u64 = 0;
        let mut swap_free: u64 = 0;
        for line in s.lines() {
            let kib: u64 = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0)
                * 1024;
            if line.starts_with("MemTotal:") {
                total = kib;
            } else if line.starts_with("MemAvailable:") {
                available = kib;
            } else if line.starts_with("SwapTotal:") {
                swap_total = kib;
            } else if line.starts_with("SwapFree:") {
                swap_free = kib;
            }
        }
        if total > 0 {
            return (
                total.saturating_sub(available),
                total,
                swap_total.saturating_sub(swap_free),
                swap_total,
            );
        }
    }
    (0, 0, 0, 0)
}

fn read_cpu_count() -> u32 {
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/proc/cpuinfo") {
        let count = s.lines().filter(|l| l.starts_with("processor")).count() as u32;
        if count > 0 {
            return count;
        }
    }
    1
}

fn read_uptime() -> u64 {
    #[cfg(target_os = "linux")]
    if let Ok(s) = std::fs::read_to_string("/proc/uptime") {
        return s
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|f| f as u64)
            .unwrap_or(0);
    }
    0
}

// ── Kill ───────────────────────────────────────────────────────────────

pub fn kill_pid(pid: u32, signal: i32) -> Result<()> {
    let ret = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if ret == 0 {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "kill({}, {}) failed: {}",
            pid,
            signal,
            std::io::Error::last_os_error()
        ))
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entries(data: &[(u32, &str, f32, f32)]) -> Vec<ProcEntry> {
        data.iter()
            .map(|&(pid, user, cpu, mem)| ProcEntry {
                pid,
                user: user.to_string(),
                cpu,
                mem,
                vsz: 0,
                rss: 0,
                stat: "S".to_string(),
                command: user.to_string(),
            })
            .collect()
    }

    #[test]
    fn test_sort_by_cpu_descending() {
        let mut entries = make_entries(&[(1, "a", 1.0, 0.0), (2, "b", 5.0, 0.0), (3, "c", 3.0, 0.0)]);
        sort_proc_entries(&mut entries, ProcSortMode::Cpu, false);
        assert_eq!(entries[0].pid, 2); // highest cpu first
        assert_eq!(entries[1].pid, 3);
        assert_eq!(entries[2].pid, 1);
    }

    #[test]
    fn test_sort_by_cpu_ascending() {
        let mut entries = make_entries(&[(1, "a", 1.0, 0.0), (2, "b", 5.0, 0.0), (3, "c", 3.0, 0.0)]);
        sort_proc_entries(&mut entries, ProcSortMode::Cpu, true);
        assert_eq!(entries[0].pid, 1); // lowest cpu first
        assert_eq!(entries[1].pid, 3);
        assert_eq!(entries[2].pid, 2);
    }

    #[test]
    fn test_sort_by_pid_ascending() {
        let mut entries = make_entries(&[(30, "a", 0.0, 0.0), (10, "b", 0.0, 0.0), (20, "c", 0.0, 0.0)]);
        sort_proc_entries(&mut entries, ProcSortMode::Pid, true);
        assert_eq!(entries[0].pid, 10);
        assert_eq!(entries[1].pid, 20);
        assert_eq!(entries[2].pid, 30);
    }

    #[test]
    fn test_sort_by_pid_descending() {
        let mut entries = make_entries(&[(30, "a", 0.0, 0.0), (10, "b", 0.0, 0.0), (20, "c", 0.0, 0.0)]);
        sort_proc_entries(&mut entries, ProcSortMode::Pid, false);
        assert_eq!(entries[0].pid, 30);
    }

    #[test]
    fn test_sort_by_mem_descending() {
        let mut entries = make_entries(&[(1, "a", 0.0, 2.0), (2, "b", 0.0, 8.0), (3, "c", 0.0, 5.0)]);
        sort_proc_entries(&mut entries, ProcSortMode::Mem, false);
        assert_eq!(entries[0].pid, 2);
    }

    #[test]
    fn test_sort_by_user_ascending() {
        let mut entries = make_entries(&[(1, "zara", 0.0, 0.0), (2, "alice", 0.0, 0.0), (3, "bob", 0.0, 0.0)]);
        sort_proc_entries(&mut entries, ProcSortMode::User, true);
        assert_eq!(entries[0].user, "alice");
        assert_eq!(entries[1].user, "bob");
        assert_eq!(entries[2].user, "zara");
    }

    #[test]
    fn test_sort_by_command_ascending() {
        let mut entries = make_entries(&[(1, "zsh", 0.0, 0.0), (2, "bash", 0.0, 0.0), (3, "cargo", 0.0, 0.0)]);
        sort_proc_entries(&mut entries, ProcSortMode::Command, true);
        assert_eq!(entries[0].command, "bash");
        assert_eq!(entries[1].command, "cargo");
        assert_eq!(entries[2].command, "zsh");
    }

    #[test]
    fn test_format_elapsed_seconds() {
        assert_eq!(format_elapsed(45), "00:00:45");
    }

    #[test]
    fn test_format_elapsed_minutes() {
        assert_eq!(format_elapsed(90), "00:01:30");
    }

    #[test]
    fn test_format_elapsed_hours() {
        assert_eq!(format_elapsed(3661), "01:01:01");
    }

    #[test]
    fn test_format_elapsed_days() {
        assert_eq!(format_elapsed(86400 + 3600 + 60 + 1), "1d 01:01:01");
    }

    #[test]
    fn test_proc_sort_mode_label() {
        assert_eq!(ProcSortMode::Cpu.label(), "CPU");
        assert_eq!(ProcSortMode::Mem.label(), "MEM");
        assert_eq!(ProcSortMode::Pid.label(), "PID");
        assert_eq!(ProcSortMode::User.label(), "USER");
        assert_eq!(ProcSortMode::Command.label(), "CMD");
    }

    #[test]
    fn test_proc_sort_mode_default_asc() {
        assert!(!ProcSortMode::Cpu.default_asc());
        assert!(!ProcSortMode::Mem.default_asc());
        assert!(ProcSortMode::Pid.default_asc());
        assert!(ProcSortMode::User.default_asc());
        assert!(ProcSortMode::Command.default_asc());
    }

    #[test]
    fn test_classify_fd_socket() {
        assert_eq!(classify_fd(5, "socket:[12345]"), FdType::Socket);
    }

    #[test]
    fn test_classify_fd_pipe() {
        assert_eq!(classify_fd(5, "pipe:[67890]"), FdType::Pipe);
    }

    #[test]
    fn test_classify_fd_anon() {
        assert_eq!(classify_fd(5, "anon_inode:[eventfd]"), FdType::Anon);
    }

    #[test]
    fn test_classify_fd_stdin() {
        assert_eq!(classify_fd(0, "/dev/pts/0"), FdType::Stdin);
    }

    #[test]
    fn test_classify_fd_stdout() {
        assert_eq!(classify_fd(1, "/dev/pts/0"), FdType::Stdout);
    }

    #[test]
    fn test_classify_fd_stderr() {
        assert_eq!(classify_fd(2, "/dev/pts/0"), FdType::Stderr);
    }

    #[test]
    fn test_classify_fd_file() {
        assert_eq!(classify_fd(5, "/home/user/file.txt"), FdType::File);
    }

    #[test]
    fn test_classify_fd_other() {
        assert_eq!(classify_fd(5, "?"), FdType::Other);
    }

    #[test]
    fn test_fd_type_tag() {
        assert_eq!(FdType::Socket.tag(), "socket");
        assert_eq!(FdType::Pipe.tag(), "pipe  ");
        assert_eq!(FdType::File.tag(), "file  ");
        assert_eq!(FdType::Stdin.tag(), "stdin ");
    }
}
