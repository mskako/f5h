use std::sync::mpsc;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crate::app::{App, DialogKind, FileDialog, GitDialog, GitDialogState, RemoteOp};
use crate::fs_utils::{
    git_command_silent, git_fetch, git_merge_no_ff, git_pull, git_push,
    git_stash_push, git_stash_pop,
};

pub(crate) type Term = ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>;

/// Git ダイアログのキーハンドラ。git_task は git バックグラウンドタスクの受信端。
/// 呼び出し元は true が返ったらイベントループで continue すること。
pub(crate) fn handle_git_dialog_key(
    app: &mut App,
    key: KeyEvent,
    git_task: &mut Option<mpsc::Receiver<anyhow::Result<()>>>,
    _lh: usize,
) {
    // ── Git submenu ───────────────────────────────────
    let state = app.git_dialog.as_ref().map(|d| d.state.clone());
    match state {
        Some(GitDialogState::Menu) => {
            // Git メニュー項目数
            const GIT_MENU_LEN: usize = 10;
            match (key.code, key.modifiers) {
                (KeyCode::Esc, _) => app.git_dialog = None,
                (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                    if app.git_menu_cursor > 0 { app.git_menu_cursor -= 1; }
                }
                (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                    if app.git_menu_cursor + 1 < GIT_MENU_LEN { app.git_menu_cursor += 1; }
                }
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    // カーソル位置に対応するキーを合成して再ディスパッチ
                    let synthetic: Option<char> = match app.git_menu_cursor {
                        0 => Some('a'), 1 => Some('A'), 2 => Some('c'),
                        3 => Some('f'), 4 => Some('p'), 5 => Some('P'),
                        6 => Some('m'), 7 => Some('s'), 8 => Some('t'),
                        9 => Some('T'), _ => None,
                    };
                    if let Some(ch) = synthetic {
                        use crossterm::event::{KeyEvent, KeyEventState};
                        let fake = KeyEvent {
                            code: KeyCode::Char(ch),
                            modifiers: if ch.is_uppercase() {
                                KeyModifiers::SHIFT
                            } else {
                                KeyModifiers::NONE
                            },
                            kind: KeyEventKind::Press,
                            state: KeyEventState::NONE,
                        };
                        // 次フレームで処理するため状態を git_dialog のまま保持しつつ
                        // 直接対応するキーハンドラーへジャンプできないため、
                        // git_dialog を None にしてから対応アクションを直接実行する
                        // — ここでは同じキーハンドラーブロックへ再入せず
                        //   代わりに各アクションの先頭コードを直接呼ぶ
                        let _ = fake; // 未使用警告抑制
                        // 各アクションを直接実行（Enterキー押下時の dispatch）
                        match app.git_menu_cursor {
                            0 => {
                                // git add: cursor/tagged
                                let targets: Vec<String> = {
                                    let tagged: Vec<String> = app.entries.iter()
                                        .zip(app.tagged.iter())
                                        .filter(|(_, t)| **t)
                                        .map(|(e, _)| e.name.clone())
                                        .collect();
                                    if !tagged.is_empty() { tagged }
                                    else {
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
                                        Ok(()) => { app.reload(); app.success_msg = Some(format!("{} 件をステージしました。", targets.len())); }
                                        Err(e) => app.error_msg = Some(e.to_string()),
                                    }
                                }
                            }
                            1 => {
                                // git add all
                                app.git_dialog = None;
                                match git_command_silent(&["add", "."], &app.current_dir) {
                                    Ok(()) => { app.reload(); app.success_msg = Some("全変更をステージしました。".to_string()); }
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            }
                            2 => { app.git_dialog = Some(GitDialog { state: GitDialogState::CommitMsg { input: vec![], cursor: 0 } }); }
                            3 => { app.git_dialog = Some(GitDialog { state: GitDialogState::Passphrase { op: RemoteOp::Fetch, input: vec![], cursor: 0 } }); }
                            4 => { app.git_dialog = Some(GitDialog { state: GitDialogState::Passphrase { op: RemoteOp::Push, input: vec![], cursor: 0 } }); }
                            5 => { app.git_dialog = Some(GitDialog { state: GitDialogState::Passphrase { op: RemoteOp::Pull, input: vec![], cursor: 0 } }); }
                            6 => {
                                app.git_dialog = None;
                                match git_merge_no_ff(&app.current_dir) {
                                    Ok(()) => { app.reload(); app.success_msg = Some("マージが完了しました。".to_string()); }
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            }
                            7 => { app.git_dialog = Some(GitDialog { state: GitDialogState::SwitchBranch { input: vec![], cursor: 0 } }); }
                            8 => { app.git_dialog = Some(GitDialog { state: GitDialogState::StashMsg { input: vec![], cursor: 0 } }); }
                            9 => {
                                app.git_dialog = None;
                                match git_stash_pop(&app.current_dir) {
                                    Ok(()) => { app.reload(); app.success_msg = Some("スタッシュを取り出しました。".to_string()); }
                                    Err(e) => app.error_msg = Some(e.to_string()),
                                }
                            }
                            _ => {}
                        }
                    }
                }
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
                            Ok(()) => {
                                app.reload();
                                app.success_msg = Some(if app.lang_en {
                                    format!("{} file(s) added.", targets.len())
                                } else {
                                    format!("{} 件をステージしました。", targets.len())
                                });
                            }
                            Err(e) => app.error_msg = Some(e.to_string()),
                        }
                    }
                }
                (KeyCode::Char('A'), KeyModifiers::NONE)
                | (KeyCode::Char('A'), KeyModifiers::SHIFT) => {
                    app.git_dialog = None;
                    match git_command_silent(&["add", "."], &app.current_dir) {
                        Ok(()) => {
                            app.reload();
                            app.success_msg = Some(if app.lang_en {
                                "All changes staged.".to_string()
                            } else {
                                "全変更をステージしました。".to_string()
                            });
                        }
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
                        Ok(()) => {
                            app.reload();
                            app.success_msg = Some(if app.lang_en {
                                "Merge completed.".to_string()
                            } else {
                                "マージが完了しました。".to_string()
                            });
                        }
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
                        Ok(()) => {
                            app.reload();
                            app.success_msg = Some(if app.lang_en {
                                "Stash popped.".to_string()
                            } else {
                                "スタッシュを取り出しました。".to_string()
                            });
                        }
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
                            Ok(()) => {
                                app.reload();
                                app.success_msg = Some(if app.lang_en {
                                    "Committed.".to_string()
                                } else {
                                    "コミットしました。".to_string()
                                });
                            }
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
                            Ok(()) => {
                                app.reload();
                                app.success_msg = Some(if app.lang_en {
                                    format!("Switched to branch '{}'.", branch.trim())
                                } else {
                                    format!("ブランチ '{}' に切り替えました。", branch.trim())
                                });
                            }
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
                        Ok(()) => {
                            app.reload();
                            app.success_msg = Some(if app.lang_en {
                                "Stash saved.".to_string()
                            } else {
                                "スタッシュに保存しました。".to_string()
                            });
                        }
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
                    *git_task = Some(rx);
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
}

/// ファイルダイアログのキーハンドラ。terminal は screen clear に使用。
pub(crate) fn handle_file_dialog_key(
    app: &mut App,
    key: KeyEvent,
    terminal: &mut Term,
    _lh: usize,
) -> anyhow::Result<()> {
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
                match (key.code, key.modifiers) {
                    (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                        if let Some(ref mut p) = dlg.overwrite {
                            if p.cursor > 0 { p.cursor -= 1; }
                        }
                        app.file_dialog = Some(dlg);
                    }
                    (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                        if let Some(ref mut p) = dlg.overwrite {
                            if p.cursor < 3 { p.cursor += 1; }
                        }
                        app.file_dialog = Some(dlg);
                    }
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        let cur = dlg.overwrite.as_ref().map(|p| p.cursor).unwrap_or(0);
                        match cur {
                            0 => apply_resume!(resume_if_newer),
                            1 => apply_resume!(resume_overwrite),
                            2 => {
                                // C と同じ: 名前変更サブダイアログを開く
                                let fname: Vec<char> = dlg.overwrite.as_ref()
                                    .map(|p| p.conflict.chars().collect())
                                    .unwrap_or_default();
                                let flen = fname.len();
                                dlg.input = fname;
                                dlg.cursor = flen;
                                dlg.conflict_rename = true;
                                dlg.error = None;
                                app.file_dialog = Some(dlg);
                            }
                            _ => apply_resume!(resume_skip),
                        }
                    }
                    (KeyCode::Char('u'), _) => apply_resume!(resume_if_newer),
                    (KeyCode::Char('U'), _) => apply_resume!(resume_if_newer_batch),
                    (KeyCode::Char('o'), _) => apply_resume!(resume_overwrite),
                    (KeyCode::Char('O'), _) => apply_resume!(resume_overwrite_batch),
                    (KeyCode::Char('n'), _) => apply_resume!(resume_skip),
                    (KeyCode::Char('N'), _) => apply_resume!(resume_skip_batch),
                    (KeyCode::Char('c'), _) | (KeyCode::Char('C'), _) => {
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
                    (KeyCode::Esc, _) => {
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
                DialogKind::DeleteConfirm => {
                    // cursor: 0=No(デフォルト) 1=Yes
                    macro_rules! do_delete {
                        () => {{
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
                        }};
                    }
                    match (key.code, key.modifiers) {
                        // h/← → 左ボタン(Y:はい) = cursor 1
                        (KeyCode::Left, _) | (KeyCode::Char('h'), _)
                        | (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                            dlg.cursor = 1;
                            app.file_dialog = Some(dlg);
                        }
                        // l/→ → 右ボタン(N:いいえ) = cursor 0
                        (KeyCode::Right, _) | (KeyCode::Char('l'), _)
                        | (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                            dlg.cursor = 0;
                            app.file_dialog = Some(dlg);
                        }
                        (KeyCode::Enter, KeyModifiers::NONE) => {
                            if dlg.cursor == 1 { do_delete!(); }
                            // cursor==0 は何もしない（ダイアログを閉じる）
                        }
                        (KeyCode::Char('y'), _) | (KeyCode::Char('Y'), _) => {
                            do_delete!();
                        }
                        (KeyCode::Esc, _) | (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) => {}
                        _ => { app.file_dialog = Some(dlg); }
                    }
                }
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
    Ok(())
}
