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

use app::{App, DialogKind, FileDialog, RunDialog};
use config::{Action, load_config};
use fs_utils::{open_in_program, run_command, shell_quote};
use ui::HEADER_ROWS;

fn main() -> Result<()> {
    let config = load_config();

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(stdout()))?;

    let mut app = App::new(config)?;

    while !app.quit {
        let term_h = terminal.size()?.height as usize;
        let lh = term_h.saturating_sub(1 + 2 + HEADER_ROWS as usize).max(1);

        terminal.draw(|f| ui::ui(f, &app))?;

        if !event::poll(Duration::from_millis(500))? { continue; }
        let Event::Key(key) = event::read()? else { continue; };
        if key.kind == KeyEventKind::Press {
            // エラーダイアログ表示中は任意のキーで閉じる
            if app.error_msg.is_some() {
                app.error_msg = None;
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
                    (KeyCode::Left, _) => {
                        #[allow(clippy::collapsible_if)]
                        if let Some(ref mut d) = app.run_dialog {
                            if d.cursor > 0 { d.cursor -= 1; }
                        }
                    }
                    (KeyCode::Right, _) => {
                        #[allow(clippy::collapsible_if)]
                        if let Some(ref mut d) = app.run_dialog {
                            if d.cursor < d.input.len() { d.cursor += 1; }
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
                    (KeyCode::Backspace, _) => {
                        #[allow(clippy::collapsible_if)]
                        if let Some(ref mut d) = app.run_dialog {
                            if d.cursor > 0 {
                                d.cursor -= 1;
                                d.input.remove(d.cursor);
                            }
                        }
                    }
                    (KeyCode::Delete, _) => {
                        #[allow(clippy::collapsible_if)]
                        if let Some(ref mut d) = app.run_dialog {
                            if d.cursor < d.input.len() { d.input.remove(d.cursor); }
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
            } else if let Some(&action) = app.keymap.get(&(key.code, key.modifiers)) {
                // ── Main keymap dispatch ──────────────────────────
                match action {
                    Action::MoveUp => app.move_up(lh),
                    Action::MoveDown => app.move_down(lh),
                    Action::MoveLeft => app.move_left(lh),
                    Action::MoveRight => app.move_right(lh),
                    Action::FirstEntry => {
                        app.cursor = 0;
                        app.update_file_info();
                    }
                    Action::LastEntry => {
                        app.cursor = app.entries.len().saturating_sub(1);
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
