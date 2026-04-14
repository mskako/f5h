mod app;
mod config;
mod fs_utils;
mod ui;

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use std::{io::stdout, path::PathBuf, time::Duration};

use app::{App, DialogKind, FileDialog, FuncDialog, GitDialog, GitDialogState, RemoteOp, RunDialog, SearchState};
use config::{Action, load_config, lookup_action};
use fs_utils::{git_command_silent, git_fetch, git_merge_no_ff, git_pull, git_push, git_stash_push, git_stash_pop, open_in_program, run_command, shell_quote};
use std::sync::mpsc;
use ui::HEADER_ROWS;

fn main() -> Result<()> {
    let config = load_config();

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
            if app.func_dialog.is_some() {
                // ── 機能ダイアログ ────────────────────────────────
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => app.func_dialog = None,
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        if let Some(ref dlg) = app.func_dialog {
                            let line: String = dlg.input.iter().collect();
                            let mut parts = line.trim().splitn(2, ' ');
                            let cmd = parts.next().unwrap_or("").to_lowercase();
                            let arg = parts.next().unwrap_or("").trim().to_string();
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
            // ── 検索入力モード ────────────────────────────────────
            if app.search.is_some() {
                match (key.code, key.modifiers) {
                    (KeyCode::Esc, _) => {
                        // キャンセル: 元の位置に戻る
                        if let Some(s) = app.search.take() {
                            app.cursor = s.origin;
                        }
                    }
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        // 確定: 検索バーを閉じ last_search を保存
                        if let Some(s) = app.search.take() {
                            app.last_search = s.input.iter().collect();
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
                // ── Git submenu ───────────────────────────────────
                let state = app.git_dialog.as_ref().map(|d| d.state.clone());
                match state {
                    Some(GitDialogState::Menu) => {
                        match (key.code, key.modifiers) {
                            (KeyCode::Esc, _) => app.git_dialog = None,
                            (KeyCode::Char('a'), KeyModifiers::NONE) => {
                                // git add: tagged files or cursor file
                                let targets: Vec<String> = {
                                    let tagged: Vec<String> = app.entries.iter()
                                        .zip(app.tagged.iter())
                                        .filter(|(_, t)| **t)
                                        .map(|(e, _)| e.name.clone())
                                        .collect();
                                    if !tagged.is_empty() {
                                        tagged
                                    } else {
                                        app.entries.get(app.cursor)
                                            .filter(|e| e.name != "..")
                                            .map(|e| vec![e.name.clone()])
                                            .unwrap_or_default()
                                    }
                                };
                                app.git_dialog = None;
                                if targets.is_empty() {
                                    app.error_msg = Some("対象ファイルがありません".to_string());
                                } else {
                                    let args: Vec<&str> = std::iter::once("add")
                                        .chain(targets.iter().map(|s| s.as_str()))
                                        .collect();
                                    match git_command_silent(&args, &app.current_dir) {
                                        Ok(()) => app.reload(),
                                        Err(e) => app.error_msg = Some(e.to_string()),
                                    }
                                }
                            }
                            (KeyCode::Char('A'), KeyModifiers::NONE)
                            | (KeyCode::Char('A'), KeyModifiers::SHIFT) => {
                                app.git_dialog = None;
                                match git_command_silent(&["add", "."], &app.current_dir) {
                                    Ok(()) => app.reload(),
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            }
                            (KeyCode::Char('c'), KeyModifiers::NONE) => {
                                app.git_dialog = Some(GitDialog {
                                    state: GitDialogState::CommitMsg {
                                        input: vec![],
                                        cursor: 0,
                                    },
                                });
                            }
                            (KeyCode::Char('f'), KeyModifiers::NONE) => {
                                app.git_dialog = Some(GitDialog {
                                    state: GitDialogState::Passphrase {
                                        op: RemoteOp::Fetch,
                                        input: vec![],
                                        cursor: 0,
                                    },
                                });
                            }
                            (KeyCode::Char('p'), KeyModifiers::NONE) => {
                                app.git_dialog = Some(GitDialog {
                                    state: GitDialogState::Passphrase {
                                        op: RemoteOp::Push,
                                        input: vec![],
                                        cursor: 0,
                                    },
                                });
                            }
                            (KeyCode::Char('P'), KeyModifiers::NONE)
                            | (KeyCode::Char('P'), KeyModifiers::SHIFT) => {
                                app.git_dialog = Some(GitDialog {
                                    state: GitDialogState::Passphrase {
                                        op: RemoteOp::Pull,
                                        input: vec![],
                                        cursor: 0,
                                    },
                                });
                            }
                            (KeyCode::Char('s'), KeyModifiers::NONE) => {
                                app.git_dialog = Some(GitDialog {
                                    state: GitDialogState::SwitchBranch {
                                        input: vec![],
                                        cursor: 0,
                                    },
                                });
                            }
                            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                                app.git_dialog = None;
                                match git_merge_no_ff(&app.current_dir) {
                                    Ok(()) => app.reload(),
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            }
                            (KeyCode::Char('t'), KeyModifiers::NONE) => {
                                app.git_dialog = Some(GitDialog {
                                    state: GitDialogState::StashMsg {
                                        input: vec![],
                                        cursor: 0,
                                    },
                                });
                            }
                            (KeyCode::Char('T'), KeyModifiers::NONE)
                            | (KeyCode::Char('T'), KeyModifiers::SHIFT) => {
                                app.git_dialog = None;
                                match git_stash_pop(&app.current_dir) {
                                    Ok(()) => app.reload(),
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(GitDialogState::CommitMsg { .. }) => {
                        match (key.code, key.modifiers) {
                            (KeyCode::Esc, _) => {
                                app.git_dialog = Some(GitDialog { state: GitDialogState::Menu });
                            }
                            (KeyCode::Enter, KeyModifiers::NONE) => {
                                let msg: String = app.git_dialog.as_ref()
                                    .and_then(|d| if let GitDialogState::CommitMsg { ref input, .. } = d.state { Some(input.iter().collect()) } else { None })
                                    .unwrap_or_default();
                                app.git_dialog = None;
                                if msg.trim().is_empty() {
                                    app.error_msg = Some("コミットメッセージを入力してください".to_string());
                                } else {
                                    match git_command_silent(&["commit", "-m", &msg], &app.current_dir) {
                                        Ok(()) => app.reload(),
                                        Err(e) => app.error_msg = Some(e.to_string()),
                                    }
                                }
                            }
                            (KeyCode::Left, _) => {
                                if let Some(GitDialog { state: GitDialogState::CommitMsg { ref mut cursor, .. } }) = app.git_dialog {
                                    if *cursor > 0 { *cursor -= 1; }
                                }
                            }
                            (KeyCode::Right, _) => {
                                if let Some(GitDialog { state: GitDialogState::CommitMsg { ref input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor < input.len() { *cursor += 1; }
                                }
                            }
                            (KeyCode::Home, _) => {
                                if let Some(GitDialog { state: GitDialogState::CommitMsg { ref mut cursor, .. } }) = app.git_dialog {
                                    *cursor = 0;
                                }
                            }
                            (KeyCode::End, _) => {
                                if let Some(GitDialog { state: GitDialogState::CommitMsg { ref input, ref mut cursor } }) = app.git_dialog {
                                    *cursor = input.len();
                                }
                            }
                            (KeyCode::Backspace, _) => {
                                if let Some(GitDialog { state: GitDialogState::CommitMsg { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor > 0 { *cursor -= 1; input.remove(*cursor); }
                                }
                            }
                            (KeyCode::Delete, _) => {
                                if let Some(GitDialog { state: GitDialogState::CommitMsg { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor < input.len() { input.remove(*cursor); }
                                }
                            }
                            (KeyCode::Char(c), KeyModifiers::NONE)
                            | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                                if let Some(GitDialog { state: GitDialogState::CommitMsg { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    input.insert(*cursor, c);
                                    *cursor += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(GitDialogState::SwitchBranch { .. }) => {
                        match (key.code, key.modifiers) {
                            (KeyCode::Esc, _) => {
                                app.git_dialog = Some(GitDialog { state: GitDialogState::Menu });
                            }
                            (KeyCode::Enter, KeyModifiers::NONE) => {
                                let branch: String = app.git_dialog.as_ref()
                                    .and_then(|d| if let GitDialogState::SwitchBranch { ref input, .. } = d.state { Some(input.iter().collect()) } else { None })
                                    .unwrap_or_default();
                                app.git_dialog = None;
                                if branch.trim().is_empty() {
                                    app.error_msg = Some("ブランチ名を入力してください".to_string());
                                } else {
                                    match git_command_silent(&["switch", branch.trim()], &app.current_dir) {
                                        Ok(()) => app.reload(),
                                        Err(e) => app.error_msg = Some(e.to_string()),
                                    }
                                }
                            }
                            (KeyCode::Left, _) => {
                                if let Some(GitDialog { state: GitDialogState::SwitchBranch { ref mut cursor, .. } }) = app.git_dialog {
                                    if *cursor > 0 { *cursor -= 1; }
                                }
                            }
                            (KeyCode::Right, _) => {
                                if let Some(GitDialog { state: GitDialogState::SwitchBranch { ref input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor < input.len() { *cursor += 1; }
                                }
                            }
                            (KeyCode::Home, _) => {
                                if let Some(GitDialog { state: GitDialogState::SwitchBranch { ref mut cursor, .. } }) = app.git_dialog {
                                    *cursor = 0;
                                }
                            }
                            (KeyCode::End, _) => {
                                if let Some(GitDialog { state: GitDialogState::SwitchBranch { ref input, ref mut cursor } }) = app.git_dialog {
                                    *cursor = input.len();
                                }
                            }
                            (KeyCode::Backspace, _) => {
                                if let Some(GitDialog { state: GitDialogState::SwitchBranch { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor > 0 { *cursor -= 1; input.remove(*cursor); }
                                }
                            }
                            (KeyCode::Delete, _) => {
                                if let Some(GitDialog { state: GitDialogState::SwitchBranch { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor < input.len() { input.remove(*cursor); }
                                }
                            }
                            (KeyCode::Char(c), KeyModifiers::NONE)
                            | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                                if let Some(GitDialog { state: GitDialogState::SwitchBranch { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    input.insert(*cursor, c);
                                    *cursor += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(GitDialogState::StashMsg { .. }) => {
                        match (key.code, key.modifiers) {
                            (KeyCode::Esc, _) => {
                                app.git_dialog = Some(GitDialog { state: GitDialogState::Menu });
                            }
                            (KeyCode::Enter, KeyModifiers::NONE) => {
                                let msg: String = app.git_dialog.as_ref()
                                    .and_then(|d| if let GitDialogState::StashMsg { ref input, .. } = d.state { Some(input.iter().collect()) } else { None })
                                    .unwrap_or_default();
                                app.git_dialog = None;
                                match git_stash_push(&msg, &app.current_dir) {
                                    Ok(()) => app.reload(),
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            }
                            (KeyCode::Left, _) => {
                                if let Some(GitDialog { state: GitDialogState::StashMsg { ref mut cursor, .. } }) = app.git_dialog {
                                    if *cursor > 0 { *cursor -= 1; }
                                }
                            }
                            (KeyCode::Right, _) => {
                                if let Some(GitDialog { state: GitDialogState::StashMsg { ref input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor < input.len() { *cursor += 1; }
                                }
                            }
                            (KeyCode::Home, _) => {
                                if let Some(GitDialog { state: GitDialogState::StashMsg { ref mut cursor, .. } }) = app.git_dialog {
                                    *cursor = 0;
                                }
                            }
                            (KeyCode::End, _) => {
                                if let Some(GitDialog { state: GitDialogState::StashMsg { ref input, ref mut cursor } }) = app.git_dialog {
                                    *cursor = input.len();
                                }
                            }
                            (KeyCode::Backspace, _) => {
                                if let Some(GitDialog { state: GitDialogState::StashMsg { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor > 0 { *cursor -= 1; input.remove(*cursor); }
                                }
                            }
                            (KeyCode::Delete, _) => {
                                if let Some(GitDialog { state: GitDialogState::StashMsg { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    if *cursor < input.len() { input.remove(*cursor); }
                                }
                            }
                            (KeyCode::Char(c), KeyModifiers::NONE)
                            | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                                if let Some(GitDialog { state: GitDialogState::StashMsg { ref mut input, ref mut cursor } }) = app.git_dialog {
                                    input.insert(*cursor, c);
                                    *cursor += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(GitDialogState::Passphrase { .. }) => {
                        match (key.code, key.modifiers) {
                            (KeyCode::Esc, _) => {
                                app.git_dialog = Some(GitDialog { state: GitDialogState::Menu });
                            }
                            (KeyCode::Enter, KeyModifiers::NONE) => {
                                let (op, passphrase): (RemoteOp, String) = match app.git_dialog.as_ref() {
                                    Some(GitDialog { state: GitDialogState::Passphrase { op, input, .. } }) => {
                                        (*op, input.iter().collect())
                                    }
                                    _ => unreachable!(),
                                };
                                app.git_dialog = None;
                                app.git_running = true;
                                let dir = app.current_dir.clone();
                                let (tx, rx) = mpsc::channel();
                                std::thread::spawn(move || {
                                    let result = match op {
                                        RemoteOp::Fetch => git_fetch(&dir, &passphrase),
                                        RemoteOp::Push  => git_push(&dir, &passphrase),
                                        RemoteOp::Pull  => git_pull(&dir, &passphrase),
                                    };
                                    let _ = tx.send(result);
                                });
                                git_task = Some(rx);
                            }
                            (KeyCode::Left, _) => {
                                if let Some(GitDialog { state: GitDialogState::Passphrase { ref mut cursor, .. } }) = app.git_dialog {
                                    if *cursor > 0 { *cursor -= 1; }
                                }
                            }
                            (KeyCode::Right, _) => {
                                if let Some(GitDialog { state: GitDialogState::Passphrase { ref input, ref mut cursor, .. } }) = app.git_dialog {
                                    if *cursor < input.len() { *cursor += 1; }
                                }
                            }
                            (KeyCode::Home, _) => {
                                if let Some(GitDialog { state: GitDialogState::Passphrase { ref mut cursor, .. } }) = app.git_dialog {
                                    *cursor = 0;
                                }
                            }
                            (KeyCode::End, _) => {
                                if let Some(GitDialog { state: GitDialogState::Passphrase { ref input, ref mut cursor, .. } }) = app.git_dialog {
                                    *cursor = input.len();
                                }
                            }
                            (KeyCode::Backspace, _) => {
                                if let Some(GitDialog { state: GitDialogState::Passphrase { ref mut input, ref mut cursor, .. } }) = app.git_dialog {
                                    if *cursor > 0 { *cursor -= 1; input.remove(*cursor); }
                                }
                            }
                            (KeyCode::Delete, _) => {
                                if let Some(GitDialog { state: GitDialogState::Passphrase { ref mut input, ref mut cursor, .. } }) = app.git_dialog {
                                    if *cursor < input.len() { input.remove(*cursor); }
                                }
                            }
                            (KeyCode::Char(c), KeyModifiers::NONE)
                            | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                                if let Some(GitDialog { state: GitDialogState::Passphrase { ref mut input, ref mut cursor, .. } }) = app.git_dialog {
                                    input.insert(*cursor, c);
                                    *cursor += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    None => {}
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
                // ── File operation dialog ─────────────────────────
                if let Some(mut dlg) = app.file_dialog.take() {
                    if dlg.overwrite.is_some() {
                        if dlg.conflict_rename {
                            // ── C キー: 名前変更して複写/移動 入力 ──────────
                            match (key.code, key.modifiers) {
                                (KeyCode::Esc, _) => {
                                    // 名前入力キャンセル → 競合ダイアログに戻る
                                    dlg.conflict_rename = false;
                                    dlg.input.clear();
                                    dlg.cursor = 0;
                                    app.file_dialog = Some(dlg);
                                }
                                (KeyCode::Enter, KeyModifiers::NONE) => {
                                    let new_name: String = dlg.input.iter().collect();
                                    let prompt = dlg.overwrite.take().unwrap();
                                    match app.resume_rename(&new_name, prompt) {
                                        Ok(None) => {
                                            app.reload();
                                            terminal.clear()?;
                                        }
                                        Ok(Some(p)) => {
                                            dlg.conflict_rename = false;
                                            dlg.input.clear();
                                            dlg.cursor = 0;
                                            dlg.error = None;
                                            dlg.overwrite = Some(p);
                                            app.file_dialog = Some(dlg);
                                        }
                                        Err(e) => {
                                            dlg.error = Some(e.to_string());
                                            app.file_dialog = Some(dlg);
                                        }
                                    }
                                }
                                (KeyCode::Left, _) => {
                                    if dlg.cursor > 0 {
                                        dlg.cursor -= 1;
                                    }
                                    app.file_dialog = Some(dlg);
                                }
                                (KeyCode::Right, _) => {
                                    if dlg.cursor < dlg.input.len() {
                                        dlg.cursor += 1;
                                    }
                                    app.file_dialog = Some(dlg);
                                }
                                (KeyCode::Home, _) => {
                                    dlg.cursor = 0;
                                    app.file_dialog = Some(dlg);
                                }
                                (KeyCode::End, _) => {
                                    dlg.cursor = dlg.input.len();
                                    app.file_dialog = Some(dlg);
                                }
                                (KeyCode::Backspace, _) => {
                                    if dlg.cursor > 0 {
                                        dlg.cursor -= 1;
                                        dlg.input.remove(dlg.cursor);
                                    }
                                    app.file_dialog = Some(dlg);
                                }
                                (KeyCode::Delete, _) => {
                                    if dlg.cursor < dlg.input.len() {
                                        dlg.input.remove(dlg.cursor);
                                    }
                                    app.file_dialog = Some(dlg);
                                }
                                (KeyCode::Char(c), KeyModifiers::NONE)
                                | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                                    dlg.input.insert(dlg.cursor, c);
                                    dlg.cursor += 1;
                                    app.file_dialog = Some(dlg);
                                }
                                _ => {
                                    app.file_dialog = Some(dlg);
                                }
                            }
                        } else {
                            // ── FILMTN スタイル競合ダイアログ ────────────────
                            // lowercase = 一件, uppercase = 一括（以降全件）
                            macro_rules! apply_resume {
                                ($method:ident) => {{
                                    let prompt = dlg.overwrite.take().unwrap();
                                    match app.$method(prompt) {
                                        Ok(None) => {
                                            app.reload();
                                            terminal.clear()?;
                                        }
                                        Ok(Some(p)) => {
                                            dlg.error = None;
                                            dlg.overwrite = Some(p);
                                            app.file_dialog = Some(dlg);
                                        }
                                        Err(e) => {
                                            dlg.error = Some(e.to_string());
                                            app.file_dialog = Some(dlg);
                                        }
                                    }
                                }};
                            }
                            match key.code {
                                KeyCode::Char('u') => apply_resume!(resume_if_newer),
                                KeyCode::Char('U') => apply_resume!(resume_if_newer_batch),
                                KeyCode::Char('o') => apply_resume!(resume_overwrite),
                                KeyCode::Char('O') => apply_resume!(resume_overwrite_batch),
                                KeyCode::Char('n') => apply_resume!(resume_skip),
                                KeyCode::Char('N') => apply_resume!(resume_skip_batch),
                                KeyCode::Char('c') | KeyCode::Char('C') => {
                                    // 名前変更サブダイアログを開く
                                    // 入力欄を競合ファイル名で初期化
                                    let fname: Vec<char> = dlg
                                        .overwrite
                                        .as_ref()
                                        .map(|p| p.conflict.chars().collect())
                                        .unwrap_or_default();
                                    let flen = fname.len();
                                    dlg.input = fname;
                                    dlg.cursor = flen;
                                    dlg.conflict_rename = true;
                                    dlg.error = None;
                                    app.file_dialog = Some(dlg);
                                }
                                KeyCode::Esc => {
                                    // 中断: ダイアログを閉じてリロード
                                    app.reload();
                                    terminal.clear()?;
                                }
                                _ => {
                                    app.file_dialog = Some(dlg);
                                }
                            }
                        }
                    } else {
                        match dlg.kind {
                            DialogKind::DeleteConfirm => match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    let targets = dlg.targets.clone();
                                    match app.exec_delete(&targets) {
                                        Ok(()) => {
                                            app.reload();
                                            if app.cursor >= app.entries.len() {
                                                app.cursor = app.entries.len().saturating_sub(1);
                                            }
                                            terminal.clear()?;
                                        }
                                        Err(e) => {
                                            dlg.error = Some(e.to_string());
                                            app.file_dialog = Some(dlg);
                                        }
                                    }
                                }
                                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {}
                                _ => {
                                    app.file_dialog = Some(dlg);
                                }
                            },
                            _ => {
                                // Input-based dialogs (Rename/Copy/Move/Mkdir/Attr/CopyNewName)
                                match (key.code, key.modifiers) {
                                    (KeyCode::Esc, _) => {}
                                    (KeyCode::Enter, KeyModifiers::NONE) => {
                                        let input: String = dlg.input.iter().collect();
                                        let targets = dlg.targets.clone();
                                        let kind = dlg.kind;

                                        // 同ディレクトリへの複写: 新ファイル名ダイアログへ遷移
                                        if kind == DialogKind::Copy {
                                            let dest = app.resolve_dest(&input);
                                            let dc = dest
                                                .canonicalize()
                                                .unwrap_or_else(|_| dest.clone());
                                            let cc = app
                                                .current_dir
                                                .canonicalize()
                                                .unwrap_or_else(|_| app.current_dir.clone());
                                            if dc == cc {
                                                if targets.len() > 1 {
                                                    dlg.error = Some(if app.lang_en {
                                                        "Multi-file same-dir copy unsupported"
                                                            .to_string()
                                                    } else {
                                                        "同ディレクトリへの複数ファイル複写は非対応です".to_string()
                                                    });
                                                    app.file_dialog = Some(dlg);
                                                } else {
                                                    let orig: Vec<char> =
                                                        targets[0].chars().collect();
                                                    let orig_len = orig.len();
                                                    app.file_dialog = Some(FileDialog {
                                                        kind: DialogKind::CopyNewName,
                                                        input: orig,
                                                        cursor: orig_len,
                                                        targets,
                                                        dest: Some(dest),
                                                        conflict_rename: false,
                                                        error: None,
                                                        overwrite: None,
                                                    });
                                                }
                                                // skip normal op handling
                                                // (remaining key handlers still need to be matched)
                                            } else {
                                                match app.begin_copy(&input, &targets) {
                                                    Ok(None) => {
                                                        app.reload();
                                                        terminal.clear()?;
                                                    }
                                                    Ok(Some(p)) => {
                                                        dlg.error = None;
                                                        dlg.overwrite = Some(p);
                                                        app.file_dialog = Some(dlg);
                                                    }
                                                    Err(e) => {
                                                        dlg.error = Some(e.to_string());
                                                        app.file_dialog = Some(dlg);
                                                    }
                                                }
                                            }
                                        } else {
                                            let op_result = match kind {
                                                DialogKind::Move => {
                                                    app.begin_move(&input, &targets)
                                                }
                                                DialogKind::Rename => {
                                                    app.exec_rename(&input, &targets).map(|_| None)
                                                }
                                                DialogKind::Mkdir => {
                                                    app.exec_mkdir(&input).map(|_| None)
                                                }
                                                DialogKind::Attr => {
                                                    app.exec_attr(&input, &targets).map(|_| None)
                                                }
                                                DialogKind::CopyNewName => {
                                                    let dest_dir = dlg
                                                        .dest
                                                        .clone()
                                                        .unwrap_or_else(|| app.current_dir.clone());
                                                    app.exec_copy_newname(
                                                        &input, &targets, &dest_dir,
                                                    )
                                                    .map(|_| None)
                                                }
                                                _ => Ok(None),
                                            };
                                            match op_result {
                                                Ok(None) => {
                                                    app.reload();
                                                    terminal.clear()?;
                                                }
                                                Ok(Some(p)) => {
                                                    dlg.error = None;
                                                    dlg.overwrite = Some(p);
                                                    app.file_dialog = Some(dlg);
                                                }
                                                Err(e) => {
                                                    dlg.error = Some(e.to_string());
                                                    app.file_dialog = Some(dlg);
                                                }
                                            }
                                        }
                                    }
                                    (KeyCode::Left, _) => {
                                        if dlg.cursor > 0 {
                                            dlg.cursor -= 1;
                                        }
                                        app.file_dialog = Some(dlg);
                                    }
                                    (KeyCode::Right, _) => {
                                        if dlg.cursor < dlg.input.len() {
                                            dlg.cursor += 1;
                                        }
                                        app.file_dialog = Some(dlg);
                                    }
                                    (KeyCode::Home, _) => {
                                        dlg.cursor = 0;
                                        app.file_dialog = Some(dlg);
                                    }
                                    (KeyCode::End, _) => {
                                        dlg.cursor = dlg.input.len();
                                        app.file_dialog = Some(dlg);
                                    }
                                    (KeyCode::Backspace, _) => {
                                        if dlg.cursor > 0 {
                                            dlg.cursor -= 1;
                                            dlg.input.remove(dlg.cursor);
                                        }
                                        app.file_dialog = Some(dlg);
                                    }
                                    (KeyCode::Delete, _) => {
                                        if dlg.cursor < dlg.input.len() {
                                            dlg.input.remove(dlg.cursor);
                                        }
                                        app.file_dialog = Some(dlg);
                                    }
                                    (KeyCode::Char(c), KeyModifiers::NONE)
                                    | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                                        dlg.input.insert(dlg.cursor, c);
                                        dlg.cursor += 1;
                                        app.file_dialog = Some(dlg);
                                    }
                                    _ => {
                                        app.file_dialog = Some(dlg);
                                    }
                                }
                            }
                        }
                    }
                }
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
                        });
                    }
                    Action::SearchNext => {
                        if !app.last_search.is_empty() {
                            let matches = app.compute_search_matches(&app.last_search.clone());
                            if !matches.is_empty() {
                                let next = matches.iter().position(|&i| i > app.cursor)
                                    .unwrap_or(0);
                                app.cursor = matches[next];
                            }
                        }
                    }
                    Action::SearchPrev => {
                        if !app.last_search.is_empty() {
                            let matches = app.compute_search_matches(&app.last_search.clone());
                            if !matches.is_empty() {
                                let prev = matches.iter().rposition(|&i| i < app.cursor)
                                    .unwrap_or(matches.len() - 1);
                                app.cursor = matches[prev];
                            }
                        }
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
                    Action::DirJump => { /* TODO */ }
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
    Ok(())
}
