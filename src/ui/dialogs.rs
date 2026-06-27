use super::*;
use crate::app::{App, DialogKind, FuncDialog, GitDialogState, RemoteOp, SortMode, FUNC_CMDS};

// ── Dir jump dialog ─────────────────────────────────────────────────────

pub(super) fn render_dir_jump_dialog(frame: &mut Frame, app: &App) {
    let dlg = match app.dir_jump_dialog.as_ref() {
        Some(d) => d,
        None => return,
    };
    let area = frame.area();
    let dw = (area.width as usize).clamp(50, 80);
    let iw = dw - 2;
    let dh = 4usize; // 上枠 + 入力行 + エラー行 + 下枠
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = ((area.height as usize).saturating_sub(dh) / 2) as u16;

    let buf = frame.buffer_mut();
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let white = Style::default().fg(Color::White);
    let red = Style::default().fg(Color::Red);
    let dim = Style::default().fg(Color::DarkGray);
    let rev = Style::default().add_modifier(Modifier::REVERSED);

    let title = if app.lang_en { " Jump to directory " } else { " ディレクトリへジャンプ " };
    let hint = if app.lang_en { "  Enter:Jump  Esc:Cancel" } else { "  Enter:移動  Esc:キャンセル" };
    let label = if app.lang_en { "Dir: " } else { "移動先: " };
    let pw = sw(label);

    clear_rect(buf, dx, dy, dw, dh);

    // 上枠
    let title_w = sw(title);
    let fill_n = iw.saturating_sub(title_w);
    blit_ch(buf, dx, dy, '╭', cyan);
    blit(buf, dx + 1, dy, title, title_w, yellow);
    blit(buf, dx + 1 + title_w as u16, dy, &"─".repeat(fill_n), fill_n, cyan);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

    // 入力行
    blit_ch(buf, dx, dy + 1, '│', cyan);
    let input_avail = iw.saturating_sub(pw).max(1);
    blit(buf, dx + 1, dy + 1, label, pw.min(iw), yellow);
    let input_x = dx + 1 + pw.min(iw) as u16;
    let scroll = if dlg.cursor >= input_avail { dlg.cursor + 1 - input_avail } else { 0 };
    blit(buf, input_x, dy + 1, &" ".repeat(input_avail), input_avail, white);
    for (i, &ch) in dlg.input[scroll..].iter().enumerate() {
        if i >= input_avail { break; }
        let st = if scroll + i == dlg.cursor { rev } else { white };
        blit_ch(buf, input_x + i as u16, dy + 1, ch, st);
    }
    if dlg.cursor == dlg.input.len() && dlg.cursor - scroll < input_avail {
        blit_ch(buf, input_x + (dlg.cursor - scroll) as u16, dy + 1, ' ', rev);
    }
    blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

    // エラー / ヒント行
    blit_ch(buf, dx, dy + 2, '│', cyan);
    if let Some(ref err) = dlg.error {
        blit(buf, dx + 1, dy + 2, &padr(&trunc(err, iw), iw), iw, red);
    } else {
        blit(buf, dx + 1, dy + 2, &padr(hint, iw), iw, dim);
    }
    blit_ch(buf, dx + dw as u16 - 1, dy + 2, '│', cyan);

    // 下枠
    blit_ch(buf, dx, dy + 3, '╰', cyan);
    blit(buf, dx + 1, dy + 3, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, dx + dw as u16 - 1, dy + 3, '╯', cyan);
}

// ── Run dialog ─────────────────────────────────────────────────────────

pub(super) fn render_run_dialog(frame: &mut Frame, app: &App) {
    let dlg = match app.run_dialog.as_ref() {
        Some(d) => d,
        None => return,
    };
    let area = frame.area();
    let dw = (area.width as usize).clamp(40, 72);
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = area.height / 2 - 2;

    let buf = frame.buffer_mut();
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let dim = Style::default().fg(Color::DarkGray);
    let white = Style::default().fg(Color::White);
    let rev = Style::default().add_modifier(Modifier::REVERSED);

    let title = if app.lang_en { " Run " } else { " 実行 " };
    let hint = if app.lang_en {
        "  Enter:Run  Esc:Cancel"
    } else {
        "  Enter:実行  Esc:キャンセル"
    };
    let prompt = "> ";
    let pw = sw(prompt);
    let iw = dw - 2;

    // Top border
    let fill_n = iw.saturating_sub(sw(title));
    blit_ch(buf, dx, dy, '╭', cyan);
    blit(buf, dx + 1, dy, title, sw(title), yellow);
    blit(
        buf,
        dx + 1 + sw(title) as u16,
        dy,
        &"─".repeat(fill_n),
        fill_n,
        cyan,
    );
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

    // Input row
    let input_x = dx + 1 + pw as u16;
    let input_w = iw - pw;
    blit_ch(buf, dx, dy + 1, '│', cyan);
    blit(buf, dx + 1, dy + 1, prompt, pw, cyan);
    let scroll = if dlg.cursor >= input_w {
        dlg.cursor + 1 - input_w
    } else {
        0
    };
    blit(buf, input_x, dy + 1, &" ".repeat(input_w), input_w, white);
    for (i, &ch) in dlg.input[scroll..].iter().enumerate() {
        if i >= input_w {
            break;
        }
        let st = if scroll + i == dlg.cursor { rev } else { white };
        blit_ch(buf, input_x + i as u16, dy + 1, ch, st);
    }
    if dlg.cursor == dlg.input.len() && dlg.cursor - scroll < input_w {
        blit_ch(
            buf,
            input_x + (dlg.cursor - scroll) as u16,
            dy + 1,
            ' ',
            rev,
        );
    }
    blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

    // Hint row
    blit_ch(buf, dx, dy + 2, '│', cyan);
    blit(buf, dx + 1, dy + 2, &padr(hint, iw), iw, dim);
    blit_ch(buf, dx + dw as u16 - 1, dy + 2, '│', cyan);

    // Bottom border
    blit_ch(buf, dx, dy + 3, '╰', cyan);
    blit(buf, dx + 1, dy + 3, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, dx + dw as u16 - 1, dy + 3, '╯', cyan);
}

// ── 機能ダイアログ ─────────────────────────────────────────────────────

pub(super) fn render_func_dialog(frame: &mut Frame, app: &App) {
    let dlg: &FuncDialog = match app.func_dialog.as_ref() {
        Some(d) => d,
        None => return,
    };
    let area = frame.area();
    let dw = (area.width as usize).clamp(44, 60);
    let iw = dw - 2;

    // 入力中のコマンド名を取得してフィルタリング
    let query: String = dlg.input.iter().collect();
    let cmd_name: &str = query.trim().splitn(2, ' ').next().unwrap_or("");
    let filtered: Vec<&crate::app::FuncCmd> = FUNC_CMDS.iter()
        .filter(|c| c.name.starts_with(cmd_name))
        .collect();
    let n_cmds = filtered.len().max(1); // 最低1行確保

    // ダイアログ高さ: 枠上 + 入力行 + セパレータ + コマンド行 + ヒント行 + 枠下
    let dh = 2 + 1 + 1 + n_cmds + 1;
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = ((area.height as usize).saturating_sub(dh) / 2) as u16;

    let buf = frame.buffer_mut();
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let dim = Style::default().fg(Color::DarkGray);
    let white = Style::default().fg(Color::White);
    let green = Style::default().fg(Color::Green);
    let rev = Style::default().add_modifier(Modifier::REVERSED);

    let title = if app.lang_en { " Func " } else { " 機能 " };
    let hint = if app.lang_en {
        "  Tab:Complete  Enter:Run  Esc:Close"
    } else {
        "  Tab:補完  Enter:実行  Esc:閉じる"
    };

    clear_rect(buf, dx, dy, dw, dh);

    // 上枠
    let fill_n = iw.saturating_sub(sw(title));
    blit_ch(buf, dx, dy, '╭', cyan);
    blit(buf, dx + 1, dy, title, sw(title), yellow);
    blit(buf, dx + 1 + sw(title) as u16, dy, &"─".repeat(fill_n), fill_n, cyan);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

    // 入力行
    let prompt = "> ";
    let pw = sw(prompt);
    let input_x = dx + 1 + pw as u16;
    let input_w = iw - pw;
    let row_input = dy + 1;
    blit_ch(buf, dx, row_input, '│', cyan);
    blit(buf, dx + 1, row_input, prompt, pw, cyan);
    let scroll = if dlg.cursor >= input_w { dlg.cursor + 1 - input_w } else { 0 };
    blit(buf, input_x, row_input, &" ".repeat(input_w), input_w, white);
    for (i, &ch) in dlg.input[scroll..].iter().enumerate() {
        if i >= input_w { break; }
        let st = if scroll + i == dlg.cursor { rev } else { white };
        blit_ch(buf, input_x + i as u16, row_input, ch, st);
    }
    if dlg.cursor == dlg.input.len() && dlg.cursor.saturating_sub(scroll) < input_w {
        blit_ch(buf, input_x + (dlg.cursor - scroll) as u16, row_input, ' ', rev);
    }
    blit_ch(buf, dx + dw as u16 - 1, row_input, '│', cyan);

    // セパレータ
    let row_sep = dy + 2;
    blit_ch(buf, dx, row_sep, '├', cyan);
    blit(buf, dx + 1, row_sep, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, dx + dw as u16 - 1, row_sep, '┤', cyan);

    // コマンドリスト
    let name_w = 10usize;
    let args_w = 10usize;
    let desc_w = iw.saturating_sub(name_w + args_w + 2);
    for (i, cmd) in filtered.iter().enumerate() {
        let y = dy + 3 + i as u16;
        let selected = i == dlg.selected;
        let row_style = if selected { rev } else { white };
        let desc = if app.lang_en { cmd.desc_en } else { cmd.desc_ja };
        blit_ch(buf, dx, y, '│', cyan);
        if selected {
            blit(buf, dx + 1, y, &" ".repeat(iw), iw, rev);
        }
        blit(buf, dx + 1, y, &padr(cmd.name, name_w), name_w, if selected { rev } else { green });
        blit(buf, dx + 1 + name_w as u16, y, &padr(cmd.args, args_w), args_w,
             if selected { rev } else { dim });
        blit(buf, dx + 1 + (name_w + args_w) as u16, y,
             &trunc(desc, desc_w), desc_w, row_style);
        blit_ch(buf, dx + dw as u16 - 1, y, '│', cyan);
    }
    if filtered.is_empty() {
        let y = dy + 3;
        let msg = if app.lang_en { "  (no matching command)" } else { "  (該当コマンドなし)" };
        blit_ch(buf, dx, y, '│', cyan);
        blit(buf, dx + 1, y, &padr(msg, iw), iw, dim);
        blit_ch(buf, dx + dw as u16 - 1, y, '│', cyan);
    }

    // ヒント行
    let row_hint = dy + 3 + n_cmds as u16;
    blit_ch(buf, dx, row_hint, '│', cyan);
    blit(buf, dx + 1, row_hint, &padr(&trunc(hint, iw), iw), iw, dim);
    blit_ch(buf, dx + dw as u16 - 1, row_hint, '│', cyan);

    // 下枠
    let row_bot = dy + dh as u16 - 1;
    blit_ch(buf, dx, row_bot, '╰', cyan);
    blit(buf, dx + 1, row_bot, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, dx + dw as u16 - 1, row_bot, '╯', cyan);
}

// ── Git submenu dialog ─────────────────────────────────────────────────

pub(super) fn render_git_dialog(frame: &mut Frame, app: &App) {
    let dlg = match app.git_dialog.as_ref() {
        Some(d) => d,
        None => return,
    };
    let area = frame.area();
    let buf = frame.buffer_mut();
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let dim = Style::default().fg(Color::DarkGray);
    let white = Style::default().fg(Color::White);
    let rev = Style::default().add_modifier(Modifier::REVERSED);
    let green = Style::default().fg(Color::Green);

    match &dlg.state {
        GitDialogState::Menu => {
            let dw: usize = 50;
            let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
            let dy = area.height / 2 - 5;
            let iw = dw - 2;

            let title = if app.lang_en { " Git " } else { " Git " };
            let rows: &[(&str, &str, &str)] = if app.lang_en {
                &[
                    ("a", "add", "cursor / tagged files"),
                    ("A", "add all", "git add ."),
                    ("c", "commit", "commit with message"),
                    ("f", "fetch", "git fetch origin"),
                    ("p", "push", "git push"),
                    ("P", "pull", "fetch + rebase"),
                    ("m", "merge", "merge --no-ff @{u}"),
                    ("s", "switch", "switch branch"),
                    ("t", "stash", "save working changes (optional msg)"),
                    ("T", "stash pop", "restore latest stash"),
                ]
            } else {
                &[
                    ("a", "add", "カーソル/タグ付きファイル"),
                    ("A", "add all", "git add ."),
                    ("c", "commit", "メッセージ入力して commit"),
                    ("f", "fetch", "git fetch origin"),
                    ("p", "push", "git push"),
                    ("P", "pull", "fetch + rebase"),
                    ("m", "merge", "--no-ff で upstream をマージ"),
                    ("s", "switch", "ブランチ切替"),
                    ("t", "stash", "作業変更を退避（メッセージ任意）"),
                    ("T", "stash pop", "最新 stash を復元"),
                ]
            };
            let hint = if app.lang_en { "  Esc:Cancel" } else { "  Esc:キャンセル" };
            let total_rows = rows.len() + 2; // top + rows + hint + bottom

            clear_rect(buf, dx, dy, dw, total_rows + 1);
            blit_ch(buf, dx, dy, '╭', cyan);
            blit(buf, dx + 1, dy, title, sw(title), yellow);
            blit(buf, dx + 1 + sw(title) as u16, dy, &"─".repeat(iw.saturating_sub(sw(title))), iw.saturating_sub(sw(title)), cyan);
            blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

            let sel = app.git_menu_cursor;
            let cursor_st = Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED);
            for (i, (key, cmd, desc)) in rows.iter().enumerate() {
                let y = dy + 1 + i as u16;
                blit_ch(buf, dx, y, '│', cyan);
                let is_sel = i == sel;
                if is_sel {
                    // 行全体を REVERSED でハイライト
                    let line = format!(" {}  {}  {}", key, padr(cmd, 9), trunc(desc, iw - 14));
                    blit(buf, dx + 1, y, &padr(&line, iw), iw, cursor_st);
                } else {
                    blit(buf, dx + 1, y, key, 1, green);
                    blit(buf, dx + 2, y, "  ", 2, white);
                    let cmd_w = 9;
                    blit(buf, dx + 4, y, &padr(cmd, cmd_w), cmd_w, white);
                    blit(buf, dx + 4 + cmd_w as u16, y, &trunc(desc, iw - 4 - cmd_w), iw - 4 - cmd_w, dim);
                }
                blit_ch(buf, dx + dw as u16 - 1, y, '│', cyan);
            }

            let hy = dy + 1 + rows.len() as u16;
            blit_ch(buf, dx, hy, '│', cyan);
            blit(buf, dx + 1, hy, &padr(hint, iw), iw, dim);
            blit_ch(buf, dx + dw as u16 - 1, hy, '│', cyan);

            let by = dy + total_rows as u16;
            blit_ch(buf, dx, by, '╰', cyan);
            blit(buf, dx + 1, by, &"─".repeat(iw), iw, cyan);
            blit_ch(buf, dx + dw as u16 - 1, by, '╯', cyan);
        }

        GitDialogState::CommitMsg { input, cursor }
        | GitDialogState::SwitchBranch { input, cursor }
        | GitDialogState::StashMsg { input, cursor } => {
            let is_commit = matches!(dlg.state, GitDialogState::CommitMsg { .. });
            let is_stash = matches!(dlg.state, GitDialogState::StashMsg { .. });
            let dw = (area.width as usize).clamp(50, 70);
            let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
            let dy = area.height / 2 - 2;
            let iw = dw - 2;

            let title = if is_commit {
                if app.lang_en { " Git commit " } else { " Git commit " }
            } else if is_stash {
                if app.lang_en { " Git stash " } else { " Git stash " }
            } else {
                if app.lang_en { " Git switch " } else { " Git switch " }
            };
            let hint = if is_stash {
                if app.lang_en { "  Enter:Run(empty=no msg)  Esc:Back" } else { "  Enter:実行(空=メッセージなし)  Esc:戻る" }
            } else if app.lang_en {
                "  Enter:Run  Esc:Back"
            } else {
                "  Enter:実行  Esc:戻る"
            };
            let prompt = if is_commit { "msg: " } else if is_stash { "msg: " } else { "branch: " };
            let pw = sw(prompt);
            let input_w = iw - pw;
            let input_x = dx + 1 + pw as u16;

            clear_rect(buf, dx, dy, dw, 4);
            blit_ch(buf, dx, dy, '╭', cyan);
            blit(buf, dx + 1, dy, title, sw(title), yellow);
            blit(buf, dx + 1 + sw(title) as u16, dy, &"─".repeat(iw.saturating_sub(sw(title))), iw.saturating_sub(sw(title)), cyan);
            blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

            blit_ch(buf, dx, dy + 1, '│', cyan);
            blit(buf, dx + 1, dy + 1, prompt, pw, cyan);
            let scroll = if *cursor >= input_w { cursor + 1 - input_w } else { 0 };
            blit(buf, input_x, dy + 1, &" ".repeat(input_w), input_w, white);
            for (i, &ch) in input[scroll..].iter().enumerate() {
                if i >= input_w { break; }
                let st = if scroll + i == *cursor { rev } else { white };
                blit_ch(buf, input_x + i as u16, dy + 1, ch, st);
            }
            if *cursor == input.len() && cursor - scroll < input_w {
                blit_ch(buf, input_x + (cursor - scroll) as u16, dy + 1, ' ', rev);
            }
            blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

            blit_ch(buf, dx, dy + 2, '│', cyan);
            blit(buf, dx + 1, dy + 2, &padr(hint, iw), iw, dim);
            blit_ch(buf, dx + dw as u16 - 1, dy + 2, '│', cyan);

            blit_ch(buf, dx, dy + 3, '╰', cyan);
            blit(buf, dx + 1, dy + 3, &"─".repeat(iw), iw, cyan);
            blit_ch(buf, dx + dw as u16 - 1, dy + 3, '╯', cyan);
        }

        GitDialogState::Passphrase { op, input, cursor } => {
            let dw = (area.width as usize).clamp(36, 56);
            let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
            let dy = area.height / 2 - 2;
            let iw = dw - 2;

            let op_label = match op {
                RemoteOp::Fetch => "fetch",
                RemoteOp::Push => "push",
                RemoteOp::Pull => "pull",
            };
            let title = format!(" Git {} ", op_label);
            let hint = if app.lang_en {
                "  Enter:Run  Esc:Cancel"
            } else {
                "  Enter:実行  Esc:キャンセル"
            };
            let prompt = "passphrase: ";
            let pw = sw(prompt);
            let input_w = iw.saturating_sub(pw);
            let input_x = dx + 1 + pw as u16;

            blit_ch(buf, dx, dy, '╭', cyan);
            blit(buf, dx + 1, dy, &title, sw(&title), yellow);
            blit(buf, dx + 1 + sw(&title) as u16, dy, &"─".repeat(iw.saturating_sub(sw(&title))), iw.saturating_sub(sw(&title)), cyan);
            blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

            // サブタイトル
            let sub = if app.lang_en {
                "  (empty = try SSH agent)"
            } else {
                "  空=SSHエージェント試行"
            };
            blit_ch(buf, dx, dy + 1, '│', cyan);
            blit(buf, dx + 1, dy + 1, &padr(sub, iw), iw, dim);
            blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

            // パスフレーズ入力行（* でマスク）
            blit_ch(buf, dx, dy + 2, '│', cyan);
            blit(buf, dx + 1, dy + 2, prompt, pw, cyan);
            let scroll = if *cursor >= input_w { cursor + 1 - input_w } else { 0 };
            blit(buf, input_x, dy + 2, &" ".repeat(input_w), input_w, white);
            for i in 0..input.len().saturating_sub(scroll).min(input_w) {
                let st = if scroll + i == *cursor { rev } else { white };
                blit_ch(buf, input_x + i as u16, dy + 2, '*', st);
            }
            if *cursor == input.len() && cursor - scroll < input_w {
                blit_ch(buf, input_x + (cursor - scroll) as u16, dy + 2, ' ', rev);
            }
            blit_ch(buf, dx + dw as u16 - 1, dy + 2, '│', cyan);

            blit_ch(buf, dx, dy + 3, '│', cyan);
            blit(buf, dx + 1, dy + 3, &padr(hint, iw), iw, dim);
            blit_ch(buf, dx + dw as u16 - 1, dy + 3, '│', cyan);

            blit_ch(buf, dx, dy + 4, '╰', cyan);
            blit(buf, dx + 1, dy + 4, &"─".repeat(iw), iw, cyan);
            blit_ch(buf, dx + dw as u16 - 1, dy + 4, '╯', cyan);
        }
    }
}

// ── Git running overlay ────────────────────────────────────────────────

pub(super) fn render_git_running(frame: &mut Frame, app: &App) {
    if !app.git_running {
        return;
    }
    let area = frame.area();
    let buf = frame.buffer_mut();
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let dim = Style::default().fg(Color::DarkGray);

    let msg = if app.lang_en { " Git: running... " } else { " Git: 実行中... " };
    let dw = sw(msg) + 2;
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = area.height / 2;

    blit_ch(buf, dx, dy, '╭', cyan);
    blit(buf, dx + 1, dy, msg, sw(msg), yellow);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

    let hint = if app.lang_en { "  please wait..." } else { "  しばらくお待ちください..." };
    let iw = dw - 2;
    blit_ch(buf, dx, dy + 1, '│', cyan);
    blit(buf, dx + 1, dy + 1, &padr(hint, iw), iw, dim);
    blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

    blit_ch(buf, dx, dy + 2, '╰', cyan);
    blit(buf, dx + 1, dy + 2, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, dx + dw as u16 - 1, dy + 2, '╯', cyan);
}

// ── File operation dialog ──────────────────────────────────────────────

pub(super) fn render_file_dialog(frame: &mut Frame, app: &App) {
    let dlg = match app.file_dialog.as_ref() {
        Some(d) => d,
        None => return,
    };
    let area = frame.area();
    let dw = (area.width as usize).clamp(50, 72);
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;

    let buf = frame.buffer_mut();
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let dim = Style::default().fg(Color::DarkGray);
    let white = Style::default().fg(Color::White);
    let red = Style::default().fg(Color::Red);
    let rev = Style::default().add_modifier(Modifier::REVERSED);
    let iw = dw - 2;

    // ── FILMTN-style conflict dialog (8 rows) ──────────────────────────
    if let Some(ref prompt) = dlg.overwrite {
        if dlg.conflict_rename {
            // C キー: 新名前入力サブダイアログ（通常の5行ダイアログ）
            let dy = ((area.height as usize).saturating_sub(5) / 2) as u16;
            let title = if app.lang_en {
                " Rename & Copy/Move "
            } else {
                " 名前変更して複写/移動 "
            };
            let label = if app.lang_en {
                "New name: "
            } else {
                "新ファイル名: "
            };
            let hint = if app.lang_en {
                "  Enter:OK  Esc:Cancel"
            } else {
                "  Enter:実行  Esc:キャンセル"
            };

            let title_w = sw(title);
            let fill_n = iw.saturating_sub(title_w);
            blit_ch(buf, dx, dy, '╭', cyan);
            blit(buf, dx + 1, dy, title, title_w, yellow);
            blit(
                buf,
                dx + 1 + title_w as u16,
                dy,
                &"─".repeat(fill_n),
                fill_n,
                cyan,
            );
            blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

            blit_ch(buf, dx, dy + 1, '│', cyan);
            let pw = sw(label);
            let input_avail = iw.saturating_sub(pw).max(1);
            blit(buf, dx + 1, dy + 1, label, pw.min(iw), yellow);
            let input_x = dx + 1 + pw.min(iw) as u16;
            let scroll = if dlg.cursor >= input_avail {
                dlg.cursor + 1 - input_avail
            } else {
                0
            };
            blit(
                buf,
                input_x,
                dy + 1,
                &" ".repeat(input_avail),
                input_avail,
                white,
            );
            for (i, &ch) in dlg.input[scroll..].iter().enumerate() {
                if i >= input_avail {
                    break;
                }
                let st = if scroll + i == dlg.cursor { rev } else { white };
                blit_ch(buf, input_x + i as u16, dy + 1, ch, st);
            }
            if dlg.cursor == dlg.input.len() && dlg.cursor - scroll < input_avail {
                blit_ch(
                    buf,
                    input_x + (dlg.cursor - scroll) as u16,
                    dy + 1,
                    ' ',
                    rev,
                );
            }
            blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

            blit_ch(buf, dx, dy + 2, '│', cyan);
            let err_s = dlg.error.as_deref().unwrap_or("");
            let err_st = if err_s.is_empty() { white } else { red };
            blit(
                buf,
                dx + 1,
                dy + 2,
                &padr(&trunc(err_s, iw), iw),
                iw,
                err_st,
            );
            blit_ch(buf, dx + dw as u16 - 1, dy + 2, '│', cyan);

            blit_ch(buf, dx, dy + 3, '│', cyan);
            blit(buf, dx + 1, dy + 3, &padr(hint, iw), iw, dim);
            blit_ch(buf, dx + dw as u16 - 1, dy + 3, '│', cyan);

            blit_ch(buf, dx, dy + 4, '╰', cyan);
            blit(buf, dx + 1, dy + 4, &"─".repeat(iw), iw, cyan);
            blit_ch(buf, dx + dw as u16 - 1, dy + 4, '╯', cyan);
        } else {
            // FILMTN conflict dialog: 8 rows
            let dy = ((area.height as usize).saturating_sub(8) / 2) as u16;
            let title = if dlg.kind == DialogKind::Move {
                if app.lang_en { " Move " } else { " 移動 " }
            } else if app.lang_en {
                " Copy "
            } else {
                " 複写 "
            };
            let exist_msg = if app.lang_en {
                format!(" \"{}\" already exists", prompt.conflict)
            } else {
                format!(" \"{}\" は既に存在します", prompt.conflict)
            };
            let (lu, lo, lc, ln, lesc, lbatch) = if app.lang_en {
                (
                    "U:Copy/Move if newer",
                    "O:Overwrite",
                    "C:Rename then copy/move",
                    "N:Skip",
                    "ESC:Abort",
                    "  SHIFT+:Apply to all",
                )
            } else {
                (
                    "U:タイムスタンプが新しい時に複写/移動する",
                    "O:上書きで複写/移動する",
                    "C:名前を変更して複写/移動する",
                    "N:複写/移動しない",
                    "ESC:中断",
                    "  SHIFT+:一括設定",
                )
            };
            let (lu, lo, lc, ln) = if dlg.kind == DialogKind::Move {
                if app.lang_en {
                    (
                        "U:Move if newer",
                        "O:Overwrite & move",
                        "C:Rename then move",
                        "N:Skip move",
                    )
                } else {
                    (
                        "U:タイムスタンプが新しい時に移動する",
                        "O:上書きで移動する",
                        "C:名前を変更して移動する",
                        "N:移動しない",
                    )
                }
            } else {
                (lu, lo, lc, ln)
            };

            let title_w = sw(title);
            let fill_n = iw.saturating_sub(title_w);
            blit_ch(buf, dx, dy, '╭', cyan);
            blit(buf, dx + 1, dy, title, title_w, yellow);
            blit(
                buf,
                dx + 1 + title_w as u16,
                dy,
                &"─".repeat(fill_n),
                fill_n,
                cyan,
            );
            blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

            // row 1: conflict filename
            blit_ch(buf, dx, dy + 1, '│', cyan);
            blit(
                buf,
                dx + 1,
                dy + 1,
                &padr(&trunc(&exist_msg, iw), iw),
                iw,
                yellow,
            );
            blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

            // row 2: blank / error
            blit_ch(buf, dx, dy + 2, '│', cyan);
            let err_s = dlg.error.as_deref().unwrap_or("");
            let err_st = if err_s.is_empty() { white } else { red };
            blit(
                buf,
                dx + 1,
                dy + 2,
                &padr(&trunc(err_s, iw), iw),
                iw,
                err_st,
            );
            blit_ch(buf, dx + dw as u16 - 1, dy + 2, '│', cyan);

            // rows 3–6: options (cursor でハイライト)
            let sel = prompt.cursor; // 0=U 1=O 2=C 3=N
            for (idx, (row, label)) in [(3u16, lu), (4, lo), (5, lc), (6, ln)].iter().enumerate() {
                blit_ch(buf, dx, dy + row, '│', cyan);
                let st = if idx == sel { Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED) } else { white };
                blit(
                    buf,
                    dx + 2,
                    dy + row,
                    &padr(&trunc(label, iw - 1), iw - 1),
                    iw - 1,
                    st,
                );
                blit_ch(buf, dx + dw as u16 - 1, dy + row, '│', cyan);
            }

            // row 6: N + ESC on same row (N はすでに上のループで描画済み; ESC を追記)
            blit_ch(buf, dx, dy + 6, '│', cyan);
            let ln_w = sw(ln);
            let esc_w = sw(lesc);
            let gap = iw.saturating_sub(ln_w + esc_w + 2);
            let n_st = if sel == 3 { Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED) } else { white };
            blit(buf, dx + 2, dy + 6, ln, ln_w, n_st);
            blit(buf, dx + 2 + ln_w as u16, dy + 6, &" ".repeat(gap + 2), gap + 2, white);
            blit(buf, dx + 2 + ln_w as u16 + (gap + 2) as u16, dy + 6, lesc, esc_w, dim);
            blit_ch(buf, dx + dw as u16 - 1, dy + 6, '│', cyan);

            // bottom border with SHIFT+ hint
            let batch_w = sw(lbatch);
            let bot_fill = iw.saturating_sub(batch_w);
            blit_ch(buf, dx, dy + 7, '╰', cyan);
            blit(buf, dx + 1, dy + 7, &"─".repeat(bot_fill), bot_fill, cyan);
            blit(buf, dx + 1 + bot_fill as u16, dy + 7, lbatch, batch_w, dim);
            blit_ch(buf, dx + dw as u16 - 1, dy + 7, '╯', cyan);
        }
        return;
    }

    // ── Normal 5-row dialog ────────────────────────────────────────────
    let dy = ((area.height as usize).saturating_sub(5) / 2) as u16;

    let (title, content, hint, show_input): (String, String, &str, bool) = match dlg.kind {
        DialogKind::DeleteConfirm => {
            let n = dlg.targets.len();
            let title = if app.lang_en {
                if n == 1 {
                    format!(" Delete [{}] ", dlg.targets[0])
                } else {
                    format!(" Delete [{} items] ", n)
                }
            } else if n == 1 {
                format!(" ファイルの削除 [{}] ", dlg.targets[0])
            } else {
                format!(" ファイルの削除 [{}件] ", n)
            };
            let content = if app.lang_en {
                if n == 1 {
                    format!("\"{}\" delete?", dlg.targets[0])
                } else {
                    format!("{} items delete?", n)
                }
            } else if n == 1 {
                format!("\"{}\" を削除しますか？", dlg.targets[0])
            } else {
                format!("{}件 を削除しますか？", n)
            };
            (title, content, "", false)
        }
        DialogKind::Rename => (
            if app.lang_en {
                " Rename ".to_string()
            } else {
                " 名前変更 ".to_string()
            },
            if app.lang_en {
                "New name: ".to_string()
            } else {
                "新名前: ".to_string()
            },
            if app.lang_en {
                "  Enter:OK  Esc:Cancel"
            } else {
                "  Enter:実行  Esc:キャンセル"
            },
            true,
        ),
        DialogKind::Copy => (
            if app.lang_en {
                " Copy ".to_string()
            } else {
                " 複写 ".to_string()
            },
            if app.lang_en {
                "Dest: ".to_string()
            } else {
                "複写先: ".to_string()
            },
            if app.lang_en {
                "  Enter:OK  Esc:Cancel"
            } else {
                "  Enter:実行  Esc:キャンセル"
            },
            true,
        ),
        DialogKind::Move => (
            if app.lang_en {
                " Move ".to_string()
            } else {
                " 移動 ".to_string()
            },
            if app.lang_en {
                "Dest: ".to_string()
            } else {
                "移動先: ".to_string()
            },
            if app.lang_en {
                "  Enter:OK  Esc:Cancel"
            } else {
                "  Enter:実行  Esc:キャンセル"
            },
            true,
        ),
        DialogKind::Mkdir => (
            if app.lang_en {
                " New Dir ".to_string()
            } else {
                " 新規ディレクトリ ".to_string()
            },
            if app.lang_en {
                "Dir name: ".to_string()
            } else {
                "ディレクトリ名: ".to_string()
            },
            if app.lang_en {
                "  Enter:OK  Esc:Cancel"
            } else {
                "  Enter:実行  Esc:キャンセル"
            },
            true,
        ),
        DialogKind::Attr => (
            if app.lang_en {
                " Chmod ".to_string()
            } else {
                " 権限変更 ".to_string()
            },
            if app.lang_en {
                "Mode(octal): ".to_string()
            } else {
                "権限(8進数): ".to_string()
            },
            if app.lang_en {
                "  Enter:OK  Esc:Cancel"
            } else {
                "  Enter:実行  Esc:キャンセル"
            },
            true,
        ),
        DialogKind::CopyNewName => (
            if app.lang_en {
                " Copy (new name) ".to_string()
            } else {
                " 複写（新名前） ".to_string()
            },
            if app.lang_en {
                "New name: ".to_string()
            } else {
                "新ファイル名: ".to_string()
            },
            if app.lang_en {
                "  Enter:OK  Esc:Cancel"
            } else {
                "  Enter:実行  Esc:キャンセル"
            },
            true,
        ),
    };

    // Top border
    let title_w = sw(&title);
    let fill_n = iw.saturating_sub(title_w);
    blit_ch(buf, dx, dy, '╭', cyan);
    blit(buf, dx + 1, dy, &title, title_w, yellow);
    blit(
        buf,
        dx + 1 + title_w as u16,
        dy,
        &"─".repeat(fill_n),
        fill_n,
        cyan,
    );
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

    // Content / input row
    blit_ch(buf, dx, dy + 1, '│', cyan);
    if show_input {
        let pw = sw(&content);
        let input_avail = iw.saturating_sub(pw).max(1);
        blit(buf, dx + 1, dy + 1, &content, pw.min(iw), yellow);
        let input_x = dx + 1 + pw.min(iw) as u16;
        let scroll = if dlg.cursor >= input_avail {
            dlg.cursor + 1 - input_avail
        } else {
            0
        };
        blit(
            buf,
            input_x,
            dy + 1,
            &" ".repeat(input_avail),
            input_avail,
            white,
        );
        for (i, &ch) in dlg.input[scroll..].iter().enumerate() {
            if i >= input_avail {
                break;
            }
            let st = if scroll + i == dlg.cursor { rev } else { white };
            blit_ch(buf, input_x + i as u16, dy + 1, ch, st);
        }
        if dlg.cursor == dlg.input.len() && dlg.cursor - scroll < input_avail {
            blit_ch(
                buf,
                input_x + (dlg.cursor - scroll) as u16,
                dy + 1,
                ' ',
                rev,
            );
        }
    } else {
        blit(
            buf,
            dx + 1,
            dy + 1,
            &padr(&trunc(&content, iw), iw),
            iw,
            white,
        );
    }
    blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

    // Error row
    blit_ch(buf, dx, dy + 2, '│', cyan);
    let err_s = dlg.error.as_deref().unwrap_or("");
    let err_st = if err_s.is_empty() { white } else { red };
    blit(
        buf,
        dx + 1,
        dy + 2,
        &padr(&trunc(err_s, iw), iw),
        iw,
        err_st,
    );
    blit_ch(buf, dx + dw as u16 - 1, dy + 2, '│', cyan);

    // Hint row
    blit_ch(buf, dx, dy + 3, '│', cyan);
    if dlg.kind == DialogKind::DeleteConfirm {
        // cursor==1 → Yes が選択、cursor==0(デフォルト) → No が選択
        let y_btn = if app.lang_en { "  Y:Yes" } else { "  Y:はい" };
        let n_btn = if app.lang_en { "  N:No  " } else { "  N:いいえ  " };
        let yw = sw(y_btn);
        let nw = sw(n_btn);
        let pad = iw.saturating_sub(yw + nw);
        let (y_st, n_st) = if dlg.cursor == 1 { (rev, dim) } else { (dim, rev) };
        blit(buf, dx + 1, dy + 3, y_btn, yw, y_st);
        blit(buf, dx + 1 + yw as u16, dy + 3, &" ".repeat(pad), pad, dim);
        blit(buf, dx + 1 + (yw + pad) as u16, dy + 3, n_btn, nw, n_st);
    } else {
        blit(buf, dx + 1, dy + 3, &padr(hint, iw), iw, dim);
    }
    blit_ch(buf, dx + dw as u16 - 1, dy + 3, '│', cyan);

    // Bottom border
    blit_ch(buf, dx, dy + 4, '╰', cyan);
    blit(buf, dx + 1, dy + 4, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, dx + dw as u16 - 1, dy + 4, '╯', cyan);
}

// ── Error message overlay ─────────────────────────────────────────────

pub(super) fn render_error_msg(frame: &mut Frame, app: &App) {
    let msg = match app.error_msg.as_deref() {
        Some(m) => m,
        None => return,
    };
    let area = frame.area();
    let dw = (area.width as usize).clamp(40, (area.width as usize).min(80));
    let iw = dw - 2;

    // メッセージを行分割し、空行を除去して最大行数に制限する
    let max_lines = ((area.height as usize).saturating_sub(6) / 2).clamp(1, 10);
    let lines: Vec<&str> = msg
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(max_lines)
        .collect();
    let n_lines = lines.len().max(1);

    // ダイアログ高さ: 上枠 + メッセージ行 + ヒント行 + 下枠
    let dh = n_lines + 3;
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = ((area.height as usize).saturating_sub(dh) / 2) as u16;

    let buf = frame.buffer_mut();
    let red = Style::default().fg(Color::Red);
    let white = Style::default().fg(Color::White);
    let dim = Style::default().fg(Color::DarkGray);
    let yellow = app.ui_colors.title;

    let title = if app.lang_en { " Error " } else { " エラー " };
    let hint = if app.lang_en { "  Press any key to close" } else { "  任意のキーで閉じる" };

    clear_rect(buf, dx, dy, dw, dh);

    let title_w = sw(title);
    let fill_n = iw.saturating_sub(title_w);
    blit_ch(buf, dx, dy, '╭', red);
    blit(buf, dx + 1, dy, title, title_w, yellow);
    blit(buf, dx + 1 + title_w as u16, dy, &"─".repeat(fill_n), fill_n, red);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', red);

    for (i, line) in lines.iter().enumerate() {
        let y = dy + 1 + i as u16;
        blit_ch(buf, dx, y, '│', red);
        blit(buf, dx + 1, y, &padr(&trunc(line, iw), iw), iw, white);
        blit_ch(buf, dx + dw as u16 - 1, y, '│', red);
    }

    let hy = dy + 1 + n_lines as u16;
    blit_ch(buf, dx, hy, '│', red);
    blit(buf, dx + 1, hy, &padr(&trunc(hint, iw), iw), iw, dim);
    blit_ch(buf, dx + dw as u16 - 1, hy, '│', red);

    let by = dy + dh as u16 - 1;
    blit_ch(buf, dx, by, '╰', red);
    blit(buf, dx + 1, by, &"─".repeat(iw), iw, red);
    blit_ch(buf, dx + dw as u16 - 1, by, '╯', red);
}

// ── Sort dialog ───────────────────────────────────────────────────────

pub(super) fn render_sort_dialog(frame: &mut Frame, app: &App) {
    if !app.show_sort_dialog {
        return;
    }
    let area = frame.area();

    // オプション定義: (key, label_ja, label_en, mode)  ← ls オプション準拠
    let opts: &[(&str, &str, &str, SortMode)] = &[
        ("N", "名前", "Name", SortMode::Name),
        ("X", "拡張子", "Ext", SortMode::Ext),
        ("S", "サイズ", "Size", SortMode::Size),
        ("T", "日付", "Date", SortMode::Date),
        ("U", "なし", "None", SortMode::None),
    ];

    // ダイアログサイズ: タイトル幅に合わせて最低幅を確保
    let title = if app.lang_en { " Sort " } else { " ファイルのソート " };
    let title_w = sw(title);
    // 各オプション行 "  X:拡張子  " の最大表示幅
    let max_opt_w = opts.iter().map(|&(k, lj, le, _)| {
        let lbl = if app.lang_en { le } else { lj };
        sw(&format!("  {}:{}", k, lbl)) + 4 // 矢印分 ▲▼ = 2 + 余白
    }).max().unwrap_or(20);
    let dw: usize = (title_w + 2).max(max_opt_w + 2).max(20);
    let iw = dw - 2;
    let dh: usize = opts.len() + 3;
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = ((area.height as usize).saturating_sub(dh) / 2) as u16;

    let buf = frame.buffer_mut();
    let bc = app.ui_colors.border;
    let title_st = app.ui_colors.title;
    let key_st = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let txt_st = Style::default().fg(Color::White);
    let active_st = Style::default().fg(Color::Yellow).add_modifier(Modifier::REVERSED);
    let dim = Style::default().fg(Color::DarkGray);

    clear_rect(buf, dx, dy, dw, dh);

    // 上枠
    let fill_n = iw.saturating_sub(title_w);
    blit_ch(buf, dx, dy, '╭', bc);
    blit(buf, dx + 1, dy, title, title_w, title_st);
    blit(buf, dx + 1 + title_w as u16, dy, &"─".repeat(fill_n), fill_n, bc);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', bc);

    for (i, &(key, label_ja, label_en, mode)) in opts.iter().enumerate() {
        let y = dy + 1 + i as u16;
        blit_ch(buf, dx, y, '│', bc);
        let label = if app.lang_en { label_en } else { label_ja };
        let is_cursor = i == app.sort_cursor;
        let is_applied = app.sort_mode == mode && mode != SortMode::None;
        // 現在のソートモードには ▲/▼ を表示
        let arrow = if is_applied {
            if app.sort_asc { " ▲" } else { " ▼" }
        } else {
            "  "
        };
        let line = format!("  {}:{}{}", key, label, arrow);
        let padded = padr(&line, iw);
        // カーソル行を REVERSED でハイライト
        let line_st = if is_cursor { active_st } else { txt_st };
        blit(buf, dx + 1, y, &padded, iw, line_st);
        // カーソル行でない場合はキー文字をシアン太字で上書き
        if !is_cursor {
            blit_ch(buf, dx + 3, y, key.chars().next().unwrap_or(' '), key_st);
        }
        blit_ch(buf, dx + dw as u16 - 1, y, '│', bc);
    }

    // ESC行
    let esc_y = dy + 1 + opts.len() as u16;
    blit_ch(buf, dx, esc_y, '│', bc);
    let esc_label = if app.lang_en { "  ESC:Cancel" } else { "  ESC:中断" };
    blit(buf, dx + 1, esc_y, &padr(esc_label, iw), iw, dim);
    blit_ch(buf, dx + dw as u16 - 1, esc_y, '│', bc);

    // 下枠
    let by = dy + dh as u16 - 1;
    blit_ch(buf, dx, by, '╰', bc);
    blit(buf, dx + 1, by, &"─".repeat(iw), iw, bc);
    blit_ch(buf, dx + dw as u16 - 1, by, '╯', bc);
}

// ── Success dialog ────────────────────────────────────────────────────

pub(super) fn render_success_msg(frame: &mut Frame, app: &App) {
    let msg = match app.success_msg.as_deref() {
        Some(m) => m,
        None => return,
    };
    let area = frame.area();
    let dw = sw(msg).clamp(24, (area.width as usize).min(60)) + 4;
    let iw = dw - 2;
    let dh = 3; // 上枠 + メッセージ行 + 下枠
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = ((area.height as usize).saturating_sub(dh) / 2) as u16;

    let buf = frame.buffer_mut();
    let green = Style::default().fg(Color::Green);
    let white = Style::default().fg(Color::White);

    let title = if app.lang_en { " Done " } else { " 完了 " };
    let title_w = sw(title);
    let fill_n = iw.saturating_sub(title_w);

    clear_rect(buf, dx, dy, dw, dh);

    blit_ch(buf, dx, dy, '╭', green);
    blit(buf, dx + 1, dy, title, title_w, app.ui_colors.title);
    blit(buf, dx + 1 + title_w as u16, dy, &"─".repeat(fill_n), fill_n, green);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', green);

    let my = dy + 1;
    blit_ch(buf, dx, my, '│', green);
    blit(buf, dx + 1, my, &padr(&trunc(msg, iw), iw), iw, white);
    blit_ch(buf, dx + dw as u16 - 1, my, '│', green);

    let by = dy + dh as u16 - 1;
    blit_ch(buf, dx, by, '╰', green);
    blit(buf, dx + 1, by, &"─".repeat(iw), iw, green);
    blit_ch(buf, dx + dw as u16 - 1, by, '╯', green);
}

// ── Help overlay ──────────────────────────────────────────────────────

pub(super) fn render_help_overlay(frame: &mut Frame, app: &App) {
    if !app.show_help {
        return;
    }
    if app.proc_mode {
        render_proc_help_overlay(frame, app);
        return;
    }
    let area = frame.area();
    let dw = (area.width as usize).min(60);
    let iw = dw - 2;

    // ヘルプ内容: (キー表示, 説明)
    let entries: &[(&str, &str, &str)] = &[
        // 見出し
        ("", "── Navigation ──", "── ナビゲーション ──"),
        ("j/k", "Move up/down", "上/下に移動"),
        ("h/l", "Move left/right (column)", "左/右の列に移動"),
        ("g/G", "First/last entry", "先頭/末尾へ"),
        ("PageUp/Dn", "Previous/next page", "前/次ページ"),
        ("Enter", "Enter dir / open file", "ディレクトリ移動/ファイルを開く"),
        ("Backspace", "Parent directory", "親ディレクトリへ"),
        ("~", "Home directory", "ホームへ"),
        ("Space", "Tag + move down", "タグ付け + 1つ下"),
        ("Home", "Toggle tag all", "全タグをトグル"),
        ("!/@ /#/%", "1/2/3/5 columns", "1/2/3/5 列表示"),
        ("Tab", "Toggle tree pane", "ツリーペイン トグル"),
        ("", "── File Ops ──", "── ファイル操作 ──"),
        ("c", "Copy", "複写"),
        ("m", "Move", "移動"),
        ("d", "Delete", "削除"),
        ("a", "Permissions (chmod)", "権限変更"),
        ("e", "Open in editor", "エディタで開く"),
        ("K", "Make directory", "ディレクトリ作成"),
        ("", "── Search/Func ──", "── 検索/機能 ──"),
        ("/", "Incremental search", "ファイル名検索"),
        ("n/N", "Next/prev match", "次/前のマッチへ"),
        (":", "Func dialog (:mv :q :help)", "機能ダイアログ"),
        ("", "── Git ──", "── Git ──"),
        ("b", "Open git submenu", "Git サブメニュー"),
        ("", "── Other ──", "── その他 ──"),
        ("F1", "This help", "このヘルプ"),
        ("q", "Quit", "終了"),
    ];

    let dh = entries.len() + 3; // 上枠 + ヒント + 下枠
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = ((area.height as usize).saturating_sub(dh) / 2) as u16;

    let buf = frame.buffer_mut();
    let bc = app.ui_colors.border;
    let title_st = app.ui_colors.title;
    let key_st = Style::default().fg(Color::Cyan);
    let desc_st = Style::default().fg(Color::White);
    let head_st = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);

    clear_rect(buf, dx, dy, dw, dh);

    // 上枠
    let title = if app.lang_en { " Help " } else { " ヘルプ " };
    let title_w = sw(title);
    let fill_n = iw.saturating_sub(title_w);
    blit_ch(buf, dx, dy, '╭', bc);
    blit(buf, dx + 1, dy, title, title_w, title_st);
    blit(buf, dx + 1 + title_w as u16, dy, &"─".repeat(fill_n), fill_n, bc);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', bc);

    // エントリ行
    for (i, &(key, desc_en, desc_ja)) in entries.iter().enumerate() {
        let y = dy + 1 + i as u16;
        blit_ch(buf, dx, y, '│', bc);
        if key.is_empty() {
            // 見出し行
            let heading = if app.lang_en { desc_en } else { desc_ja };
            let padded = padr(heading, iw);
            blit(buf, dx + 1, y, &padded, iw, head_st);
        } else {
            const KEY_W: usize = 10;
            let key_padded = padr(key, KEY_W);
            let desc = if app.lang_en { desc_en } else { desc_ja };
            let desc_w = iw.saturating_sub(KEY_W + 1);
            blit(buf, dx + 1, y, &key_padded, KEY_W, key_st);
            blit_ch(buf, dx + 1 + KEY_W as u16, y, ' ', Style::default());
            blit(buf, dx + 1 + KEY_W as u16 + 1, y, &trunc(desc, desc_w), desc_w, desc_st);
        }
        blit_ch(buf, dx + dw as u16 - 1, y, '│', bc);
    }

    // ヒント行
    let hint_y = dy + 1 + entries.len() as u16;
    let hint = if app.lang_en { "  Press any key to close" } else { "  任意のキーで閉じる" };
    blit_ch(buf, dx, hint_y, '│', bc);
    blit(buf, dx + 1, hint_y, &padr(&trunc(hint, iw), iw), iw, dim);
    blit_ch(buf, dx + dw as u16 - 1, hint_y, '│', bc);

    // 下枠
    let by = dy + dh as u16 - 1;
    blit_ch(buf, dx, by, '╰', bc);
    blit(buf, dx + 1, by, &"─".repeat(iw), iw, bc);
    blit_ch(buf, dx + dw as u16 - 1, by, '╯', bc);
}
