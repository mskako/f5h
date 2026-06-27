use ratatui::{prelude::*, widgets::Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, FileEntry, SortMode};
use crate::fs_utils::now_str;

mod dialogs;
mod proc_view;

use dialogs::*;
use proc_view::*;

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
pub fn clear_rect(buf: &mut Buffer, x: u16, y: u16, w: usize, h: usize) {
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
            ("t", "ツリー "),
            ("x", "シグナル "),
            ("f/RET", "FD一覧 "),
            ("r", "更新 "),
            ("s", "ソート "),
            ("q/Esc", "ファイル "),
        ];
        static PROC_MENU_EN: &[(&str, &str)] = &[
            ("t", "Tree "),
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

/// ボトムボーダーの内側を描画する共通ヘルパー。
/// is_root=true のとき赤黒縞模様を敷き、右端に status を REVERSED で重ねる。
/// is_root=false のとき通常の罫線 + status を描画する。
pub fn render_bottom_fill(
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
