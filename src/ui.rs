use ratatui::{prelude::*, widgets::Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, DialogKind, FileEntry, FuncDialog, GitDialogState, RemoteOp, SortMode, FUNC_CMDS};
use crate::proc::{SIGNAL_ITEMS, get_sys_info};
use crate::fs_utils::now_str;

// ── Layout constants ───────────────────────────────────────────────────

/// Left section (ボリューム情報) fixed display width
pub const LEFT_W: usize = 28;
/// Inner rows occupied by the info header (before the file list)
pub const HEADER_ROWS: u16 = 7;
/// proc モードのリストヘッダー行数（カラムヘッダー1行）
pub const PROC_LIST_HEADER: u16 = HEADER_ROWS + 1;

// ── String / display helpers ───────────────────────────────────────────

pub fn sw(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn trunc(s: &str, max_cols: usize) -> String {
    let mut w = 0usize;
    let mut r = String::new();
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(1);
        if w + cw > max_cols {
            if w < max_cols {
                r.push('~');
            }
            break;
        }
        r.push(ch);
        w += cw;
    }
    r
}

pub fn padr(s: &str, cols: usize) -> String {
    let w = sw(s);
    if w >= cols {
        trunc(s, cols)
    } else {
        format!("{}{}", s, " ".repeat(cols - w))
    }
}

pub fn padl(s: &str, cols: usize) -> String {
    let w = sw(s);
    if w >= cols {
        trunc(s, cols)
    } else {
        format!("{}{}", " ".repeat(cols - w), s)
    }
}

pub fn blit(buf: &mut Buffer, x: u16, y: u16, text: &str, max_cols: usize, style: Style) {
    buf.set_string(x, y, trunc(text, max_cols), style);
}

/// ダイアログ背景を空白で塗りつぶす（透け防止）
fn clear_rect(buf: &mut Buffer, x: u16, y: u16, w: usize, h: usize) {
    let blank = " ".repeat(w);
    let sty = Style::default();
    for row in 0..h as u16 {
        blit(buf, x, y + row, &blank, w, sty);
    }
}

pub fn blit_ch(buf: &mut Buffer, x: u16, y: u16, ch: char, style: Style) {
    if let Some(cell) = buf.cell_mut(Position::new(x, y)) {
        cell.set_char(ch);
        cell.set_style(style);
    }
}

/// Raw byte count; ≥1 TB shown as TB notation
pub fn fmt_size(n: u64) -> String {
    if n < 1_000_000_000_000u64 {
        format!("{}", n)
    } else {
        format!("{:.1}TB", n as f64 / 1_000_000_000_000.0)
    }
}

pub fn fmt_cpt_parts(n: u64) -> (String, &'static str) {
    const K: u64 = 1024;
    const M: u64 = 1024 * 1024;
    const G: u64 = 1024 * 1024 * 1024;
    if n < K {
        (format!("{}", n), "B")
    } else if n < M {
        (format!("{}", n / K), "Ki")
    } else if n < G {
        (format!("{}", n / M), "Mi")
    } else {
        (format!("{}", n / G), "Gi")
    }
}

pub fn fmt_mode(mode: u32, is_dir: bool, is_link: bool) -> String {
    let ft = if is_link {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    };
    let bit = |mask: u32, ch: char| -> char { if mode & mask != 0 { ch } else { '-' } };
    format!(
        "{}{}{}{}{}{}{}{}{}{}",
        ft,
        bit(0o400, 'r'),
        bit(0o200, 'w'),
        bit(0o100, 'x'),
        bit(0o040, 'r'),
        bit(0o020, 'w'),
        bit(0o010, 'x'),
        bit(0o004, 'r'),
        bit(0o002, 'w'),
        bit(0o001, 'x'),
    )
}

pub fn ls_style(e: &FileEntry, colors: &std::collections::HashMap<String, Style>) -> Style {
    let fallback = Style::default();
    if e.is_link {
        return colors.get("ln").copied().unwrap_or(fallback);
    }
    if e.is_dir {
        let sticky = e.mode & 0o1000 != 0;
        let other_write = e.mode & 0o002 != 0;
        let key = if sticky && other_write {
            "tw"
        } else if other_write {
            "ow"
        } else if sticky {
            "st"
        } else {
            "di"
        };
        return colors
            .get(key)
            .or_else(|| colors.get("di"))
            .copied()
            .unwrap_or(fallback);
    }
    if e.mode & 0o111 != 0 {
        return colors.get("ex").copied().unwrap_or(fallback);
    }
    if let Some(dot) = e.name.rfind('.').filter(|&d| d > 0) {
        let ext = e.name[dot + 1..].to_lowercase();
        if let Some(s) = colors.get(&format!("ext:{}", ext)) {
            return *s;
        }
    }
    fallback
}

// ── Top-level render entry point ───────────────────────────────────────

pub fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let [menu_area, main_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(10)]).areas(area);

    render_menu(frame, menu_area, app);

    if app.proc_mode {
        if app.fd_mode {
            render_fd_view(frame, main_area, app);
        } else {
            render_proc_view(frame, main_area, app);
            render_proc_signal_menu(frame, app);
        }
        render_error_msg(frame, app);
        render_success_msg(frame, app);
        render_help_overlay(frame, app);
        return;
    }

    let mw = main_area.width as usize;
    let mh = main_area.height;
    let mx = main_area.x;
    let my = main_area.y;

    if mw < LEFT_W + 20 || mh < HEADER_ROWS + 4 {
        return;
    }

    let iw = mw - 2;
    let ih = mh.saturating_sub(2);
    let inner = Rect::new(mx + 1, my + 1, iw as u16, ih);

    let list_h = (ih as usize).saturating_sub(HEADER_ROWS as usize);
    let lh = list_h.max(1);

    let list_area = Rect::new(inner.x, inner.y + HEADER_ROWS, inner.width, list_h as u16);

    render_header(frame, main_area, inner, app);

    if app.tree_open {
        // ツリー幅 = 画面幅の約20%（最小10、最大40）
        let tw = (iw / 5).clamp(10, 40).min(list_area.width as usize / 2);
        let sep_x = list_area.x + tw as u16;
        let tree_area = Rect::new(list_area.x, list_area.y, tw as u16, list_area.height);
        let file_area = Rect::new(
            sep_x + 1,
            list_area.y,
            list_area.width.saturating_sub(tw as u16 + 1),
            list_area.height,
        );
        render_tree(frame, tree_area, app);
        {
            let buf = frame.buffer_mut();
            let bc = app.ui_colors.border;
            for y in list_area.y..(list_area.y + list_area.height) {
                blit_ch(buf, sep_x, y, '│', bc);
            }
        }
        render_file_list(frame, file_area, app, lh);
    } else {
        render_file_list(frame, list_area, app, lh);
    }

    let cyan = app.ui_colors.border;
    let buf = frame.buffer_mut();

    let info_bot_y = inner.y + HEADER_ROWS - 1;
    let title_y = inner.y + 1;

    for y in (my + 1)..(my + mh - 1) {
        if y == info_bot_y {
            blit_ch(buf, mx, y, '├', cyan);
            blit_ch(buf, mx + main_area.width - 1, y, '┤', cyan);
        } else if y == title_y {
            blit_ch(buf, mx, y, '│', cyan);
            blit_ch(buf, mx + main_area.width - 1, y, '┤', cyan);
        } else {
            blit_ch(buf, mx, y, '│', cyan);
            blit_ch(buf, mx + main_area.width - 1, y, '│', cyan);
        }
    }

    // Bottom border: 検索バーが active なら左側に表示、右端にページ情報
    let by = my + mh - 1;
    let page = app.current_page(lh) + 1;
    let total = app.total_pages(lh);
    let sort_indicator = match app.sort_mode {
        SortMode::None => String::new(),
        SortMode::Name => format!(" [N{}]", if app.sort_asc { "▲" } else { "▼" }),
        SortMode::Ext  => format!(" [X{}]", if app.sort_asc { "▲" } else { "▼" }),
        SortMode::Size => format!(" [S{}]", if app.sort_asc { "▲" } else { "▼" }),
        SortMode::Date => format!(" [T{}]", if app.sort_asc { "▲" } else { "▼" }),
    };
    let status_text = format!(
        "{}{}  {:>3}/{:<3} {}",
        app.labels.f1_help, sort_indicator, page, total, app.labels.page_unit
    );
    let st_w = sw(&status_text);
    let rev = Style::default().add_modifier(Modifier::REVERSED);
    blit_ch(buf, mx, by, '╰', cyan);
    if app.is_root {
        render_bottom_fill(buf, mx, by, mw, &status_text, st_w, rev, true);
    } else if let Some(ref s) = app.search.as_ref().filter(|s| !s.confirmed) {
        // 検索バーを底辺ボーダーに埋め込む（入力中のみ）
        let search_prefix = "/ ";
        let prefix_w = sw(search_prefix);
        let search_avail = (mw - 2).saturating_sub(st_w + 1);
        let cyan2 = Style::default().fg(Color::Cyan);
        let yellow2 = Style::default().fg(Color::Yellow);
        blit(buf, mx + 1, by, search_prefix, prefix_w, cyan2);
        let pat_disp_w = search_avail.saturating_sub(prefix_w);
        let scroll = if s.cursor >= pat_disp_w { s.cursor + 1 - pat_disp_w } else { 0 };
        blit(buf, mx + 1 + prefix_w as u16, by, &" ".repeat(pat_disp_w), pat_disp_w, yellow2);
        let pat_chars: Vec<char> = s.input.iter().copied().collect();
        for (i, &ch) in pat_chars[scroll..].iter().enumerate() {
            if i >= pat_disp_w { break; }
            let st = if scroll + i == s.cursor { rev } else { yellow2 };
            blit_ch(buf, mx + 1 + prefix_w as u16 + i as u16, by, ch, st);
        }
        if s.cursor == s.input.len() && s.cursor.saturating_sub(scroll) < pat_disp_w {
            blit_ch(buf, mx + 1 + prefix_w as u16 + (s.cursor - scroll) as u16, by, ' ', rev);
        }
        let sep_x = mx + 1 + search_avail as u16;
        blit_ch(buf, sep_x, by, '─', cyan);
        blit(buf, sep_x + 1, by, &status_text, st_w, rev);
    } else {
        let fill_n = (mw - 2).saturating_sub(st_w);
        blit(buf, mx + 1, by, &"─".repeat(fill_n), fill_n, cyan);
        blit(buf, mx + 1 + fill_n as u16, by, &status_text, st_w, rev);
    }
    blit_ch(buf, mx + mw as u16 - 1, by, '╯', cyan);

    render_dir_jump_dialog(frame, app);
    render_run_dialog(frame, app);
    render_func_dialog(frame, app);
    render_git_dialog(frame, app);
    render_git_running(frame, app);
    render_file_dialog(frame, app);
    render_sort_dialog(frame, app);
    render_error_msg(frame, app);
    render_success_msg(frame, app);
    render_help_overlay(frame, app);
}

// ── Menu bar ───────────────────────────────────────────────────────────

fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
    let key_style = Style::default().add_modifier(Modifier::REVERSED);
    let text_style = Style::default().fg(Color::Cyan);

    let spans: Vec<Span> = if app.fd_mode {
        static FD_MENU: &[(&str, &str)] = &[
            ("r", "更新 "),
            ("q/Esc", "プロセス "),
        ];
        static FD_MENU_EN: &[(&str, &str)] = &[
            ("r", "Refresh "),
            ("q/Esc", "Proc "),
        ];
        let items = if app.lang_en { FD_MENU_EN } else { FD_MENU };
        items.iter().flat_map(|(k, v)| {
            vec![Span::styled(*k, key_style), Span::styled(format!(":{} ", v), text_style)]
        }).collect()
    } else if app.proc_mode {
        static PROC_MENU: &[(&str, &str)] = &[
            ("x", "シグナル "),
            ("f/RET", "FD一覧 "),
            ("r", "更新 "),
            ("s", "ソート "),
            ("q/Esc", "ファイル "),
        ];
        static PROC_MENU_EN: &[(&str, &str)] = &[
            ("x", "Kill "),
            ("f/RET", "FD "),
            ("r", "Refresh "),
            ("s", "Sort "),
            ("q/Esc", "File "),
        ];
        let items = if app.lang_en { PROC_MENU_EN } else { PROC_MENU };
        items.iter().flat_map(|(k, v)| {
            vec![Span::styled(*k, key_style), Span::styled(format!(":{} ", v), text_style)]
        }).collect()
    } else {
        app.menu_items
            .iter()
            .flat_map(|(k, v)| {
                vec![
                    Span::styled(k.as_str(), key_style),
                    Span::styled(format!(":{} ", v), text_style),
                ]
            })
            .collect()
    };

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ── Info header ────────────────────────────────────────────────────────

fn render_header(frame: &mut Frame, main_area: Rect, inner: Rect, app: &App) {
    let mw = main_area.width as usize;
    let iw = inner.width as usize;
    if iw < LEFT_W + 20 {
        return;
    }

    let info_w = iw - LEFT_W - 1;

    let buf = frame.buffer_mut();
    let mx = main_area.x;
    let my = main_area.y;
    let ix = inner.x;
    let iy = inner.y;

    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let white = Style::default().fg(Color::White);
    let lc = app.ui_colors.label;
    let vc = white;
    let unit_st = app.ui_colors.unit;
    let date_st = app.ui_colors.date;
    let clock_st = app.ui_colors.clock;

    let sx1 = ix + LEFT_W as u16;

    // ── Top border ───────────────────────────────────────────────────────
    {
        let now = now_str();
        let clock_s = format!(" {} ", now);
        let vol_pre = "─ ";
        let vol_lbl = app.labels.vol_info;
        let vol_fill_n = LEFT_W.saturating_sub(sw(vol_pre) + sw(vol_lbl));
        let pas_pre = "─ ";
        let pas_lbl = app.labels.path_lbl;
        let pas_suf = " ";
        let ver_pre = "─── ";
        let ver_lbl = concat!("f5h v", env!("CARGO_PKG_VERSION"));
        let ver_suf = " ─";
        let fixed = LEFT_W
            + sw(pas_pre)
            + sw(pas_lbl)
            + sw(pas_suf)
            + sw(&clock_s)
            + sw(ver_pre)
            + sw(ver_lbl)
            + sw(ver_suf);
        let mid_fill_n = mw.saturating_sub(fixed + 2);

        let mut x = mx;
        blit_ch(buf, x, my, '╭', cyan);
        x += 1;
        blit(buf, x, my, vol_pre, sw(vol_pre), cyan);
        x += sw(vol_pre) as u16;
        blit(buf, x, my, vol_lbl, sw(vol_lbl), yellow);
        x += sw(vol_lbl) as u16;
        blit(buf, x, my, &"─".repeat(vol_fill_n), vol_fill_n, cyan);
        x += vol_fill_n as u16;
        blit(buf, x, my, pas_pre, sw(pas_pre), cyan);
        x += sw(pas_pre) as u16;
        blit(buf, x, my, pas_lbl, sw(pas_lbl), yellow);
        x += sw(pas_lbl) as u16;
        blit(buf, x, my, pas_suf, sw(pas_suf), cyan);
        x += sw(pas_suf) as u16;
        blit(buf, x, my, &"─".repeat(mid_fill_n), mid_fill_n, cyan);
        x += mid_fill_n as u16;
        blit(buf, x, my, &clock_s, sw(&clock_s), clock_st);
        x += sw(&clock_s) as u16;
        blit(buf, x, my, ver_pre, sw(ver_pre), cyan);
        x += sw(ver_pre) as u16;
        blit(buf, x, my, ver_lbl, sw(ver_lbl), yellow);
        x += sw(ver_lbl) as u16;
        blit(buf, x, my, ver_suf, sw(ver_suf), cyan);
        x += sw(ver_suf) as u16;
        blit_ch(buf, x, my, '╮', cyan);
        blit_ch(buf, sx1, my, '┬', cyan);
    }

    // ── iy+0: volume (left) │ path + git branch (right) ──────────────
    {
        let green = Style::default().fg(Color::Green);
        let vol = format!(" {}", trunc(&app.volume_info, LEFT_W - 1));
        blit(buf, ix, iy, &padr(&vol, LEFT_W), LEFT_W, green);
        blit_ch(buf, sx1, iy, '│', cyan);
        let branch_s = app
            .git_branch
            .as_ref()
            .map(|b| {
                let mut s = format!("  \u{E0A0} {}", b);
                match (app.git_ahead, app.git_behind) {
                    (0, 0) => {}
                    (a, 0) => s.push_str(&format!(" \u{2191}{}", a)),
                    (0, b) => s.push_str(&format!(" \u{2193}{}", b)),
                    (a, b) => s.push_str(&format!(" \u{2191}{}\u{2193}{}", a, b)),
                }
                s
            })
            .unwrap_or_default();
        let branch_w = sw(&branch_s);
        let path_avail = info_w.saturating_sub(branch_w);
        let path_s = format!(" {}", app.current_dir.to_string_lossy());
        blit(
            buf,
            sx1 + 1,
            iy,
            &padr(&trunc(&path_s, path_avail), path_avail),
            path_avail,
            white,
        );
        if branch_w > 0 {
            blit(
                buf,
                sx1 + 1 + path_avail as u16,
                iy,
                &branch_s,
                branch_w,
                yellow,
            );
        }
    }

    // helper: label + right-aligned numeric value + cyan unit + separator char
    let left_stat_cu = |buf: &mut Buffer, y: u16, lbl: &str, num: &str, unit: &str, sc: char| {
        let lw = sw(lbl);
        let nw = sw(num);
        let uw = sw(unit);
        let pad = LEFT_W.saturating_sub(lw + nw + uw);
        blit(buf, ix, y, lbl, lw, lc);
        if pad > 0 {
            blit(buf, ix + lw as u16, y, &" ".repeat(pad), pad, vc);
        }
        blit(buf, ix + (lw + pad) as u16, y, num, nw, vc);
        blit(buf, ix + (lw + pad + nw) as u16, y, unit, uw, unit_st);
        blit_ch(buf, sx1, y, sc, cyan);
    };

    // ── iy+1: 空き (left) │ ─ ファイル情報 ─ title ───────────────────
    {
        let (free_n, free_u) = fmt_cpt_parts(app.free_bytes);
        left_stat_cu(buf, iy + 1, app.labels.free, &free_n, free_u, '├');
        let m_pre = "─ ";
        let m_lbl = app.labels.file_info;
        let m_suf = " ";
        let m_fill = info_w.saturating_sub(sw(m_pre) + sw(m_lbl) + sw(m_suf));
        let mut xm = sx1 + 1;
        blit(buf, xm, iy + 1, m_pre, sw(m_pre), cyan);
        xm += sw(m_pre) as u16;
        blit(buf, xm, iy + 1, m_lbl, sw(m_lbl), yellow);
        xm += sw(m_lbl) as u16;
        blit(buf, xm, iy + 1, m_suf, sw(m_suf), cyan);
        xm += sw(m_suf) as u16;
        blit(buf, xm, iy + 1, &"─".repeat(m_fill), m_fill, cyan);
    }

    let cur = app.entries.get(app.cursor);

    // ── iy+2: カレント (left) │ ファイル: name ──────────────────────
    {
        let total = app.current_total_bytes();
        let n = app.file_count() + app.dir_count();
        let cnt_s = format!("{:>3}", n);
        let ll = app.labels.curr;
        let llw = sw(ll);
        let cs_w = sw(&cnt_s);
        let iw2 = sw(app.labels.count_unit);
        let bytes_raw = format!("{}", total);
        let bytes_field = LEFT_W.saturating_sub(llw + 1 + cs_w + iw2);
        let bytes_s = padl(&bytes_raw, bytes_field);
        let ts_w = sw(&bytes_s);
        blit(buf, ix, iy + 2, ll, llw, lc);
        blit(buf, ix + llw as u16, iy + 2, &bytes_s, ts_w, vc);
        blit(buf, ix + (llw + ts_w) as u16, iy + 2, " ", 1, vc);
        blit(buf, ix + (llw + ts_w + 1) as u16, iy + 2, &cnt_s, cs_w, vc);
        blit(
            buf,
            ix + (llw + ts_w + 1 + cs_w) as u16,
            iy + 2,
            app.labels.count_unit,
            iw2,
            lc,
        );

        blit_ch(buf, sx1, iy + 2, '│', cyan);
        let fname = cur.map(|e| e.name.as_str()).unwrap_or("");
        let fl = app.labels.file_lbl;
        let flw = sw(fl);
        blit(buf, sx1 + 1, iy + 2, fl, flw, lc);
        blit(
            buf,
            sx1 + 1 + flw as u16,
            iy + 2,
            &padr(
                &trunc(fname, info_w.saturating_sub(flw)),
                info_w.saturating_sub(flw),
            ),
            info_w.saturating_sub(flw),
            vc,
        );
    }

    // ── iy+3: 合計 (left) │ 種別: file type ─────────────────────────
    {
        let (tot_n, tot_u) = fmt_cpt_parts(app.total_bytes);
        left_stat_cu(buf, iy + 3, app.labels.total, &tot_n, tot_u, '│');
        let sl = app.labels.kind;
        let slw = sw(sl);
        blit(buf, sx1 + 1, iy + 3, sl, slw, lc);
        blit(
            buf,
            sx1 + 1 + slw as u16,
            iy + 3,
            &padr(
                &trunc(&app.file_type, info_w.saturating_sub(slw)),
                info_w.saturating_sub(slw),
            ),
            info_w.saturating_sub(slw),
            vc,
        );
    }

    // ── iy+4: 使用中 (left) │ サイズ/Blk/Inode/Links  [right: 修正/変更]
    {
        let (used_n, used_u) = fmt_cpt_parts(app.used_bytes);
        left_stat_cu(buf, iy + 4, app.labels.used, &used_n, used_u, '│');
        blit(buf, sx1 + 1, iy + 4, &" ".repeat(info_w), info_w, vc);

        let mtime = cur
            .map(|e| format!("{} {}", e.date, e.time_full))
            .unwrap_or_default();
        let ctime = cur.map(|e| e.ctime_s.clone()).unwrap_or_default();
        let ts4_parts = [
            (app.labels.mtime, mtime.as_str()),
            (app.labels.ctime, ctime.as_str()),
        ];
        let ts4_w: usize = ts4_parts.iter().map(|(l, v)| sw(l) + sw(v)).sum();
        let ts4_x = sx1 + 1 + (info_w.saturating_sub(ts4_w)) as u16;

        let mut x4 = sx1 + 1;
        let sz_l = app.labels.size_lbl;
        let sz_lw = sw(sz_l);
        let sz_v = cur
            .map(|e| {
                if e.is_dir {
                    "< DIR >".to_string()
                } else {
                    format!("{}", e.size)
                }
            })
            .unwrap_or_default();
        blit(buf, x4, iy + 4, sz_l, sz_lw, lc);
        x4 += sz_lw as u16;
        blit(buf, x4, iy + 4, &sz_v, sw(&sz_v), vc);
        x4 += sw(&sz_v) as u16;
        let blk_l = "  Blk:";
        let blk_lw = sw(blk_l);
        let blk_v = cur.map(|e| format!("{}", e.blocks)).unwrap_or_default();
        blit(buf, x4, iy + 4, blk_l, blk_lw, lc);
        x4 += blk_lw as u16;
        blit(buf, x4, iy + 4, &blk_v, sw(&blk_v), vc);
        x4 += sw(&blk_v) as u16;
        let ino_l = "  Inode:";
        let ino_lw = sw(ino_l);
        let ino_v = cur.map(|e| format!("{}", e.inode)).unwrap_or_default();
        blit(buf, x4, iy + 4, ino_l, ino_lw, lc);
        x4 += ino_lw as u16;
        blit(buf, x4, iy + 4, &ino_v, sw(&ino_v), vc);
        x4 += sw(&ino_v) as u16;
        let lnk_l = "  Links:";
        let lnk_lw = sw(lnk_l);
        let lnk_v = cur.map(|e| format!("{}", e.nlink)).unwrap_or_default();
        blit(buf, x4, iy + 4, lnk_l, lnk_lw, lc);
        x4 += lnk_lw as u16;
        blit(buf, x4, iy + 4, &lnk_v, sw(&lnk_v), vc);

        let mut xt = ts4_x;
        for (lbl, val) in &ts4_parts {
            blit(buf, xt, iy + 4, lbl, sw(lbl), lc);
            xt += sw(lbl) as u16;
            blit(buf, xt, iy + 4, val, sw(val), date_st);
            xt += sw(val) as u16;
        }
    }

    // ── iy+5: 使用率 (left) │ 権限/所有  [right: 作成/参照] ────────────
    {
        let pct = if app.total_bytes > 0 {
            app.used_bytes * 100 / app.total_bytes
        } else {
            0
        };
        left_stat_cu(
            buf,
            iy + 5,
            app.labels.usage,
            &format!("{}", pct),
            " %",
            '│',
        );
        blit(buf, sx1 + 1, iy + 5, &" ".repeat(info_w), info_w, vc);

        let birth = cur
            .and_then(|e| e.birth_s.as_deref())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "--".to_string());
        let atime = cur.map(|e| e.atime_s.clone()).unwrap_or_default();
        let ts5_parts = [
            (app.labels.birth, birth.as_str()),
            (app.labels.atime, atime.as_str()),
        ];
        let ts5_w: usize = ts5_parts.iter().map(|(l, v)| sw(l) + sw(v)).sum();
        let ts5_x = sx1 + 1 + (info_w.saturating_sub(ts5_w)) as u16;

        let mut x5 = sx1 + 1;
        let mode_s = cur
            .map(|e| fmt_mode(e.mode, e.is_dir, e.is_link))
            .unwrap_or_else(|| "----------".to_string());
        let ml = app.labels.perm;
        let mlw = sw(ml);
        blit(buf, x5, iy + 5, ml, mlw, lc);
        x5 += mlw as u16;
        blit(buf, x5, iy + 5, &mode_s, sw(&mode_s), vc);
        x5 += sw(&mode_s) as u16;
        let ol = app.labels.own;
        let olw = sw(ol);
        blit(buf, x5, iy + 5, ol, olw, lc);
        x5 += olw as u16;
        blit(buf, x5, iy + 5, &app.owner_s, sw(&app.owner_s), vc);

        let mut xt = ts5_x;
        for (lbl, val) in &ts5_parts {
            blit(buf, xt, iy + 5, lbl, sw(lbl), lc);
            xt += sw(lbl) as u16;
            blit(buf, xt, iy + 5, val, sw(val), date_st);
            xt += sw(val) as u16;
        }
    }

    // ── iy+6: bottom separator ─┴─ ─────────────────────────────────────
    {
        let mut chars: Vec<char> = "─".repeat(iw).chars().collect();
        if LEFT_W < iw {
            chars[LEFT_W] = '┴';
        }
        blit(buf, ix, iy + 6, &chars.iter().collect::<String>(), iw, cyan);
    }
}

// ── Dir jump dialog ─────────────────────────────────────────────────────

fn render_dir_jump_dialog(frame: &mut Frame, app: &App) {
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

fn render_run_dialog(frame: &mut Frame, app: &App) {
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

fn render_func_dialog(frame: &mut Frame, app: &App) {
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

fn render_git_dialog(frame: &mut Frame, app: &App) {
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

fn render_git_running(frame: &mut Frame, app: &App) {
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

fn render_file_dialog(frame: &mut Frame, app: &App) {
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

fn render_error_msg(frame: &mut Frame, app: &App) {
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

fn render_sort_dialog(frame: &mut Frame, app: &App) {
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

fn render_success_msg(frame: &mut Frame, app: &App) {
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

fn render_help_overlay(frame: &mut Frame, app: &App) {
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

fn render_proc_help_overlay(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let dw = (area.width as usize).min(60);
    let iw = dw - 2;

    let entries: &[(&str, &str, &str)] = &[
        ("", "── プロセス一覧 ──", "── Process List ──"),
        ("j/k",       "上/下に移動",       "Move up/down"),
        ("g/G",       "先頭/末尾へ",       "First/last entry"),
        ("PageUp/Dn", "前/次ページ",       "Previous/next page"),
        ("s",         "ソート切替",         "Cycle sort mode"),
        ("x",         "シグナル送信メニュー", "Signal menu"),
        ("f/Enter",   "FD一覧を開く",      "Open FD list"),
        ("r",         "プロセス一覧を更新", "Refresh list"),
        ("q/Esc",     "ファイルモードへ",   "Return to file mode"),
        ("", "── FD一覧 ──", "── FD List ──"),
        ("j/k",       "上/下に移動",       "Move up/down"),
        ("r",         "FD一覧を更新",      "Refresh FD list"),
        ("q/Esc",     "プロセス一覧へ",    "Return to proc list"),
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

// ── Tree pane ──────────────────────────────────────────────────────────

fn render_tree(frame: &mut Frame, area: Rect, app: &App) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let buf = frame.buffer_mut();
    let aw = area.width as usize;
    let ah = area.height as usize;
    for row in 0..ah {
        let idx = app.tree_offset + row;
        let y = area.y + row as u16;
        if idx >= app.tree_nodes.len() {
            blit(buf, area.x, y, &" ".repeat(aw), aw, Style::default());
            continue;
        }
        let node = &app.tree_nodes[idx];
        let is_cur = idx == app.tree_cursor;
        let icon = if !node.has_children {
            "  "
        } else if node.expanded {
            "▼ "
        } else {
            "▶ "
        };
        let indent = "  ".repeat(node.depth);
        let text = if node.depth == 0 {
            format!("{}{}{}", indent, icon, node.name)
        } else {
            format!("{}{}{}/", indent, icon, node.name)
        };
        let st = if is_cur && app.tree_focus {
            Style::default().add_modifier(Modifier::REVERSED)
        } else if is_cur {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        blit(buf, area.x, y, &padr(&text, aw), aw, st);
    }
}

// ── File list ──────────────────────────────────────────────────────────

fn render_file_list(frame: &mut Frame, area: Rect, app: &App, lh: usize) {
    if area.height == 0 || area.width == 0 || lh == 0 {
        return;
    }
    let cols = app.cols();
    let pp = cols * lh;
    let pstart = app.current_page(lh) * pp;
    let total_w = area.width as usize;

    const COL_GAP: usize = 3;
    let col_w = total_w.saturating_sub(COL_GAP * (cols - 1)) / cols;
    let buf = frame.buffer_mut();

    // 検索中: マッチインデックスセットを構築
    let match_set: std::collections::HashSet<usize> = app
        .search
        .as_ref()
        .map(|s| s.matches.iter().copied().collect())
        .unwrap_or_default();
    let search_active = app.search.is_some();

    for col in 0..cols {
        let col_x = area.x + (col * (col_w + COL_GAP)) as u16;
        let cw = if col + 1 == cols {
            total_w.saturating_sub(col * (col_w + COL_GAP))
        } else {
            col_w
        };

        for row in 0..lh {
            let idx = pstart + col * lh + row;
            if idx >= app.entries.len() {
                break;
            }
            let e = &app.entries[idx];
            let is_cur = idx == app.cursor;
            let is_tagged = app.tagged.get(idx).copied().unwrap_or(false);
            let is_match = search_active && match_set.contains(&idx);

            let base_style = ls_style(e, &app.ls_colors);
            let file_style = if is_cur {
                base_style.add_modifier(Modifier::REVERSED)
            } else if is_match {
                // 検索マッチ: 黄緑でアンダーライン
                Style::default().fg(Color::LightGreen).add_modifier(Modifier::UNDERLINED)
            } else if is_tagged {
                Style::default().fg(Color::Yellow)
            } else {
                base_style
            };

            let y = area.y + row as u16;
            if y >= area.y + area.height {
                break;
            }

            const PFX: usize = 3;
            let tag_ch = if is_tagged { '*' } else { ' ' };
            let git_ch = app.git_status.get(&e.name).copied().unwrap_or(' ');
            let git_base = match git_ch {
                'M' | 'm' => Style::default().fg(Color::Yellow),
                'A' => Style::default().fg(Color::Green),
                '?' | 'D' => Style::default().fg(Color::Red),
                _ => Style::default(),
            };
            let tag_st = if is_tagged {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            blit_ch(buf, col_x, y, tag_ch, tag_st);
            blit_ch(buf, col_x + 1, y, git_ch, git_base);
            blit_ch(buf, col_x + 2, y, ' ', Style::default());

            let entry_w = cw.saturating_sub(PFX);
            blit(
                buf,
                col_x + PFX as u16,
                y,
                &padr(&fmt_entry(e, entry_w, app.col_mode), entry_w),
                entry_w,
                file_style,
            );
        }
    }
}

// SZ_W: raw bytes up to 12 digits  DT_W: YYYY-MM-DD HH:MM = 16 chars
const SZ_W: usize = 12;
const DT_W: usize = 16;

fn fmt_entry(e: &FileEntry, cw: usize, col_mode: u8) -> String {
    let name_disp = if e.is_dir && e.name != ".." {
        format!("{}/", e.name)
    } else {
        e.name.clone()
    };
    let size_s = if e.is_dir {
        padl("<DIR>", SZ_W)
    } else {
        padl(&fmt_size(e.size), SZ_W)
    };
    let dt_s = if e.date.is_empty() {
        String::new()
    } else {
        format!("{} {}", e.date, e.time_str)
    };

    match col_mode {
        1 | 2 => {
            let name_w = cw.saturating_sub(1 + SZ_W + 1 + DT_W).max(3);
            if dt_s.is_empty() {
                format!("{} {}", padr(&name_disp, name_w), size_s)
            } else {
                format!("{} {} {}", padr(&name_disp, name_w), size_s, dt_s)
            }
        }
        3 => {
            let name_w = cw.saturating_sub(1 + SZ_W).max(3);
            format!("{} {}", padr(&name_disp, name_w), size_s)
        }
        _ => padr(&name_disp, cw),
    }
}

// ── Proc mode ──────────────────────────────────────────────────────────

/// proc モードのメインビュー（7行ヘッダー + プロセスリスト）
fn render_proc_view(frame: &mut Frame, main_area: Rect, app: &App) {
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
        let now     = now_str();
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

    // ── iy+0: 負荷(green keep) / PID・PPID・TTY・ユーザ・状態 ───────────
    {
        let load_lbl = if app.lang_en { " Load" } else { " 負荷" };
        let load_s = format!("{} {:.2} {:.2} {:.2}", load_lbl, sys.load_1, sys.load_5, sys.load_15);
        blit(buf, ix, iy, &padr(&load_s, lw), lw, green);

        let (ppid_lbl, tty_lbl, user_lbl, stat_lbl) = if app.lang_en {
            (" PPID:", " TTY:", " User:", " Stat:")
        } else {
            (" PPID:", " TTY:", " ユーザ:", " 状態:")
        };
        let pid_s  = padr(&format!("{}", det.pid), 6);
        let ppid_s = format!("{}", det.ppid);
        let tty_s  = det.tty.clone();
        let user_s = padr(trunc(&det.user, 10).as_str(), 11);
        let stat_s = trunc(&det.stat, 4);
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

    // ── カラムヘッダー (my + HEADER_ROWS + 1) ─────────────────────────────
    let col_hdr_y = my + HEADER_ROWS + 1;
    {
        blit_ch(buf, mx, col_hdr_y, '│', cyan);
        let sort_lbl = app.proc_sort.label();
        let sort_arrow = if app.proc_sort_asc { "▲" } else { "▼" };
        let user_hdr = if app.lang_en { "USER" } else { "ユーザ" };
        let cmd_hdr  = if app.lang_en { "COMMAND" } else { "コマンド" };
        let hdr = format!(
            " {} {} {:>5} {:>5} {:>7} {:>6} {:2} {}",
            padl("PID", 6), padr(user_hdr, 10),
            "%CPU", "%MEM", "VSZ", "RSS", "S", cmd_hdr
        );
        blit(buf, mx + 1, col_hdr_y, &padr(&hdr, iw), iw, lc);
        let sort_ind = format!("[{}{}]", sort_lbl, sort_arrow);
        let si_x = mx + mw as u16 - 1 - sw(&sort_ind) as u16 - 1;
        blit(buf, si_x, col_hdr_y, &sort_ind, sw(&sort_ind), yellow);
        blit_ch(buf, mx + mw as u16 - 1, col_hdr_y, '│', cyan);
    }

    // ── プロセスリスト ────────────────────────────────────────────────────
    let list_start_y = my + HEADER_ROWS + 2;
    let list_h = (my + mh - 1).saturating_sub(list_start_y) as usize;
    let n = app.proc_entries.len();
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
            blit(buf, mx + 1, screen_y, &" ".repeat(iw), iw, Style::default());
        } else {
            let e = &app.proc_entries[entry_idx];
            let is_cur = entry_idx == app.proc_cursor;
            let (vsz_n, vsz_u) = fmt_cpt_parts(e.vsz * 1024);
            let (rss_n, rss_u) = fmt_cpt_parts(e.rss * 1024);
            let cmd_avail = iw.saturating_sub(40);
            let line = format!(
                " {:>6} {:<10} {:>5.1} {:>5.1} {:>7} {:>6} {:<2} {}",
                e.pid, trunc(&e.user, 10), e.cpu, e.mem,
                padl(&format!("{}{}", vsz_n, vsz_u), 7),
                padl(&format!("{}{}", rss_n, rss_u), 6),
                trunc(&e.stat, 2), trunc(&e.command, cmd_avail),
            );
            let style = if is_cur {
                rev
            } else if e.cpu >= 50.0 {
                Style::default().fg(Color::Red)
            } else {
                match e.stat.chars().next().unwrap_or(' ') {
                    'R' => Style::default().fg(Color::Yellow),
                    'T' => Style::default().fg(Color::Cyan),
                    'D' => Style::default().fg(Color::Green),
                    'Z' => Style::default().fg(Color::Magenta),
                    _ => white,
                }
            };
            blit(buf, mx + 1, screen_y, &padr(&line, iw), iw, style);
        }
        blit_ch(buf, mx + mw as u16 - 1, screen_y, '│', cyan);
    }

    // 余白行
    for row_i in (list_h + list_start_y as usize)..(my as usize + mh as usize - 1) {
        let screen_y = row_i as u16;
        blit_ch(buf, mx, screen_y, '│', cyan);
        blit(buf, mx + 1, screen_y, &" ".repeat(iw), iw, Style::default());
        blit_ch(buf, mx + mw as u16 - 1, screen_y, '│', cyan);
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
        render_bottom_fill(buf, mx, by, mw, &status, st_w, rev, app.is_root);
        blit_ch(buf, mx + mw as u16 - 1, by, '╯', cyan);
    }
}

/// fd 一覧ビュー
fn render_fd_view(frame: &mut Frame, main_area: Rect, app: &App) {
    let mw = main_area.width as usize;
    let mh = main_area.height;
    let mx = main_area.x;
    let my = main_area.y;
    if mw < 30 || mh < 6 { return; }

    let iw = mw - 2;
    let buf = frame.buffer_mut();
    let cyan  = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let lc    = app.ui_colors.label;
    let white = Style::default().fg(Color::White);
    let rev   = Style::default().add_modifier(Modifier::REVERSED);

    // ── 上枠 ──────────────────────────────────────────────────────────────
    {
        let title = format!(" FD: {} (PID {}) ", app.fd_proc_name, app.fd_pid);
        let n_fds = app.fd_entries.len();
        let count_unit = if app.lang_en { "fds" } else { "個" };
        let count_s = format!(" {} {} ", n_fds, count_unit);
        let clock_s = format!(" {} ", now_str());
        let ver_pre = "─── ";
        let ver_lbl = concat!("f5h v", env!("CARGO_PKG_VERSION"));
        let ver_suf = " ─";
        let fixed = 1 + sw(&title) + 4 + sw(&count_s)
            + sw(&clock_s) + sw(ver_pre) + sw(ver_lbl) + sw(ver_suf) + 1;
        let mid_fill_n = mw.saturating_sub(fixed);

        let mut x = mx;
        blit_ch(buf, x, my, '╭', cyan); x += 1;
        blit(buf, x, my, &title, sw(&title), yellow); x += sw(&title) as u16;
        blit(buf, x, my, "─── ", 4, cyan); x += 4;
        blit(buf, x, my, &count_s, sw(&count_s), lc); x += sw(&count_s) as u16;
        blit(buf, x, my, &"─".repeat(mid_fill_n), mid_fill_n, cyan); x += mid_fill_n as u16;
        blit(buf, x, my, &clock_s, sw(&clock_s), app.ui_colors.clock); x += sw(&clock_s) as u16;
        blit(buf, x, my, ver_pre, sw(ver_pre), cyan); x += sw(ver_pre) as u16;
        blit(buf, x, my, ver_lbl, sw(ver_lbl), yellow); x += sw(ver_lbl) as u16;
        blit(buf, x, my, ver_suf, sw(ver_suf), cyan); x += sw(ver_suf) as u16;
        blit_ch(buf, x, my, '╮', cyan);
    }

    // ── カラムヘッダー ────────────────────────────────────────────────────
    {
        blit_ch(buf, mx, my + 1, '│', cyan);
        let type_hdr   = if app.lang_en { "TYPE" } else { "種別" };
        let target_hdr = if app.lang_en { "TARGET" } else { "対象" };
        let hdr = format!(" {:>4}  {}  {}", "FD", padr(type_hdr, 6), target_hdr);
        blit(buf, mx + 1, my + 1, &padr(&hdr, iw), iw, lc);
        blit_ch(buf, mx + mw as u16 - 1, my + 1, '│', cyan);
    }

    // ── セパレータ ────────────────────────────────────────────────────────
    blit_ch(buf, mx, my + 2, '├', cyan);
    blit(buf, mx + 1, my + 2, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, mx + mw as u16 - 1, my + 2, '┤', cyan);

    // ── fd 一覧 ───────────────────────────────────────────────────────────
    let list_start_y = my + 3;
    let list_h = (my + mh - 1).saturating_sub(list_start_y) as usize;
    let red_style = Style::default().fg(Color::Red);
    let n = app.fd_entries.len();
    let offset = if app.fd_cursor < app.fd_offset {
        app.fd_cursor
    } else if n > 0 && app.fd_cursor >= app.fd_offset + list_h {
        app.fd_cursor + 1 - list_h
    } else {
        app.fd_offset
    };

    if let Some(ref err_msg) = app.fd_error {
        // パーミッションエラー等を表示
        for row_i in 0..list_h {
            let screen_y = list_start_y + row_i as u16;
            if screen_y >= my + mh - 1 { break; }
            blit_ch(buf, mx, screen_y, '│', cyan);
            if row_i == list_h / 2 {
                let msg = format!(" ⚠ {}", err_msg);
                blit(buf, mx + 1, screen_y, &padr(&msg, iw), iw, red_style);
            } else {
                blit(buf, mx + 1, screen_y, &" ".repeat(iw), iw, Style::default());
            }
            blit_ch(buf, mx + mw as u16 - 1, screen_y, '│', cyan);
        }
    } else {
        for row_i in 0..list_h {
            let screen_y = list_start_y + row_i as u16;
            if screen_y >= my + mh - 1 { break; }
            blit_ch(buf, mx, screen_y, '│', cyan);
            let entry_idx = offset + row_i;
            if entry_idx >= n {
                blit(buf, mx + 1, screen_y, &" ".repeat(iw), iw, Style::default());
            } else {
                let e = &app.fd_entries[entry_idx];
                let is_cur = entry_idx == app.fd_cursor;
                let target_avail = iw.saturating_sub(14);
                let line = format!(
                    " {:>4}  {:<6}  {}",
                    e.fd, e.fd_type.tag(), trunc(&e.target, target_avail)
                );
                let style = if is_cur { rev } else { white };
                blit(buf, mx + 1, screen_y, &padr(&line, iw), iw, style);
            }
            blit_ch(buf, mx + mw as u16 - 1, screen_y, '│', cyan);
        }
    }

    // ── 下枠 ─────────────────────────────────────────────────────────────
    {
        let by = my + mh - 1;
        let page = if list_h > 0 { offset / list_h + 1 } else { 1 };
        let total = if list_h > 0 { (n + list_h - 1).max(1) / list_h } else { 1 };
        let f1_lbl = if app.lang_en { "F1:Help" } else { "F1:ヘルプ" };
        let status = format!(" {}  {:>3}/{:<3} {}", f1_lbl, page, total, app.labels.page_unit);
        let st_w = sw(&status);
        blit_ch(buf, mx, by, '╰', cyan);
        render_bottom_fill(buf, mx, by, mw, &status, st_w, rev, app.is_root);
        blit_ch(buf, mx + mw as u16 - 1, by, '╯', cyan);
    }
}

/// ボトムボーダーの内側を描画する共通ヘルパー。
/// is_root=true のとき赤黒縞模様を敷き、右端に status を REVERSED で重ねる。
/// is_root=false のとき通常の罫線 + status を描画する。
fn render_bottom_fill(
    buf: &mut ratatui::buffer::Buffer,
    mx: u16, by: u16, mw: usize,
    status: &str, st_w: usize,
    rev: Style, is_root: bool,
) {
    let inner_w = mw.saturating_sub(2);
    if is_root {
        let warn_lbl = " !! ROOT !!";
        let warn_w = sw(warn_lbl);
        let stripe_red = Style::default().fg(Color::White).bg(Color::Red)
            .add_modifier(Modifier::SLOW_BLINK | Modifier::BOLD);
        let stripe_blk = Style::default().fg(Color::Red).bg(Color::Black)
            .add_modifier(Modifier::SLOW_BLINK | Modifier::BOLD);
        let mut xi = mx + 1;
        let mut col = 0usize;
        while col < inner_w {
            let remaining = inner_w - col;
            let chunk = warn_w.min(remaining);
            let st = if (col / warn_w) % 2 == 0 { stripe_red } else { stripe_blk };
            blit(buf, xi, by, warn_lbl, chunk, st);
            xi += chunk as u16;
            col += chunk;
        }
        let status_x = mx + 1 + inner_w.saturating_sub(st_w) as u16;
        blit(buf, status_x, by, status, st_w, rev);
    } else {
        let fill_n = inner_w.saturating_sub(st_w);
        let cyan = Style::default().fg(Color::Cyan);
        blit(buf, mx + 1, by, &"─".repeat(fill_n), fill_n, cyan);
        blit(buf, mx + 1 + fill_n as u16, by, status, st_w, rev);
    }
}

/// シグナルメニューオーバーレイ
fn render_proc_signal_menu(frame: &mut Frame, app: &App) {
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

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── sw: display width ──────────────────────────────────────────────

    #[test]
    fn test_sw_ascii() {
        assert_eq!(sw("hello"), 5);
        assert_eq!(sw(""), 0);
        assert_eq!(sw("a"), 1);
    }

    #[test]
    fn test_sw_cjk_double_width() {
        assert_eq!(sw("日本語"), 6); // 3文字 × 2幅
        assert_eq!(sw("A日B"), 4); // 1+2+1
    }

    #[test]
    fn test_sw_mixed() {
        assert_eq!(sw("abc日本"), 7); // 3+2+2
    }

    // ── trunc ──────────────────────────────────────────────────────────

    #[test]
    fn test_trunc_under_limit() {
        assert_eq!(trunc("hello", 10), "hello");
    }

    #[test]
    fn test_trunc_exact_limit() {
        assert_eq!(trunc("hello", 5), "hello");
    }

    #[test]
    fn test_trunc_over_limit() {
        // ASCII は各文字1幅でmax_colsがちょうど埋まるため~を挿入する余地なし
        assert_eq!(trunc("hello world", 5), "hello"); // ASCII: ~を入れる余地なし
    }

    #[test]
    fn test_trunc_cjk_fits_exactly() {
        // "日本" = 4 display width; max=4 → no truncation
        assert_eq!(trunc("日本語", 4), "日本");
    }

    #[test]
    fn test_trunc_cjk_with_tilde() {
        // "日本" (4) + "~" (1) = 5
        assert_eq!(trunc("日本語", 5), "日本~");
    }

    #[test]
    fn test_trunc_cjk_odd_width() {
        // max=3: "日" (2) fits, "本" (2) would overflow → "日" + "~" but 2+1=3 ✓
        assert_eq!(trunc("日本語", 3), "日~");
    }

    // ── padr ───────────────────────────────────────────────────────────

    #[test]
    fn test_padr_pads_right() {
        assert_eq!(padr("hi", 5), "hi   ");
    }

    #[test]
    fn test_padr_exact() {
        assert_eq!(padr("hello", 5), "hello");
    }

    #[test]
    fn test_padr_truncates() {
        assert_eq!(padr("hello world", 5), "hello");
    }

    #[test]
    fn test_padr_cjk() {
        // "日" = 2 width; padded to 5 → "日   " (3 spaces)
        assert_eq!(padr("日", 5), "日   ");
    }

    // ── padl ───────────────────────────────────────────────────────────

    #[test]
    fn test_padl_pads_left() {
        assert_eq!(padl("hi", 5), "   hi");
    }

    #[test]
    fn test_padl_exact() {
        assert_eq!(padl("hello", 5), "hello");
    }

    #[test]
    fn test_padl_truncates() {
        assert_eq!(padl("hello world", 5), "hello");
    }

    // ── fmt_mode ───────────────────────────────────────────────────────

    #[test]
    fn test_fmt_mode_regular_rw() {
        assert_eq!(fmt_mode(0o644, false, false), "-rw-r--r--");
    }

    #[test]
    fn test_fmt_mode_executable() {
        assert_eq!(fmt_mode(0o755, false, false), "-rwxr-xr-x");
    }

    #[test]
    fn test_fmt_mode_directory() {
        assert_eq!(fmt_mode(0o755, true, false), "drwxr-xr-x");
    }

    #[test]
    fn test_fmt_mode_symlink() {
        assert_eq!(fmt_mode(0o777, false, true), "lrwxrwxrwx");
    }

    #[test]
    fn test_fmt_mode_no_permissions() {
        assert_eq!(fmt_mode(0o000, false, false), "----------");
    }

    #[test]
    fn test_fmt_mode_all_permissions() {
        assert_eq!(fmt_mode(0o777, false, false), "-rwxrwxrwx");
    }

    // ── fmt_cpt_parts ─────────────────────────────────────────────────

    #[test]
    fn test_fmt_cpt_parts_bytes() {
        let (n, u) = fmt_cpt_parts(512);
        assert_eq!(u, "B");
        assert_eq!(n, "512");
    }

    #[test]
    fn test_fmt_cpt_parts_boundary_ki() {
        let (_, u) = fmt_cpt_parts(1024);
        assert_eq!(u, "Ki");
    }

    #[test]
    fn test_fmt_cpt_parts_ki() {
        let (n, u) = fmt_cpt_parts(2 * 1024);
        assert_eq!(u, "Ki");
        assert_eq!(n, "2");
    }

    #[test]
    fn test_fmt_cpt_parts_mi() {
        let (n, u) = fmt_cpt_parts(5 * 1024 * 1024);
        assert_eq!(u, "Mi");
        assert_eq!(n, "5");
    }

    #[test]
    fn test_fmt_cpt_parts_gi() {
        let (n, u) = fmt_cpt_parts(10 * 1024 * 1024 * 1024);
        assert_eq!(u, "Gi");
        assert_eq!(n, "10");
    }

    // ── fmt_size ───────────────────────────────────────────────────────

    #[test]
    fn test_fmt_size_small() {
        assert_eq!(fmt_size(0), "0");
        assert_eq!(fmt_size(1234), "1234");
    }

    #[test]
    fn test_fmt_size_tb() {
        let tb = 2_000_000_000_000u64;
        assert_eq!(fmt_size(tb), "2.0TB");
    }

    // ── clear_rect ─────────────────────────────────────────────────────

    #[test]
    fn test_clear_rect_fills_interior_with_spaces() {
        use ratatui::prelude::{Buffer, Position, Rect};
        let mut buf = Buffer::empty(Rect { x: 0, y: 0, width: 10, height: 5 });
        // バッファ全体を 'X' で埋める
        for y in 0..5u16 {
            for x in 0..10u16 {
                buf.cell_mut(Position::new(x, y)).unwrap().set_char('X');
            }
        }
        // (2, 1) から幅 6、高さ 2 をクリア
        clear_rect(&mut buf, 2, 1, 6, 2);
        // クリア範囲は空白になっている
        for row in 1..3u16 {
            for col in 2..8u16 {
                assert_eq!(
                    buf.cell(Position::new(col, row)).unwrap().symbol(),
                    " ",
                    "cell ({col},{row}) should be space"
                );
            }
        }
        // クリア範囲外は 'X' のまま
        assert_eq!(buf.cell(Position::new(0, 0)).unwrap().symbol(), "X");
        assert_eq!(buf.cell(Position::new(9, 4)).unwrap().symbol(), "X");
        assert_eq!(buf.cell(Position::new(1, 1)).unwrap().symbol(), "X"); // クリア範囲の左外
        assert_eq!(buf.cell(Position::new(8, 2)).unwrap().symbol(), "X"); // クリア範囲の右外
    }

    #[test]
    fn test_clear_rect_zero_height_is_noop() {
        use ratatui::prelude::{Buffer, Position, Rect};
        let mut buf = Buffer::empty(Rect { x: 0, y: 0, width: 5, height: 3 });
        for y in 0..3u16 {
            for x in 0..5u16 {
                buf.cell_mut(Position::new(x, y)).unwrap().set_char('X');
            }
        }
        clear_rect(&mut buf, 0, 0, 5, 0); // 高さ 0 → 何もしない
        assert_eq!(buf.cell(Position::new(0, 0)).unwrap().symbol(), "X");
    }
}
