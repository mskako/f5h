mod app;
mod config;
mod fs_utils;
mod proc;
mod ui;
mod keys;

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::{io::stdout, path::PathBuf, time::Duration};

use app::{App, DialogKind, DirJumpDialog, FileDialog, FuncDialog, GitDialog, GitDialogState, RunDialog, SearchState, SortMode};
use config::{Action, load_config, lookup_action};
use fs_utils::{open_in_program, run_command, shell_quote};
use std::sync::mpsc;
use ui::HEADER_ROWS;

/// プロセス一覧をリフレッシュし、カーソルを PID で追跡する。
/// 旧カーソル位置の PID を記憶し、リフレッシュ後に同じ PID を探して復元する。
/// PID が消えた場合はインデックスをクランプする。
macro_rules! refresh_procs {
    ($app:expr) => {{
        let tracked_pid = if $app.proc_tree {
            $app.proc_tree_rows.get($app.proc_cursor)
                .and_then(|r| $app.proc_entries.get(r.idx))
                .map(|e| e.pid)
        } else {
            $app.proc_entries.get($app.proc_cursor).map(|e| e.pid)
        };
        let mut entries = proc::get_proc_list();
        proc::sort_proc_entries(&mut entries, $app.proc_sort, $app.proc_sort_asc);
        $app.proc_entries = entries;
        if $app.proc_tree {
            $app.proc_tree_rows = proc::build_proc_tree(&$app.proc_entries);
            let n_rows = $app.proc_tree_rows.len();
            $app.proc_cursor = tracked_pid
                .and_then(|pid| $app.proc_tree_rows.iter().position(|r|
                    $app.proc_entries.get(r.idx).map(|e| e.pid) == Some(pid)))
                .unwrap_or_else(|| $app.proc_cursor.min(n_rows.saturating_sub(1)));
            if let Some(r) = $app.proc_tree_rows.get($app.proc_cursor) {
                let pid = $app.proc_entries[r.idx].pid;
                $app.proc_detail = proc::get_proc_detail(pid, &$app.proc_entries);
            }
        } else {
            $app.proc_cursor = tracked_pid
                .and_then(|pid| $app.proc_entries.iter().position(|e| e.pid == pid))
                .unwrap_or_else(|| $app.proc_cursor.min($app.proc_entries.len().saturating_sub(1)));
            if let Some(e) = $app.proc_entries.get($app.proc_cursor) {
                let pid = e.pid;
                $app.proc_detail = proc::get_proc_detail(pid, &$app.proc_entries);
            }
        }
    }};
}

fn main() -> Result<()> {
    let config = load_config();

    // ターミナルタブタイトルを設定
    print!("\x1b]0;\u{1F365} f5h\x07");
    use std::io::Write;
    std::io::stdout().flush().ok();

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;

    let mut app = App::new(config)?;
    let mut git_task: Option<mpsc::Receiver<anyhow::Result<()>>> = None;

    while !app.quit {
        // バックグラウンド git タスクの完了チェック
        if let Some(ref rx) = git_task {
            match rx.try_recv() {
                Ok(Ok(())) => {
                    app.git_running = false;
                    git_task = None;
                    app.reload();
                    app.success_msg = Some(if app.lang_en {
                        "Git operation completed successfully.".to_string()
                    } else {
                        "Git 操作が完了しました。".to_string()
                    });
                }
                Ok(Err(e)) => {
                    app.git_running = false;
                    git_task = None;
                    app.error_msg = Some(e.to_string());
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    app.git_running = false;
                    git_task = None;
                }
            }
        }
        let term_h = terminal.size()?.height as usize;
        let lh = term_h.saturating_sub(1 + 2 + HEADER_ROWS as usize).max(1);

        terminal.draw(|f| ui::ui(f, &app))?;

        if !event::poll(Duration::from_millis(500))? {
            // proc モード中はタイムアウトのたびに自動リフレッシュ
            if app.proc_mode && app.proc_signal_menu.is_none() && !app.fd_mode {
                refresh_procs!(app);
            }
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Press {
            // エラーダイアログ表示中は任意のキーで閉じる
            if app.error_msg.is_some() {
                app.error_msg = None;
                continue;
            }
            // 成功ダイアログ表示中は任意のキーで閉じる
            if app.success_msg.is_some() {
                app.success_msg = None;
                continue;
            }
            // ヘルプオーバーレイ表示中は任意のキーで閉じる
            if app.show_help {
                app.show_help = false;
                continue;
            }
            // ── proc モード ───────────────────────────────────────
            if app.proc_mode {
                // ── fd モード ─────────────────────────────────────
                if app.fd_mode {
                    let fd_lh = term_h.saturating_sub(1 + 2 + ui::PROC_LIST_HEADER as usize).max(1);
                    match (key.code, key.modifiers) {
                        (KeyCode::Esc, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => {
                            app.fd_mode = false;
                        }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                            if app.fd_cursor > 0 { app.fd_cursor -= 1; }
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                            if app.fd_cursor + 1 < app.fd_entries.len() { app.fd_cursor += 1; }
                        }
                        (KeyCode::Char('g'), KeyModifiers::NONE) => { app.fd_cursor = 0; }
                        (KeyCode::Char('G'), _) => {
                            app.fd_cursor = app.fd_entries.len().saturating_sub(1);
                        }
                        (KeyCode::PageUp, _) => {
                            app.fd_cursor = app.fd_cursor.saturating_sub(fd_lh);
                        }
                        (KeyCode::PageDown, _) => {
                            let n = app.fd_entries.len();
                            app.fd_cursor = (app.fd_cursor + fd_lh).min(n.saturating_sub(1));
                        }
                        (KeyCode::Char('r'), KeyModifiers::NONE) => {
                            match proc::get_fd_list(app.fd_pid) {
                                Ok(list) => { app.fd_entries = list; app.fd_error = None; }
                                Err(e)   => { app.fd_entries.clear(); app.fd_error = Some(e); }
                            }
                            app.fd_cursor = app.fd_cursor.min(app.fd_entries.len().saturating_sub(1));
                        }
                        (KeyCode::F(1), KeyModifiers::NONE) => {
                            app.show_help = true;
                        }
                        _ => {}
                    }
                    continue;
                }

                // ── シグナルメニュー ────────────────────────────────
                if app.proc_signal_menu.is_some() {
                    match (key.code, key.modifiers) {
                        (KeyCode::Esc, _) => { app.proc_signal_menu = None; }
                        (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                            if let Some(ref mut m) = app.proc_signal_menu {
                                if m.cursor > 0 { m.cursor -= 1; }
                            }
                        }
                        (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                            if let Some(ref mut m) = app.proc_signal_menu {
                                if m.cursor + 1 < proc::SIGNAL_ITEMS.len() { m.cursor += 1; }
                            }
                        }
                        (KeyCode::Enter, KeyModifiers::NONE) => {
                            if let Some(ref m) = app.proc_signal_menu {
                                let (sig, _, _, _) = proc::SIGNAL_ITEMS[m.cursor];
                                match proc::kill_pid(m.pid, sig) {
                                    Ok(()) => {
                                        app.success_msg = Some(format!(
                                            "Sent signal {} to {} (PID {})", sig, m.proc_name, m.pid
                                        ));
                                    }
                                    Err(e) => { app.error_msg = Some(e.to_string()); }
                                }
                            }
                            app.proc_signal_menu = None;
                            refresh_procs!(app);
                        }
                        (KeyCode::Char(c), KeyModifiers::NONE) => {
                            let sig_opt = proc::SIGNAL_ITEMS.iter()
                                .find(|(_, key_ch, _, _)| *key_ch == c);
                            if let Some(&(sig, _, _, _)) = sig_opt {
                                if let Some(ref m) = app.proc_signal_menu {
                                    match proc::kill_pid(m.pid, sig) {
                                        Ok(()) => {
                                            app.success_msg = Some(format!(
                                                "Sent signal {} to {} (PID {})", sig, m.proc_name, m.pid
                                            ));
                                        }
                                        Err(e) => { app.error_msg = Some(e.to_string()); }
                                    }
                                }
                                app.proc_signal_menu = None;
                                refresh_procs!(app);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }

                // ── proc リストナビゲーション ───────────────────────
                let proc_lh = term_h
                    .saturating_sub(1 + 2 + ui::HEADER_ROWS as usize + 1)
                    .max(1);
                let proc_n = if app.proc_tree { app.proc_tree_rows.len() } else { app.proc_entries.len() };
                let prev_cursor = app.proc_cursor;
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => {
                        app.proc_mode = false;
                        app.proc_signal_menu = None;
                        app.fd_mode = false;
                    }
                    (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                        if app.proc_cursor > 0 { app.proc_cursor -= 1; }
                    }
                    (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                        if app.proc_cursor + 1 < proc_n { app.proc_cursor += 1; }
                    }
                    (KeyCode::Char('g'), KeyModifiers::NONE) => { app.proc_cursor = 0; }
                    (KeyCode::Char('G'), _) => {
                        app.proc_cursor = proc_n.saturating_sub(1);
                    }
                    (KeyCode::PageUp, _) => {
                        app.proc_cursor = app.proc_cursor.saturating_sub(proc_lh);
                    }
                    (KeyCode::PageDown, _) => {
                        app.proc_cursor = (app.proc_cursor + proc_lh).min(proc_n.saturating_sub(1));
                    }
                    (KeyCode::Char('r'), KeyModifiers::NONE) => {
                        refresh_procs!(app);
                    }
                    (KeyCode::Char('t'), KeyModifiers::NONE) => {
                        // ツリーモード切替
                        let cur_pid = if app.proc_tree {
                            app.proc_tree_rows.get(app.proc_cursor)
                                .and_then(|r| app.proc_entries.get(r.idx))
                                .map(|e| e.pid)
                        } else {
                            app.proc_entries.get(app.proc_cursor).map(|e| e.pid)
                        };
                        app.proc_tree = !app.proc_tree;
                        if app.proc_tree {
                            app.proc_tree_rows = proc::build_proc_tree(&app.proc_entries);
                            app.proc_cursor = cur_pid
                                .and_then(|pid| app.proc_tree_rows.iter().position(|r|
                                    app.proc_entries.get(r.idx).map(|e| e.pid) == Some(pid)))
                                .unwrap_or(0);
                        } else {
                            app.proc_cursor = cur_pid
                                .and_then(|pid| app.proc_entries.iter().position(|e| e.pid == pid))
                                .unwrap_or(0);
                        }
                        app.proc_offset = 0;
                    }
                    (KeyCode::Char('x'), KeyModifiers::NONE) => {
                        let entry_opt = if app.proc_tree {
                            app.proc_tree_rows.get(app.proc_cursor).and_then(|r| app.proc_entries.get(r.idx))
                        } else {
                            app.proc_entries.get(app.proc_cursor)
                        };
                        if let Some(entry) = entry_opt {
                            app.proc_signal_menu = Some(proc::ProcSignalMenu {
                                cursor: 0,
                                pid: entry.pid,
                                proc_name: entry.command.clone(),
                            });
                        }
                    }
                    (KeyCode::Enter, KeyModifiers::NONE)
                    | (KeyCode::Char('f'), KeyModifiers::NONE) => {
                        // fd モードを開く
                        let entry_opt = if app.proc_tree {
                            app.proc_tree_rows.get(app.proc_cursor).and_then(|r| app.proc_entries.get(r.idx))
                        } else {
                            app.proc_entries.get(app.proc_cursor)
                        };
                        if let Some(entry) = entry_opt {
                            app.fd_pid = entry.pid;
                            app.fd_proc_name = entry.command.clone();
                            match proc::get_fd_list(entry.pid) {
                                Ok(list) => { app.fd_entries = list; app.fd_error = None; }
                                Err(e)   => { app.fd_entries.clear(); app.fd_error = Some(e); }
                            }
                            app.fd_cursor = 0;
                            app.fd_offset = 0;
                            app.fd_mode = true;
                        }
                    }
                    (KeyCode::Char('s'), KeyModifiers::NONE) => {
                        use proc::ProcSortMode::*;
                        const MODES: &[proc::ProcSortMode] = &[Cpu, Mem, Pid, User, Command];
                        let idx = MODES.iter().position(|&m| m == app.proc_sort).unwrap_or(0);
                        let next = MODES[(idx + 1) % MODES.len()];
                        if next == app.proc_sort {
                            app.proc_sort_asc = !app.proc_sort_asc;
                        } else {
                            app.proc_sort = next;
                            app.proc_sort_asc = next.default_asc();
                        }
                        refresh_procs!(app);
                        app.proc_cursor = 0;
                        app.proc_offset = 0;
                    }
                    (KeyCode::F(1), KeyModifiers::NONE) => {
                        app.show_help = true;
                    }
                    _ => {}
                }
                // カーソルが移動したら詳細を更新
                if app.proc_cursor != prev_cursor {
                    let entry_opt = if app.proc_tree {
                        app.proc_tree_rows.get(app.proc_cursor).and_then(|r| app.proc_entries.get(r.idx))
                    } else {
                        app.proc_entries.get(app.proc_cursor)
                    };
                    if let Some(e) = entry_opt {
                        let pid = e.pid;
                        app.proc_detail = proc::get_proc_detail(pid, &app.proc_entries);
                    }
                }
                continue;
            }
            // ── ソートダイアログ ──────────────────────────────────
            if app.show_sort_dialog {
                // opts の順番: 0=Name 1=Ext 2=Size 3=Date 4=None
                use SortMode::*;
                const SORT_OPTS: &[SortMode] = &[Name, Ext, Size, Date, None];
                macro_rules! apply_sort {
                    ($app:expr, $idx:expr) => {{
                        let mode = SORT_OPTS[$idx];
                        if $app.sort_mode == mode && mode != SortMode::None {
                            $app.sort_asc = !$app.sort_asc;
                        } else {
                            $app.sort_mode = mode;
                            $app.sort_asc = true;
                        }
                        $app.show_sort_dialog = false;
                        $app.reload();
                    }};
                }
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => { app.show_sort_dialog = false; }
                    (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                        if app.sort_cursor > 0 { app.sort_cursor -= 1; }
                    }
                    (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                        if app.sort_cursor + 1 < SORT_OPTS.len() { app.sort_cursor += 1; }
                    }
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        let idx = app.sort_cursor;
                        apply_sort!(app, idx);
                    }
                    (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) => { app.sort_cursor = 0; apply_sort!(app, 0); }
                    (KeyCode::Char('x'), _) | (KeyCode::Char('X'), _) => { app.sort_cursor = 1; apply_sort!(app, 1); }
                    (KeyCode::Char('s'), _) | (KeyCode::Char('S'), _) => { app.sort_cursor = 2; apply_sort!(app, 2); }
                    (KeyCode::Char('t'), _) | (KeyCode::Char('T'), _) => { app.sort_cursor = 3; apply_sort!(app, 3); }
                    (KeyCode::Char('u'), _) | (KeyCode::Char('U'), _) => { app.sort_cursor = 4; apply_sort!(app, 4); }
                    _ => {}
                }
                continue;
            }
            if app.func_dialog.is_some() {
                // ── 機能ダイアログ ────────────────────────────────
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => app.func_dialog = None,
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        if let Some(ref dlg) = app.func_dialog {
                            let line: String = dlg.input.iter().collect();
                            let selected = dlg.selected;
                            let mut parts = line.trim().splitn(2, ' ');
                            let cmd_raw = parts.next().unwrap_or("").to_string();
                            let arg = parts.next().unwrap_or("").trim().to_string();
                            // 完全一致しない場合は j/k で選択中の候補を使う
                            let cmd = if app::FUNC_CMDS.iter().any(|c| c.name == cmd_raw.as_str()) {
                                cmd_raw.clone()
                            } else {
                                app::FUNC_CMDS.iter()
                                    .filter(|c| c.name.starts_with(cmd_raw.as_str()))
                                    .nth(selected)
                                    .map(|c| c.name.to_string())
                                    .unwrap_or_else(|| cmd_raw.to_lowercase())
                            };
                            let cmd = cmd.to_lowercase();
                            app.func_dialog = None;
                            match cmd.as_str() {
                                "q" | "quit" => app.quit = true,
                                "mv" => {
                                    if arg.is_empty() {
                                        app.error_msg = Some("使い方: mv <新しい名前>".to_string());
                                    } else {
                                        let targets = app.collect_op_targets();
                                        if targets.is_empty() {
                                            app.error_msg = Some("対象ファイルがありません".to_string());
                                        } else {
                                            // タグなし or 単一: カーソルファイルのみリネーム
                                            let src = app.current_dir.join(&targets[0]);
                                            let dst = app.current_dir.join(&arg);
                                            match crate::fs_utils::move_path(&src, &dst) {
                                                Ok(()) => app.reload(),
                                                Err(e) => app.error_msg = Some(e.to_string()),
                                            }
                                        }
                                    }
                                }
                                "mkdir" => {
                                    app.file_dialog = Some(app::FileDialog {
                                        kind: app::DialogKind::Mkdir,
                                        input: arg.chars().collect(),
                                        cursor: arg.chars().count(),
                                        targets: vec![],
                                        dest: None,
                                        conflict_rename: false,
                                        error: None,
                                        overwrite: None,
                                    });
                                }
                                "proc" => {
                                    let mut entries = proc::get_proc_list();
                                    proc::sort_proc_entries(&mut entries, app.proc_sort, app.proc_sort_asc);
                                    app.proc_entries = entries;
                                    app.proc_cursor = 0;
                                    app.proc_offset = 0;
                                    if let Some(e) = app.proc_entries.first() {
                                        let pid = e.pid;
                                        app.proc_detail = proc::get_proc_detail(pid, &app.proc_entries);
                                    }
                                    app.proc_mode = true;
                                }
                                "help" => {
                                    // ヘルプ: ダイアログを再表示（入力クリア）
                                    app.func_dialog = Some(FuncDialog::default());
                                }
                                _ => {
                                    app.error_msg = Some(format!("不明なコマンド: {}", cmd));
                                }
                            }
                        }
                    }
                    (KeyCode::Up, _) => {
                        if let Some(ref mut d) = app.func_dialog {
                            if d.selected > 0 { d.selected -= 1; }
                        }
                    }
                    (KeyCode::Down, _) => {
                        if let Some(ref d) = app.func_dialog {
                            let query: String = d.input.iter().collect();
                            let cmd_name: &str = query.trim().splitn(2, ' ').next().unwrap_or("");
                            let n = app::FUNC_CMDS.iter()
                                .filter(|c| c.name.starts_with(cmd_name))
                                .count();
                            if d.selected + 1 < n {
                                app.func_dialog.as_mut().unwrap().selected += 1;
                            }
                        }
                    }
                    (KeyCode::Tab, _) => {
                        // 先頭候補のコマンド名を補完
                        if let Some(ref mut d) = app.func_dialog {
                            let query: String = d.input.iter().collect();
                            let cmd_name: &str = query.trim().splitn(2, ' ').next().unwrap_or("");
                            let candidate = app::FUNC_CMDS.iter()
                                .filter(|c| c.name.starts_with(cmd_name))
                                .nth(d.selected);
                            if let Some(c) = candidate {
                                let new_input: Vec<char> = format!("{} ", c.name).chars().collect();
                                let new_cursor = new_input.len();
                                d.input = new_input;
                                d.cursor = new_cursor;
                                d.selected = 0;
                            }
                        }
                    }
                    (KeyCode::Left, _) => {
                        if let Some(ref mut d) = app.func_dialog {
                            if d.cursor > 0 { d.cursor -= 1; }
                        }
                    }
                    (KeyCode::Right, _) => {
                        if let Some(ref mut d) = app.func_dialog {
                            if d.cursor < d.input.len() { d.cursor += 1; }
                        }
                    }
                    (KeyCode::Home, _) => {
                        if let Some(ref mut d) = app.func_dialog { d.cursor = 0; }
                    }
                    (KeyCode::End, _) => {
                        if let Some(ref mut d) = app.func_dialog {
                            d.cursor = d.input.len();
                        }
                    }
                    (KeyCode::Backspace, _) => {
                        if let Some(ref mut d) = app.func_dialog {
                            if d.cursor > 0 { d.cursor -= 1; d.input.remove(d.cursor); }
                            d.selected = 0;
                        }
                    }
                    (KeyCode::Delete, _) => {
                        if let Some(ref mut d) = app.func_dialog {
                            if d.cursor < d.input.len() { d.input.remove(d.cursor); }
                            d.selected = 0;
                        }
                    }
                    (KeyCode::Char(c), KeyModifiers::NONE)
                    | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                        if let Some(ref mut d) = app.func_dialog {
                            d.input.insert(d.cursor, c);
                            d.cursor += 1;
                            d.selected = 0;
                        }
                    }
                    _ => {}
                }
                continue;
            }
            // ── 確定済み検索: ESC のみ受け付けてハイライトを消す ───
            if app.search.as_ref().map(|s| s.confirmed).unwrap_or(false) {
                if key.code == KeyCode::Esc {
                    app.search = None;
                }
                // 確定済みの場合は通常キー処理へ fall-through（continue しない）
            }
            // ── 検索入力モード（未確定） ──────────────────────────
            else if app.search.is_some() {
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => {
                        // キャンセル: 元の位置に戻る
                        if let Some(s) = app.search.take() {
                            app.cursor = s.origin;
                        }
                    }
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        // 確定: バーを閉じるが matches は維持してハイライト継続
                        if let Some(ref mut s) = app.search {
                            app.last_search = s.input.iter().collect();
                            s.confirmed = true;
                        }
                    }
                    (KeyCode::Left, _) => {
                        if let Some(ref mut s) = app.search {
                            if s.cursor > 0 { s.cursor -= 1; }
                        }
                    }
                    (KeyCode::Right, _) => {
                        if let Some(ref mut s) = app.search {
                            if s.cursor < s.input.len() { s.cursor += 1; }
                        }
                    }
                    (KeyCode::Backspace, _) => {
                        if let Some(ref mut s) = app.search {
                            if s.cursor > 0 {
                                s.cursor -= 1;
                                s.input.remove(s.cursor);
                            }
                        }
                        let pat = app.search.as_ref().map(|s| s.input.iter().collect::<String>()).unwrap_or_default();
                        if let Some(ref mut s) = app.search {
                            s.matches = app.entries.iter().enumerate()
                                .filter(|(_, e)| {
                                    if e.name == ".." || pat.is_empty() { return false; }
                                    e.name.to_lowercase().contains(&pat.to_lowercase())
                                })
                                .map(|(i, _)| i).collect();
                            s.match_idx = 0;
                            if let Some(&idx) = s.matches.first() { app.cursor = idx; }
                        }
                    }
                    (KeyCode::Char(c), KeyModifiers::NONE)
                    | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                        if let Some(ref mut s) = app.search {
                            s.input.insert(s.cursor, c);
                            s.cursor += 1;
                        }
                        let pat = app.search.as_ref().map(|s| s.input.iter().collect::<String>()).unwrap_or_default();
                        if let Some(ref mut s) = app.search {
                            s.matches = app.entries.iter().enumerate()
                                .filter(|(_, e)| {
                                    if e.name == ".." || pat.is_empty() { return false; }
                                    e.name.to_lowercase().contains(&pat.to_lowercase())
                                })
                                .map(|(i, _)| i).collect();
                            s.match_idx = 0;
                            if let Some(&idx) = s.matches.first() { app.cursor = idx; }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if app.git_dialog.is_some() {
                keys::handle_git_dialog_key(&mut app, key, &mut git_task, lh);
                continue;
            }
            if app.dir_jump_dialog.is_some() {
                // ── Dir jump dialog ───────────────────────────────
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => { app.dir_jump_dialog = None; }
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        if let Some(ref dlg) = app.dir_jump_dialog {
                            let input: String = dlg.input.iter().collect();
                            let expanded = if input.starts_with('~') {
                                if let Ok(home) = std::env::var("HOME") {
                                    input.replacen('~', &home, 1)
                                } else { input.clone() }
                            } else { input.clone() };
                            let path = std::path::PathBuf::from(&expanded);
                            let _ = dlg;
                            match app.enter_dir_abs(&path) {
                                Ok(()) => { app.dir_jump_dialog = None; }
                                Err(e) => {
                                    if let Some(ref mut d) = app.dir_jump_dialog {
                                        d.error = Some(e.to_string());
                                    }
                                }
                            }
                        }
                    }
                    (KeyCode::Left, _) => {
                        if let Some(ref mut d) = app.dir_jump_dialog {
                            if d.cursor > 0 { d.cursor -= 1; }
                        }
                    }
                    (KeyCode::Right, _) => {
                        if let Some(ref mut d) = app.dir_jump_dialog {
                            if d.cursor < d.input.len() { d.cursor += 1; }
                        }
                    }
                    (KeyCode::Home, _) => {
                        if let Some(ref mut d) = app.dir_jump_dialog { d.cursor = 0; }
                    }
                    (KeyCode::End, _) => {
                        if let Some(ref mut d) = app.dir_jump_dialog {
                            d.cursor = d.input.len();
                        }
                    }
                    (KeyCode::Backspace, _) => {
                        if let Some(ref mut d) = app.dir_jump_dialog {
                            if d.cursor > 0 { d.cursor -= 1; d.input.remove(d.cursor); }
                            d.error = None;
                        }
                    }
                    (KeyCode::Delete, _) => {
                        if let Some(ref mut d) = app.dir_jump_dialog {
                            if d.cursor < d.input.len() { d.input.remove(d.cursor); }
                            d.error = None;
                        }
                    }
                    (KeyCode::Char(c), KeyModifiers::NONE)
                    | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                        if let Some(ref mut d) = app.dir_jump_dialog {
                            d.input.insert(d.cursor, c);
                            d.cursor += 1;
                            d.error = None;
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if app.run_dialog.is_some() {
                // ── Run dialog input ──────────────────────────────
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => app.run_dialog = None,
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        if let Some(ref dlg) = app.run_dialog {
                            let cmd: String = dlg.input.iter().collect();
                            app.run_dialog = None;
                            if !cmd.trim().is_empty() {
                                run_command(&cmd, &app.current_dir)?;
                                terminal.clear()?;
                                app.reload();
                            }
                        }
                    }
                    (KeyCode::Left, _) =>
                    {
                        #[allow(clippy::collapsible_if)]
                        if let Some(ref mut d) = app.run_dialog {
                            if d.cursor > 0 {
                                d.cursor -= 1;
                            }
                        }
                    }
                    (KeyCode::Right, _) =>
                    {
                        #[allow(clippy::collapsible_if)]
                        if let Some(ref mut d) = app.run_dialog {
                            if d.cursor < d.input.len() {
                                d.cursor += 1;
                            }
                        }
                    }
                    (KeyCode::Home, _) => {
                        if let Some(ref mut d) = app.run_dialog {
                            d.cursor = 0;
                        }
                    }
                    (KeyCode::End, _) => {
                        if let Some(ref mut d) = app.run_dialog {
                            d.cursor = d.input.len();
                        }
                    }
                    (KeyCode::Backspace, _) =>
                    {
                        #[allow(clippy::collapsible_if)]
                        if let Some(ref mut d) = app.run_dialog {
                            if d.cursor > 0 {
                                d.cursor -= 1;
                                d.input.remove(d.cursor);
                            }
                        }
                    }
                    (KeyCode::Delete, _) =>
                    {
                        #[allow(clippy::collapsible_if)]
                        if let Some(ref mut d) = app.run_dialog {
                            if d.cursor < d.input.len() {
                                d.input.remove(d.cursor);
                            }
                        }
                    }
                    (KeyCode::Char(c), KeyModifiers::NONE)
                    | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                        if let Some(ref mut d) = app.run_dialog {
                            d.input.insert(d.cursor, c);
                            d.cursor += 1;
                        }
                    }
                    _ => {}
                }
            } else if app.file_dialog.is_some() {
            } else if app.file_dialog.is_some() {
                keys::handle_file_dialog_key(&mut app, key, &mut terminal, lh)?;
            } else if app.tree_open && app.tree_focus {
                // ── Tree pane navigation ──────────────────────────
                match (key.code, key.modifiers) {
                    (KeyCode::Up, KeyModifiers::NONE)
                    | (KeyCode::Char('k'), KeyModifiers::NONE) => app.tree_move_up(lh),
                    (KeyCode::Down, KeyModifiers::NONE)
                    | (KeyCode::Char('j'), KeyModifiers::NONE) => app.tree_move_down(lh),
                    (KeyCode::Right, KeyModifiers::NONE)
                    | (KeyCode::Char('l'), KeyModifiers::NONE) => app.tree_expand(),
                    (KeyCode::Left, KeyModifiers::NONE)
                    | (KeyCode::Char('h'), KeyModifiers::NONE)
                    | (KeyCode::Backspace, KeyModifiers::NONE) => app.tree_collapse(lh),
                    (KeyCode::Enter, KeyModifiers::NONE) => app.tree_focus = false,
                    (KeyCode::Char('t'), KeyModifiers::NONE)
                    | (KeyCode::Tab, KeyModifiers::NONE) => {
                        app.tree_open = false;
                        app.tree_focus = false;
                    }
                    (KeyCode::Char('q'), KeyModifiers::NONE) => app.quit = true,
                    _ => {}
                }
            } else if key.code == KeyCode::F(1) && key.modifiers == KeyModifiers::NONE {
                app.show_help = true;
            } else if let Some(action) = lookup_action(&app.keymap, key.code, key.modifiers) {
                // ── Main keymap dispatch ──────────────────────────
                match action {
                    Action::MoveUp => app.move_up(lh),
                    Action::MoveDown => app.move_down(lh),
                    Action::MoveLeft => app.move_left(lh),
                    Action::MoveRight => app.move_right(lh),
                    Action::FirstEntry => {
                        app.cursor = app.first_list_entry_index();
                        app.update_file_info();
                    }
                    Action::LastEntry => {
                        app.cursor = app.last_list_entry_index();
                        app.update_file_info();
                    }
                    Action::PageUp => app.page_up(lh),
                    Action::PageDown => app.page_down(lh),
                    Action::Enter => {
                        if let Some(e) = app.entries.get(app.cursor) {
                            if e.is_dir {
                                let n = e.name.clone();
                                match app.enter_dir(&n) {
                                    Ok(()) => {
                                        if app.tree_open {
                                            app.tree_rebuild();
                                        }
                                    }
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            } else {
                                let path = app.current_dir.join(&e.name);
                                let ext = std::path::Path::new(&e.name)
                                    .extension()
                                    .and_then(|x| x.to_str())
                                    .unwrap_or("")
                                    .to_lowercase();
                                let program = app
                                    .ext_programs
                                    .get(&ext)
                                    .cloned()
                                    .unwrap_or_else(|| app.pager.clone());
                                match open_in_program(&program, &path) {
                                    Ok(()) => terminal.clear()?,
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            }
                        }
                    }
                    Action::ParentDir => match app.enter_dir("..") {
                        Ok(()) => {
                            if app.tree_open {
                                app.tree_rebuild();
                            }
                        }
                        Err(e) => app.error_msg = Some(e.to_string()),
                    },
                    Action::HomeDir => {
                        if let Some(home) = std::env::var_os("HOME") {
                            let prev = app.current_dir.clone();
                            app.current_dir = PathBuf::from(home);
                            app.cursor = 0;
                            match app.load_entries() {
                                Ok(()) => {
                                    app.update_file_info();
                                    if app.tree_open {
                                        app.tree_rebuild();
                                    }
                                }
                                Err(e) => {
                                    app.current_dir = prev;
                                    let _ = app.load_entries();
                                    app.error_msg = Some(e.to_string());
                                }
                            }
                        }
                    }
                    Action::TagMove => app.tag_toggle_move(lh),
                    Action::TagAll => app.tag_all(),
                    Action::Quit => app.quit = true,
                    Action::ColMode1 => app.col_mode = 1,
                    Action::ColMode2 => app.col_mode = 2,
                    Action::ColMode3 => app.col_mode = 3,
                    Action::ColMode5 => app.col_mode = 5,
                    Action::Run => {
                        let args: String = {
                            let tagged: Vec<String> = app
                                .entries
                                .iter()
                                .zip(app.tagged.iter())
                                .filter(|(_, t)| **t)
                                .map(|(e, _)| shell_quote(&e.name))
                                .collect();
                            if !tagged.is_empty() {
                                tagged.join(" ")
                            } else {
                                app.entries
                                    .get(app.cursor)
                                    .map(|e| shell_quote(&e.name))
                                    .unwrap_or_default()
                            }
                        };
                        let input: Vec<char> = args.chars().collect();
                        app.run_dialog = Some(RunDialog { input, cursor: 0 });
                    }
                    Action::Func => {
                        app.func_dialog = Some(FuncDialog::default());
                    }
                    Action::Search => {
                        app.search = Some(SearchState {
                            input: vec![],
                            cursor: 0,
                            origin: app.cursor,
                            matches: vec![],
                            match_idx: 0,
                            confirmed: false,
                        });
                    }
                    Action::SearchNext => {
                        // 確定済み search があればその matches を、なければ last_search から再計算
                        let matches: Vec<usize> =
                            if let Some(ref s) = app.search.as_ref().filter(|s| s.confirmed) {
                                s.matches.clone()
                            } else if !app.last_search.is_empty() {
                                app.compute_search_matches(&app.last_search.clone())
                            } else { vec![] };
                        if !matches.is_empty() {
                            let next = matches.iter().position(|&i| i > app.cursor).unwrap_or(0);
                            app.cursor = matches[next];
                            if let Some(ref mut s) = app.search { s.match_idx = next; }
                        }
                    }
                    Action::SearchPrev => {
                        let matches: Vec<usize> =
                            if let Some(ref s) = app.search.as_ref().filter(|s| s.confirmed) {
                                s.matches.clone()
                            } else if !app.last_search.is_empty() {
                                app.compute_search_matches(&app.last_search.clone())
                            } else { vec![] };
                        if !matches.is_empty() {
                            let prev = matches.iter().rposition(|&i| i < app.cursor)
                                .unwrap_or(matches.len() - 1);
                            app.cursor = matches[prev];
                            if let Some(ref mut s) = app.search { s.match_idx = prev; }
                        }
                    }
                    Action::Sort => {
                        app.show_sort_dialog = true;
                        // sort_cursor を現在の sort_mode に合わせる
                        app.sort_cursor = match app.sort_mode {
                            SortMode::Name => 0,
                            SortMode::Ext  => 1,
                            SortMode::Size => 2,
                            SortMode::Date => 3,
                            SortMode::None => 4,
                        };
                    }
                    Action::Git => {
                        app.git_dialog = Some(GitDialog { state: GitDialogState::Menu });
                    }
                    Action::Edit => {
                        if let Some(e) = app.entries.get(app.cursor).filter(|e| !e.is_dir) {
                            let path = app.current_dir.join(&e.name);
                            match open_in_program(&app.editor, &path) {
                                Ok(()) => terminal.clear()?,
                                Err(e) => app.error_msg = Some(e.to_string()),
                            }
                        }
                    }
                    Action::Copy => {
                        let targets = app.collect_op_targets();
                        if !targets.is_empty() {
                            let input: Vec<char> =
                                app.current_dir.to_string_lossy().chars().collect();
                            let cursor = input.len();
                            app.file_dialog = Some(FileDialog {
                                kind: DialogKind::Copy,
                                input,
                                cursor,
                                targets,
                                dest: None,
                                conflict_rename: false,
                                error: None,
                                overwrite: None,
                            });
                        }
                    }
                    Action::Move => {
                        let targets = app.collect_op_targets();
                        if !targets.is_empty() {
                            let input: Vec<char> =
                                app.current_dir.to_string_lossy().chars().collect();
                            let cursor = input.len();
                            app.file_dialog = Some(FileDialog {
                                kind: DialogKind::Move,
                                input,
                                cursor,
                                targets,
                                dest: None,
                                conflict_rename: false,
                                error: None,
                                overwrite: None,
                            });
                        }
                    }
                    Action::Delete => {
                        let targets = app.collect_op_targets();
                        if !targets.is_empty() {
                            app.file_dialog = Some(FileDialog {
                                kind: DialogKind::DeleteConfirm,
                                input: vec![],
                                cursor: 0,
                                targets,
                                dest: None,
                                conflict_rename: false,
                                error: None,
                                overwrite: None,
                            });
                        }
                    }
                    Action::Rename => {
                        let targets = app.collect_op_targets();
                        if let Some(fname) = targets.first() {
                            let input: Vec<char> = fname.chars().collect();
                            let cursor = input.len();
                            app.file_dialog = Some(FileDialog {
                                kind: DialogKind::Rename,
                                input,
                                cursor,
                                targets: vec![fname.clone()],
                                dest: None,
                                conflict_rename: false,
                                error: None,
                                overwrite: None,
                            });
                        }
                    }
                    Action::Mkdir => {
                        app.file_dialog = Some(FileDialog {
                            kind: DialogKind::Mkdir,
                            input: vec![],
                            cursor: 0,
                            targets: vec![],
                            dest: None,
                            conflict_rename: false,
                            error: None,
                            overwrite: None,
                        });
                    }
                    Action::Attr => {
                        let targets = app.collect_op_targets();
                        if !targets.is_empty() {
                            let mode_s: Vec<char> = app
                                .entries
                                .get(app.cursor)
                                .map(|e| format!("{:04o}", e.mode & 0o7777))
                                .unwrap_or_default()
                                .chars()
                                .collect();
                            let cursor = mode_s.len();
                            app.file_dialog = Some(FileDialog {
                                kind: DialogKind::Attr,
                                input: mode_s,
                                cursor,
                                targets,
                                dest: None,
                                conflict_rename: false,
                                error: None,
                                overwrite: None,
                            });
                        }
                    }
                    Action::DirJump => {
                        // カレントディレクトリを初期値として入力欄に入れる
                        let init: Vec<char> = app.current_dir.to_string_lossy().chars().collect();
                        let len = init.len();
                        app.dir_jump_dialog = Some(DirJumpDialog {
                            input: init,
                            cursor: len,
                            error: None,
                        });
                    }
                    Action::TreeToggle => {
                        if app.tree_open {
                            app.tree_open = false;
                            app.tree_focus = false;
                        } else {
                            app.tree_rebuild();
                            app.tree_open = true;
                            app.tree_focus = true;
                        }
                    }
                }
            }
        }
    }

    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    // ターミナルタブタイトルをリセット
    print!("\x1b]0;\x07");
    std::io::stdout().flush().ok();

    Ok(())
}
