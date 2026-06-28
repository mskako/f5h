use super::*;
use crate::proc::{ProcEntry, RightPanel, SIGNAL_ITEMS, get_sys_info, stat_to_en, stat_to_ja};

/// proc モードのメインビュー（7行ヘッダー + プロセスリスト）
pub(super) fn render_proc_view(frame: &mut Frame, main_area: Rect, app: &App) {
    let mw = main_area.width as usize;
    let mh = main_area.height;
    let mx = main_area.x;
    let my = main_area.y;
    if mw < 40 || mh < 10 { return; }

    let iw = mw - 2;
    let buf = frame.buffer_mut();

    let cyan  = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let lc    = app.ui_colors.label;
    let green = Style::default().fg(Color::Green);
    let white = Style::default().fg(Color::White);
    let rev   = Style::default().add_modifier(Modifier::REVERSED);
    let sys = get_sys_info();
    let det = &app.proc_detail;

    // ── 上枠 (my+0) ──────────────────────────────────────────────────────
    {
        let now     = crate::fs_utils::now_str();
        let clock_s = format!(" {} ", now);
        let pre     = "─ ";
        let title   = if app.lang_en { "Proc " } else { "プロセス " };
        let n_procs = app.proc_entries.len();
        let count_n = format!(" {}", n_procs);          // 数字部分（白）
        let count_u = if app.lang_en { " procs " } else { " 個 " }; // 単位部分（水色）
        let sep     = "─── ";
        let ver_pre = "─── ";
        let ver_lbl = concat!("f5h v", env!("CARGO_PKG_VERSION"));
        let ver_suf = " ─";
        // fixed = ╭ + pre + title + sep + count_n + count_u + [fill] + clock_s + ver_pre + ver_lbl + ver_suf + ╮
        let fixed = 1 + sw(pre) + sw(title) + sw(sep) + sw(&count_n) + sw(count_u)
            + sw(&clock_s) + sw(ver_pre) + sw(ver_lbl) + sw(ver_suf) + 1;
        let mid_fill_n = mw.saturating_sub(fixed);

        let mut x = mx;
        blit_ch(buf, x, my, '╭', cyan); x += 1;
        blit(buf, x, my, pre, sw(pre), cyan); x += sw(pre) as u16;
        blit(buf, x, my, title, sw(title), yellow); x += sw(title) as u16;
        blit(buf, x, my, sep, sw(sep), cyan); x += sw(sep) as u16;
        blit(buf, x, my, &count_n, sw(&count_n), white); x += sw(&count_n) as u16;
        blit(buf, x, my, count_u, sw(count_u), lc); x += sw(count_u) as u16;
        blit(buf, x, my, &"─".repeat(mid_fill_n), mid_fill_n, cyan); x += mid_fill_n as u16;
        blit(buf, x, my, &clock_s, sw(&clock_s), app.ui_colors.clock); x += sw(&clock_s) as u16;
        blit(buf, x, my, ver_pre, sw(ver_pre), cyan); x += sw(ver_pre) as u16;
        blit(buf, x, my, ver_lbl, sw(ver_lbl), yellow); x += sw(ver_lbl) as u16;
        blit(buf, x, my, ver_suf, sw(ver_suf), cyan); x += sw(ver_suf) as u16;
        blit_ch(buf, x, my, '╮', cyan);
    }

    // ── ヘッダー内容 (inner.y+0 〜 inner.y+5) ────────────────────────────
    // 左パネル幅は通常レイアウトと同じ LEFT_W=28
    let lw  = LEFT_W;         // 左パネル幅（ボーダー内側）
    let rw  = iw - lw - 1;    // 右パネル幅
    let sx1 = mx + 1 + lw as u16;  // 仕切り縦線 x

    let ix = mx + 1;
    let iy = my + 1;

    // 各行の左右ボーダーと縦仕切り
    for row in 0..6u16 {
        blit_ch(buf, mx,              iy + row, '│', cyan);
        blit_ch(buf, sx1,             iy + row, '│', cyan);
        blit_ch(buf, mx + mw as u16 - 1, iy + row, '│', cyan);
    }

    // 項目名=lc(水色)・値=white の分割blit。新しい x を返す
    macro_rules! kv {
        ($x:expr, $y:expr, $lbl:expr, $val:expr, $vc:expr) => {{
            let x0: u16 = $x;
            let lw0 = sw($lbl);
            blit(buf, x0, $y, $lbl, lw0, lc);
            blit(buf, x0 + lw0 as u16, $y, $val, sw($val), $vc);
            x0 + lw0 as u16 + sw($val) as u16
        }};
    }
    macro_rules! fill_r {
        ($rx:expr, $y:expr) => {{
            let used = ($rx as usize).saturating_sub((sx1 + 1) as usize);
            if used < rw { blit(buf, $rx, $y, &" ".repeat(rw - used), rw - used, Style::default()); }
        }};
    }
    macro_rules! fill_l {
        ($lx:expr, $y:expr) => {{
            let used = ($lx as usize).saturating_sub(ix as usize);
            if used < lw { blit(buf, $lx, $y, &" ".repeat(lw - used), lw - used, Style::default()); }
        }};
    }

    // ── iy+0: 負荷(green keep) / PID・PPID・TTY・ユーザ・状態(日本語) ──
    {
        let load_lbl = if app.lang_en { " Load" } else { " 負荷" };
        let load_s = format!("{} {:.2} {:.2} {:.2}", load_lbl, sys.load_1, sys.load_5, sys.load_15);
        blit(buf, ix, iy, &padr(&load_s, lw), lw, green);

        let (ppid_lbl, tty_lbl, user_lbl, stat_lbl) = if app.lang_en {
            (" PPID:", " TTY:", " User:", " State:")
        } else {
            (" PPID:", " TTY:", " ユーザ:", " 状態:")
        };
        let pid_s  = padr(&format!("{}", det.pid), 6);
        let ppid_s = format!("{}", det.ppid);
        let tty_s  = det.tty.clone();
        let user_s = padr(trunc(&det.user, 10).as_str(), 11);
        // 状態を日本語/英語説明で表示（残りスペースに収める）
        let fixed_w = 6 + sw(ppid_lbl) + 6 + sw(tty_lbl) + 7 + sw(user_lbl) + 11 + sw(stat_lbl);
        let stat_avail = rw.saturating_sub(fixed_w).max(4);
        let stat_desc = if app.lang_en { stat_to_en(&det.stat) } else { stat_to_ja(&det.stat) };
        let stat_s = trunc(&stat_desc, stat_avail);
        let mut rx = sx1 + 1;
        rx = kv!(rx, iy, " PID:", &pid_s, white);
        rx = kv!(rx, iy, ppid_lbl, &padr(&ppid_s, 6), white);
        rx = kv!(rx, iy, tty_lbl, &padr(&tty_s, 7), white);
        rx = kv!(rx, iy, user_lbl, &user_s, white);
        rx = kv!(rx, iy, stat_lbl, &stat_s, white);
        fill_r!(rx, iy);
    }

    // ── iy+1: CPUコア数 / %CPU・%MEM・VSZ・RSS ───────────────────────────
    {
        let (vsz_n, vsz_u) = fmt_cpt_parts(det.vsz * 1024);
        let (rss_n, rss_u) = fmt_cpt_parts(det.rss * 1024);
        let core_lbl = if app.lang_en { " cores" } else { " コア" };
        let cores_s  = format!("{}", sys.cpu_online);
        let mut lx = ix;
        blit(buf, lx, iy + 1, " CPU ", 5, lc); lx += 5;
        blit(buf, lx, iy + 1, &cores_s, sw(&cores_s), white); lx += sw(&cores_s) as u16;
        blit(buf, lx, iy + 1, core_lbl, sw(core_lbl), lc); lx += sw(core_lbl) as u16;
        fill_l!(lx, iy + 1);

        let cpu_s = padr(&format!("{:.1}", det.cpu), 5);
        let mem_s = padr(&format!("{:.1}", det.mem), 5);
        let vsz_s = padl(&format!("{}{}", vsz_n, vsz_u), 7);
        let rss_s = padl(&format!("{}{}", rss_n, rss_u), 6);
        let mut rx = sx1 + 1;
        rx = kv!(rx, iy + 1, " %CPU:", &cpu_s, white);
        rx = kv!(rx, iy + 1, " %MEM:", &mem_s, white);
        rx = kv!(rx, iy + 1, " VSZ:", &vsz_s, white);
        rx = kv!(rx, iy + 1, "  RSS:", &rss_s, white);
        fill_r!(rx, iy + 1);
    }

    // ── iy+2: メモリ / コマンド名(yellow keep) ───────────────────────────
    {
        let (mu, muu) = fmt_cpt_parts(sys.mem_used);
        let (mt, mtu) = fmt_cpt_parts(sys.mem_total);
        let pct     = if sys.mem_total > 0 { sys.mem_used as f64 / sys.mem_total as f64 * 100.0 } else { 0.0 };
        let mem_lbl = if app.lang_en { " Mem " } else { " メモリ " };
        let mem_val = format!("{}{}/{}{} {:.0}%", mu, muu, mt, mtu, pct);
        let mut lx = ix;
        blit(buf, lx, iy + 2, mem_lbl, sw(mem_lbl), lc); lx += sw(mem_lbl) as u16;
        blit(buf, lx, iy + 2, &mem_val, sw(&mem_val), white); lx += sw(&mem_val) as u16;
        fill_l!(lx, iy + 2);

        let cmd_lbl = if app.lang_en { " CMD: " } else { " コマンド: " };
        let cmd_val = trunc(&det.command, rw.saturating_sub(sw(cmd_lbl)));
        let mut rx = sx1 + 1;
        blit(buf, rx, iy + 2, cmd_lbl, sw(cmd_lbl), yellow); rx += sw(cmd_lbl) as u16;
        blit(buf, rx, iy + 2, &padr(&cmd_val, rw.saturating_sub(sw(cmd_lbl))), rw.saturating_sub(sw(cmd_lbl)), yellow);
        fill_r!(rx + padr(&cmd_val, rw.saturating_sub(sw(cmd_lbl))).len() as u16, iy + 2);
    }

    // ── iy+3: スワップ / 引数 ────────────────────────────────────────────
    {
        let (su, suu) = fmt_cpt_parts(sys.swap_used);
        let (st, stu) = fmt_cpt_parts(sys.swap_total);
        let swap_lbl = if app.lang_en { " Swap " } else { " スワップ " };
        let swap_val = format!("{}{}/{}{}", su, suu, st, stu);
        let mut lx = ix;
        blit(buf, lx, iy + 3, swap_lbl, sw(swap_lbl), lc); lx += sw(swap_lbl) as u16;
        blit(buf, lx, iy + 3, &swap_val, sw(&swap_val), white); lx += sw(&swap_val) as u16;
        fill_l!(lx, iy + 3);

        let args_lbl = if app.lang_en { " args: " } else { " 引数: " };
        let args_avail = rw.saturating_sub(sw(args_lbl));
        let args_val = padr(trunc(&det.cmdline, args_avail).as_str(), args_avail);
        let mut rx = sx1 + 1;
        rx = kv!(rx, iy + 3, args_lbl, &args_val, white);
        fill_r!(rx, iy + 3);
    }

    // ── iy+4: 稼働時間 / 開始・経過 ─────────────────────────────────────
    {
        let up     = crate::proc::format_elapsed(sys.uptime_secs);
        let up_lbl = if app.lang_en { " Uptime " } else { " 稼働 " };
        let mut lx = ix;
        blit(buf, lx, iy + 4, up_lbl, sw(up_lbl), lc); lx += sw(up_lbl) as u16;
        blit(buf, lx, iy + 4, &up, sw(&up), white); lx += sw(&up) as u16;
        fill_l!(lx, iy + 4);

        let (sl, el) = if app.lang_en { (" Started: ", "  Elapsed: ") } else { (" 開始: ", "  経過: ") };
        let mut rx = sx1 + 1;
        rx = kv!(rx, iy + 4, sl, &det.started_str, white);
        rx = kv!(rx, iy + 4, el, &det.elapsed_str, white);
        fill_r!(rx, iy + 4);
    }

    // ── iy+5: (空白) / 作業Dir・スレッド・FD ────────────────────────────
    {
        blit(buf, ix, iy + 5, &" ".repeat(lw), lw, Style::default());

        let (cwd_lbl, thr_lbl, fd_lbl) = if app.lang_en {
            (" CWD: ", "  Threads: ", "  FDs: ")
        } else {
            (" 作業Dir: ", "  スレッド: ", "  FD: ")
        };
        let thr_s    = format!("{}", det.threads);
        let fd_s     = format!("{}", det.fd_count);
        let suffix_w = sw(thr_lbl) + sw(&thr_s) + sw(fd_lbl) + sw(&fd_s);
        let cwd_avail = rw.saturating_sub(sw(cwd_lbl) + suffix_w);
        let cwd_s    = padr(trunc(&det.cwd, cwd_avail).as_str(), cwd_avail);
        let mut rx = sx1 + 1;
        rx = kv!(rx, iy + 5, cwd_lbl, &cwd_s, white);
        rx = kv!(rx, iy + 5, thr_lbl, &thr_s, white);
        rx = kv!(rx, iy + 5, fd_lbl, &fd_s, white);
        fill_r!(rx, iy + 5);
    }

    // ── セパレータ (info_bot_y = my + HEADER_ROWS) ────────────────────────
    let sep_y = my + HEADER_ROWS;
    blit_ch(buf, mx,               sep_y, '├', cyan);
    blit(buf, ix,                  sep_y, &"─".repeat(lw), lw, cyan);
    blit_ch(buf, sx1,              sep_y, '┴', cyan);
    blit(buf, sx1 + 1,             sep_y, &"─".repeat(rw), rw, cyan);
    blit_ch(buf, mx + mw as u16 - 1, sep_y, '┤', cyan);

    // ── 右パネルの分割計算 ───────────────────────────────────────────────
    let has_panel = app.right_panel != RightPanel::None;
    let panel_visible = has_panel && iw >= 100;
    let (list_iw, panel_w, sx2): (usize, usize, u16) = if panel_visible {
        let liw = iw * 3 / 5;
        let pw  = iw - liw - 1;
        let s2  = mx + 1 + liw as u16;
        (liw, pw, s2)
    } else {
        (iw, 0, 0)
    };

    // ── カラムヘッダー (my + HEADER_ROWS + 1) ─────────────────────────────
    let col_hdr_y = my + HEADER_ROWS + 1;
    {
        blit_ch(buf, mx, col_hdr_y, '│', cyan);
        let sort_lbl = app.proc_sort.label();
        let sort_arrow = if app.proc_sort_asc { "▲" } else { "▼" };
        let user_hdr = if app.lang_en { "USER" } else { "ユーザ" };
        let cmd_hdr  = if app.lang_en { "COMMAND" } else { "コマンド" };
        let hdr = if app.proc_tree {
            format!(" {} {} {} {}", "S", padl("PID", 6), padr(user_hdr, 8), cmd_hdr)
        } else {
            format!(
                " {} {} {} {:>5} {:>5} {:>7} {:>6} {:4} {}",
                padl("PID", 6), padl("PPID", 6), padr(user_hdr, 8),
                "%CPU", "%MEM", "VSZ", "RSS", "STAT", cmd_hdr
            )
        };
        // 右インジケーター（パネルが開いているとき付加）
        let panel_tag = if has_panel && !panel_visible {
            match app.right_panel {
                RightPanel::Fd => " [FD]",
                RightPanel::Threads => " [THR]",
                RightPanel::None => "",
            }
        } else { "" };
        let base_ind = if app.proc_tree {
            if app.lang_en { "[T:List]".to_string() } else { "[T:一覧]".to_string() }
        } else {
            format!("[{}{}]", sort_lbl, sort_arrow)
        };
        let right_ind = format!("{}{}", base_ind, panel_tag);
        blit(buf, mx + 1, col_hdr_y, &padr(&hdr, list_iw), list_iw, lc);
        let si_x = mx + 1 + list_iw as u16 - 1 - sw(&right_ind) as u16;
        blit(buf, si_x, col_hdr_y, &right_ind, sw(&right_ind), yellow);
        if panel_visible {
            blit_ch(buf, sx2, col_hdr_y, '│', cyan);
        }
        blit_ch(buf, mx + mw as u16 - 1, col_hdr_y, '│', cyan);
    }

    // ── プロセスリスト ────────────────────────────────────────────────────
    let list_start_y = my + HEADER_ROWS + 2;
    let list_h = (my + mh - 1).saturating_sub(list_start_y) as usize;
    let n = if app.proc_tree { app.proc_tree_rows.len() } else { app.proc_entries.len() };
    let offset = if app.proc_cursor < app.proc_offset {
        app.proc_cursor
    } else if n > 0 && app.proc_cursor >= app.proc_offset + list_h {
        app.proc_cursor + 1 - list_h
    } else {
        app.proc_offset
    };

    for row_i in 0..list_h {
        let screen_y = list_start_y + row_i as u16;
        if screen_y >= my + mh - 1 { break; }
        blit_ch(buf, mx, screen_y, '│', cyan);
        let entry_idx = offset + row_i;
        if entry_idx >= n {
            blit(buf, mx + 1, screen_y, &" ".repeat(list_iw), list_iw, Style::default());
        } else {
            let is_cur = entry_idx == app.proc_cursor;
            let e: &ProcEntry = if app.proc_tree {
                &app.proc_entries[app.proc_tree_rows[entry_idx].idx]
            } else {
                &app.proc_entries[entry_idx]
            };
            let base_style = proc_entry_style(e);
            let cursor_style = if is_cur {
                if app.right_panel_focus {
                    // 右フォーカス中は DarkGray でカーソル位置を示す
                    base_style.bg(Color::DarkGray)
                } else {
                    base_style.add_modifier(Modifier::REVERSED)
                }
            } else {
                base_style
            };
            let line = if app.proc_tree {
                let prefix = &app.proc_tree_rows[entry_idx].prefix;
                let prefix_w = sw(prefix);
                let cmd_avail = list_iw.saturating_sub(prefix_w + 2 + 7 + 9);
                format!(
                    "{}{} {:>6} {:<8}  {}",
                    prefix,
                    e.stat.chars().next().unwrap_or('?'),
                    e.pid, trunc(&e.user, 8),
                    trunc(&e.command, cmd_avail),
                )
            } else {
                let (vsz_n, vsz_u) = fmt_cpt_parts(e.vsz * 1024);
                let (rss_n, rss_u) = fmt_cpt_parts(e.rss * 1024);
                let cmd_avail = list_iw.saturating_sub(57);
                format!(
                    " {:>6} {:>6} {:<8} {:>5.1} {:>5.1} {:>7} {:>6} {:<4} {}",
                    e.pid, e.ppid, trunc(&e.user, 8), e.cpu, e.mem,
                    padl(&format!("{}{}", vsz_n, vsz_u), 7),
                    padl(&format!("{}{}", rss_n, rss_u), 6),
                    trunc(&e.stat, 4), trunc(&e.command, cmd_avail),
                )
            };
            blit(buf, mx + 1, screen_y, &padr(&line, list_iw), list_iw, cursor_style);
        }
        if panel_visible {
            blit_ch(buf, sx2, screen_y, '│', cyan);
        }
        blit_ch(buf, mx + mw as u16 - 1, screen_y, '│', cyan);
    }

    // 余白行
    for row_i in (list_h + list_start_y as usize)..(my as usize + mh as usize - 1) {
        let screen_y = row_i as u16;
        blit_ch(buf, mx, screen_y, '│', cyan);
        blit(buf, mx + 1, screen_y, &" ".repeat(list_iw), list_iw, Style::default());
        if panel_visible {
            blit_ch(buf, sx2, screen_y, '│', cyan);
            blit(buf, sx2 + 1, screen_y, &" ".repeat(panel_w), panel_w, Style::default());
        }
        blit_ch(buf, mx + mw as u16 - 1, screen_y, '│', cyan);
    }

    // ── 右パネルを描画 ───────────────────────────────────────────────────
    if panel_visible {
        match app.right_panel {
            RightPanel::Fd => render_fd_panel(
                buf, sx2 + 1, col_hdr_y, list_start_y, panel_w, list_h, app,
            ),
            RightPanel::Threads => render_thread_panel(
                buf, sx2 + 1, col_hdr_y, list_start_y, panel_w, list_h, app,
            ),
            RightPanel::None => {}
        }
    }

    // ── 下枠 ─────────────────────────────────────────────────────────────
    {
        let by = my + mh - 1;
        let page = if list_h > 0 { offset / list_h + 1 } else { 1 };
        let total_pages = if list_h > 0 { (n + list_h - 1).max(1) / list_h } else { 1 };
        let f1_lbl = if app.lang_en { "F1:Help" } else { "F1:ヘルプ" };
        let status = format!(" {}  {:>3}/{:<3} {}", f1_lbl, page, total_pages, app.labels.page_unit);
        let st_w = sw(&status);
        blit_ch(buf, mx, by, '╰', cyan);
        if panel_visible {
            // 縦仕切りの下端: ┴
            let fill_left = (sx2 - mx - 1) as usize;
            blit(buf, mx + 1, by, &"─".repeat(fill_left), fill_left, cyan);
            blit_ch(buf, sx2, by, '┴', cyan);
            let fill_right = (mw - 2).saturating_sub(fill_left + 1).saturating_sub(st_w);
            blit(buf, sx2 + 1, by, &"─".repeat(fill_right), fill_right, cyan);
            blit(buf, sx2 + 1 + fill_right as u16, by, &status, st_w, rev);
        } else {
            render_bottom_fill(buf, mx, by, mw, &status, st_w, rev, app.is_root);
        }
        blit_ch(buf, mx + mw as u16 - 1, by, '╯', cyan);
    }
}

/// FD パネル（右半分）を描画する
fn render_fd_panel(
    buf: &mut Buffer,
    x: u16,
    hdr_y: u16,
    content_y: u16,
    w: usize,
    list_h: usize,
    app: &App,
) {
    let lc    = app.ui_colors.label;
    let white = Style::default().fg(Color::White);
    let rev   = Style::default().add_modifier(Modifier::REVERSED);
    let red   = Style::default().fg(Color::Red);

    // ヘッダー行
    let pid_s = format!("FD:{}", app.fd_pid);
    let type_hdr = if app.lang_en { "TYPE  " } else { "種別  " };
    let hdr = format!(" {:<5} {} INFO", padr(&pid_s, 5), type_hdr);
    blit(buf, x, hdr_y, &padr(&hdr, w), w, lc);

    // コンテンツ
    let n = app.fd_entries.len();
    let offset = if app.fd_cursor < app.fd_offset {
        app.fd_cursor
    } else if n > 0 && app.fd_cursor >= app.fd_offset + list_h {
        app.fd_cursor + 1 - list_h
    } else {
        app.fd_offset
    };

    if let Some(ref err_msg) = app.fd_error {
        for row_i in 0..list_h {
            let screen_y = content_y + row_i as u16;
            if row_i == list_h / 2 {
                let msg = format!(" ⚠ {}", err_msg);
                blit(buf, x, screen_y, &padr(&msg, w), w, red);
            } else {
                blit(buf, x, screen_y, &" ".repeat(w), w, Style::default());
            }
        }
        return;
    }

    for row_i in 0..list_h {
        let screen_y = content_y + row_i as u16;
        let entry_idx = offset + row_i;
        if entry_idx >= n {
            blit(buf, x, screen_y, &" ".repeat(w), w, Style::default());
        } else {
            let e = &app.fd_entries[entry_idx];
            let is_cur = entry_idx == app.fd_cursor && app.right_panel_focus;
            let style = if is_cur { rev } else { white };
            // type tag 4 chars (trunc of fd_type.tag())
            let type_tag = trunc(e.fd_type.tag().trim(), 4);
            let info = if !e.proto.is_empty() {
                // ソケット: proto + local→remote + state (省略形)
                let state_abbr = match e.sock_state.as_str() {
                    "ESTABLISHED" => "EST",
                    "LISTEN"      => "LSN",
                    "TIME_WAIT"   => "TW",
                    "CLOSE_WAIT"  => "CW",
                    "SYN_SENT"    => "SYS",
                    "SYN_RECV"    => "SYR",
                    "CLOSING"     => "CLG",
                    s => s,
                };
                let proto_s = format!("{:<5}", e.proto);
                if e.remote_addr.is_empty() {
                    format!("{} {}  {}", proto_s, e.local_addr, state_abbr)
                } else {
                    format!("{} {}→{} {}", proto_s, e.local_addr, e.remote_addr, state_abbr)
                }
            } else if e.fd_type == crate::proc::FdType::Pipe || e.fd_type == crate::proc::FdType::Anon {
                e.target.clone()
            } else {
                // ファイル: パスのみ
                e.target.clone()
            };
            let info_avail = w.saturating_sub(9);
            let line = format!(" {:>3}  {:<4} {}", e.fd, type_tag, trunc(&info, info_avail));
            blit(buf, x, screen_y, &padr(&line, w), w, style);
        }
    }
}

/// スレッドパネル（右半分）を描画する
fn render_thread_panel(
    buf: &mut Buffer,
    x: u16,
    hdr_y: u16,
    content_y: u16,
    w: usize,
    list_h: usize,
    app: &App,
) {
    use crate::proc::ThreadEntry;
    let lc  = app.ui_colors.label;
    let red = Style::default().fg(Color::Red);

    // ヘッダー行
    let pid_s = format!("PID:{}", app.thread_pid);
    let hdr = format!(" {:<7} ST TICKS  NAME", padr(&pid_s, 7));
    blit(buf, x, hdr_y, &padr(&hdr, w), w, lc);

    // コンテンツ
    let n = app.thread_entries.len();
    let offset = if app.thread_cursor < app.thread_offset {
        app.thread_cursor
    } else if n > 0 && app.thread_cursor >= app.thread_offset + list_h {
        app.thread_cursor + 1 - list_h
    } else {
        app.thread_offset
    };

    if let Some(ref err_msg) = app.thread_error {
        for row_i in 0..list_h {
            let screen_y = content_y + row_i as u16;
            if row_i == list_h / 2 {
                let msg = format!(" ⚠ {}", err_msg);
                blit(buf, x, screen_y, &padr(&msg, w), w, red);
            } else {
                blit(buf, x, screen_y, &" ".repeat(w), w, Style::default());
            }
        }
        return;
    }

    for row_i in 0..list_h {
        let screen_y = content_y + row_i as u16;
        let entry_idx = offset + row_i;
        if entry_idx >= n {
            blit(buf, x, screen_y, &" ".repeat(w), w, Style::default());
        } else {
            let e: &ThreadEntry = &app.thread_entries[entry_idx];
            let is_cur = entry_idx == app.thread_cursor && app.right_panel_focus;
            // スレッド状態に基づく色
            let base_style = match e.state {
                'R' => Style::default().fg(Color::Yellow),
                'T' | 't' => Style::default().fg(Color::Cyan),
                'D' => Style::default().fg(Color::Green),
                'Z' => Style::default().fg(Color::Magenta),
                _   => Style::default().fg(Color::White),
            };
            let style = if is_cur { base_style.add_modifier(Modifier::REVERSED) } else { base_style };
            let name_avail = w.saturating_sub(22);
            let line = format!(
                " {:>6} {}  {:>8}  {}",
                e.tid, e.state,
                e.cpu_ticks,
                trunc(&e.name, name_avail),
            );
            blit(buf, x, screen_y, &padr(&line, w), w, style);
        }
    }
}


/// プロセス状態に応じたベーススタイル（カーソル時は REVERSED を追加して使う）
fn proc_entry_style(e: &ProcEntry) -> Style {
    if e.cpu >= 50.0 {
        return Style::default().fg(Color::Red);
    }
    match e.stat.chars().next().unwrap_or(' ') {
        'R' => Style::default().fg(Color::Yellow),
        'T' | 't' => Style::default().fg(Color::Cyan),
        'D' => Style::default().fg(Color::Green),
        'Z' => Style::default().fg(Color::Magenta),
        _ => Style::default().fg(Color::White),
    }
}

/// シグナルメニューオーバーレイ
pub(super) fn render_proc_signal_menu(frame: &mut Frame, app: &App) {
    let menu = match app.proc_signal_menu.as_ref() {
        Some(m) => m,
        None => return,
    };

    let area = frame.area();
    // 幅: "  k  SIGKILL ( 9)  force kill  " ≈ 36 + 両端 2 = 38
    let dw: usize = 42;
    let dh: usize = SIGNAL_ITEMS.len() + 2; // 上枠 + 各行 + 下枠
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = ((area.height as usize).saturating_sub(dh) / 2) as u16;

    let buf = frame.buffer_mut();
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let white = Style::default().fg(Color::White);
    let rev = Style::default().add_modifier(Modifier::REVERSED);

    let title = format!(" Signal: {} (PID {}) ", menu.proc_name, menu.pid);
    let title_w = sw(&title);
    let iw = dw - 2;

    // 上枠
    clear_rect(buf, dx, dy, dw, dh);
    blit_ch(buf, dx, dy, '╭', cyan);
    if title_w <= iw {
        blit(buf, dx + 1, dy, &title, title_w, yellow);
        let fill = iw.saturating_sub(title_w);
        blit(buf, dx + 1 + title_w as u16, dy, &"─".repeat(fill), fill, cyan);
    } else {
        blit(buf, dx + 1, dy, &"─".repeat(iw), iw, cyan);
    }
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

    // シグナル行
    for (i, &(_sig, key_ch, name, desc)) in SIGNAL_ITEMS.iter().enumerate() {
        let row_y = dy + 1 + i as u16;
        let is_sel = i == menu.cursor;
        let row_style = if is_sel {
            rev
        } else {
            white
        };
        blit_ch(buf, dx, row_y, '│', cyan);
        let key_s = format!("  {}  ", key_ch);
        let sig_s = format!("{}", name);
        let desc_s = format!("  {}", desc);
        let content = format!("{}{}{}", key_s, sig_s, desc_s);
        blit(buf, dx + 1, row_y, &padr(&content, iw), iw, row_style);
        blit_ch(buf, dx + dw as u16 - 1, row_y, '│', cyan);
    }

    // 下枠
    let bot_y = dy + dh as u16 - 1;
    blit_ch(buf, dx, bot_y, '╰', cyan);
    blit(buf, dx + 1, bot_y, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, dx + dw as u16 - 1, bot_y, '╯', cyan);
}

pub(super) fn render_proc_help_overlay(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let dw = (area.width as usize).min(60);
    let iw = dw - 2;

    let entries: &[(&str, &str, &str)] = &[
        ("", "── プロセス一覧 ──", "── Process List ──"),
        ("j/k",       "上/下に移動",       "Move up/down"),
        ("g/G",       "先頭/末尾へ",       "First/last entry"),
        ("PageUp/Dn", "前/次ページ",       "Previous/next page"),
        ("s",         "ソート切替",         "Cycle sort mode"),
        ("t",         "ツリー表示切替",     "Toggle tree view"),
        ("x",         "シグナル送信メニュー", "Signal menu"),
        ("f/Enter",   "FD パネル開閉",      "Toggle FD panel"),
        ("T",         "スレッドパネル開閉", "Toggle thread panel"),
        ("Tab",       "パネルへフォーカス", "Focus right panel"),
        ("r",         "プロセス一覧を更新", "Refresh list"),
        ("q/Esc",     "パネル閉/ファイルへ", "Close panel/File mode"),
        ("", "── パネル操作（Tab で移動後）──", "── Panel (after Tab) ──"),
        ("j/k",       "上/下にスクロール",  "Scroll up/down"),
        ("r",         "パネル更新",         "Reload panel"),
        ("Tab/Esc/q", "プロセスリストへ戻る", "Back to proc list"),
        ("", "── シグナルメニュー ──", "── Signal Menu ──"),
        ("h",         "SIGHUP (1)  再起動/リロード", "SIGHUP (1)  reload"),
        ("i",         "SIGINT (2)  割り込み",        "SIGINT (2)  interrupt"),
        ("k",         "SIGKILL (9) 強制終了",        "SIGKILL (9) force kill"),
        ("t",         "SIGTERM(15) 終了要求",        "SIGTERM(15) terminate"),
        ("c",         "SIGCONT(18) 再開",            "SIGCONT(18) continue"),
        ("s",         "SIGSTOP(20) 一時停止",        "SIGSTOP(20) stop"),
        ("", "── その他 ──", "── Other ──"),
        ("F1",        "このヘルプ",  "This help"),
    ];

    let dh = entries.len() + 3;
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

    let title = if app.lang_en { " Proc Help " } else { " プロセス ヘルプ " };
    let title_w = sw(title);
    let fill_n = iw.saturating_sub(title_w);
    blit_ch(buf, dx, dy, '╭', bc);
    blit(buf, dx + 1, dy, title, title_w, title_st);
    blit(buf, dx + 1 + title_w as u16, dy, &"─".repeat(fill_n), fill_n, bc);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', bc);

    for (i, &(key, desc_ja, desc_en)) in entries.iter().enumerate() {
        let y = dy + 1 + i as u16;
        blit_ch(buf, dx, y, '│', bc);
        if key.is_empty() {
            let heading = if app.lang_en { desc_en } else { desc_ja };
            blit(buf, dx + 1, y, &padr(heading, iw), iw, head_st);
        } else {
            const KEY_W: usize = 10;
            let desc = if app.lang_en { desc_en } else { desc_ja };
            let desc_w = iw.saturating_sub(KEY_W + 1);
            blit(buf, dx + 1, y, &padr(key, KEY_W), KEY_W, key_st);
            blit_ch(buf, dx + 1 + KEY_W as u16, y, ' ', Style::default());
            blit(buf, dx + 1 + KEY_W as u16 + 1, y, &trunc(desc, desc_w), desc_w, desc_st);
        }
        blit_ch(buf, dx + dw as u16 - 1, y, '│', bc);
    }

    let hint_y = dy + 1 + entries.len() as u16;
    let hint = if app.lang_en { "  Press any key to close" } else { "  任意のキーで閉じる" };
    blit_ch(buf, dx, hint_y, '│', bc);
    blit(buf, dx + 1, hint_y, &padr(&trunc(hint, iw), iw), iw, dim);
    blit_ch(buf, dx + dw as u16 - 1, hint_y, '│', bc);

    let by = dy + dh as u16 - 1;
    blit_ch(buf, dx, by, '╰', bc);
    blit(buf, dx + 1, by, &"─".repeat(iw), iw, bc);
    blit_ch(buf, dx + dw as u16 - 1, by, '╯', bc);
}
