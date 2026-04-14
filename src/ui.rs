use ratatui::{prelude::*, widgets::Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{App, DialogKind, FileEntry, GitDialogState, MacroDialog, RemoteOp};
use crate::fs_utils::now_str;

// ── Layout constants ───────────────────────────────────────────────────

/// Left section (ボリューム情報) fixed display width
pub const LEFT_W: usize = 28;
/// Tree pane width (when tree is open)
pub const TREE_W: usize = 22;
/// Inner rows occupied by the info header (before the file list)
pub const HEADER_ROWS: u16 = 7;

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
        let tw = TREE_W.min(list_area.width as usize / 2);
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

    // Bottom border with embedded reversed status text (original FILMTN style)
    let by = my + mh - 1;
    let page = app.current_page(lh) + 1;
    let total = app.total_pages(lh);
    let status_text = format!(
        "{}  {:>3}/{:<3} {}",
        app.labels.f1_help, page, total, app.labels.page_unit
    );
    let st_w = sw(&status_text);
    let fill_n = (mw - 2).saturating_sub(st_w);
    let rev = Style::default().add_modifier(Modifier::REVERSED);
    blit_ch(buf, mx, by, '╰', cyan);
    blit(buf, mx + 1, by, &"─".repeat(fill_n), fill_n, cyan);
    blit(buf, mx + 1 + fill_n as u16, by, &status_text, st_w, rev);
    blit_ch(buf, mx + mw as u16 - 1, by, '╯', cyan);

    render_run_dialog(frame, app);
    render_macro_dialog(frame, app);
    render_git_dialog(frame, app);
    render_git_running(frame, app);
    render_file_dialog(frame, app);
    render_error_msg(frame, app);
}

// ── Menu bar ───────────────────────────────────────────────────────────

fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
    let key_style = Style::default().add_modifier(Modifier::REVERSED);
    let text_style = Style::default().fg(Color::Cyan);
    let spans: Vec<Span> = app
        .menu_items
        .iter()
        .flat_map(|(k, v)| {
            vec![
                Span::styled(k.as_str(), key_style),
                Span::styled(format!(":{} ", v), text_style),
            ]
        })
        .collect();
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
        let ver_lbl = "f5h v0.1";
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
            .map(|b| format!("  \u{E0A0} {}", b))
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

// ── Macro command dialog ───────────────────────────────────────────────

fn render_macro_dialog(frame: &mut Frame, app: &App) {
    let dlg: &MacroDialog = match app.macro_dialog.as_ref() {
        Some(d) => d,
        None => return,
    };
    let area = frame.area();
    let dw = (area.width as usize).clamp(30, 50);
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = area.height / 2 - 2;

    let buf = frame.buffer_mut();
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;
    let dim = Style::default().fg(Color::DarkGray);
    let white = Style::default().fg(Color::White);
    let rev = Style::default().add_modifier(Modifier::REVERSED);

    let title = if app.lang_en { " Macro " } else { " マクロ " };
    let hint = if app.lang_en {
        "  q:Quit  Esc:Cancel"
    } else {
        "  q:終了  Esc:キャンセル"
    };
    let prompt = ":";
    let pw = sw(prompt);
    let iw = dw - 2;

    // Top border
    let fill_n = iw.saturating_sub(sw(title));
    blit_ch(buf, dx, dy, '╭', cyan);
    blit(buf, dx + 1, dy, title, sw(title), yellow);
    blit(buf, dx + 1 + sw(title) as u16, dy, &"─".repeat(fill_n), fill_n, cyan);
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

    // Input row
    let input_x = dx + 1 + pw as u16;
    let input_w = iw - pw;
    blit_ch(buf, dx, dy + 1, '│', cyan);
    blit(buf, dx + 1, dy + 1, prompt, pw, cyan);
    let scroll = if dlg.cursor >= input_w { dlg.cursor + 1 - input_w } else { 0 };
    blit(buf, input_x, dy + 1, &" ".repeat(input_w), input_w, white);
    for (i, &ch) in dlg.input[scroll..].iter().enumerate() {
        if i >= input_w { break; }
        let st = if scroll + i == dlg.cursor { rev } else { white };
        blit_ch(buf, input_x + i as u16, dy + 1, ch, st);
    }
    if dlg.cursor == dlg.input.len() && dlg.cursor - scroll < input_w {
        blit_ch(buf, input_x + (dlg.cursor - scroll) as u16, dy + 1, ' ', rev);
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
                    ("P", "pull", "fetch + fast-forward"),
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
                    ("P", "pull", "fetch + fast-forward"),
                    ("s", "switch", "ブランチ切替"),
                    ("t", "stash", "作業変更を退避（メッセージ任意）"),
                    ("T", "stash pop", "最新 stash を復元"),
                ]
            };
            let hint = if app.lang_en { "  Esc:Cancel" } else { "  Esc:キャンセル" };
            let total_rows = rows.len() + 2; // top + rows + hint + bottom

            blit_ch(buf, dx, dy, '╭', cyan);
            blit(buf, dx + 1, dy, title, sw(title), yellow);
            blit(buf, dx + 1 + sw(title) as u16, dy, &"─".repeat(iw.saturating_sub(sw(title))), iw.saturating_sub(sw(title)), cyan);
            blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

            for (i, (key, cmd, desc)) in rows.iter().enumerate() {
                let y = dy + 1 + i as u16;
                blit_ch(buf, dx, y, '│', cyan);
                blit(buf, dx + 1, y, key, 1, green);
                blit(buf, dx + 2, y, "  ", 2, white);
                let cmd_w = 9;
                blit(buf, dx + 4, y, &padr(cmd, cmd_w), cmd_w, white);
                blit(buf, dx + 4 + cmd_w as u16, y, &trunc(desc, iw - 4 - cmd_w), iw - 4 - cmd_w, dim);
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

            // rows 3–6: options
            for (row, label) in [(3u16, lu), (4, lo), (5, lc), (6, ln)] {
                blit_ch(buf, dx, dy + row, '│', cyan);
                blit(
                    buf,
                    dx + 2,
                    dy + row,
                    &padr(&trunc(label, iw - 1), iw - 1),
                    iw - 1,
                    white,
                );
                blit_ch(buf, dx + dw as u16 - 1, dy + row, '│', cyan);
            }

            // row 6: N + ESC on same row
            blit_ch(buf, dx, dy + 6, '│', cyan);
            let ln_w = sw(ln);
            let esc_w = sw(lesc);
            let gap = iw.saturating_sub(ln_w + esc_w + 2);
            blit(buf, dx + 2, dy + 6, ln, ln_w, white);
            blit(
                buf,
                dx + 2 + ln_w as u16,
                dy + 6,
                &" ".repeat(gap + 2),
                gap + 2,
                white,
            );
            blit(
                buf,
                dx + 2 + ln_w as u16 + (gap + 2) as u16,
                dy + 6,
                lesc,
                esc_w,
                dim,
            );
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
        // ボタン形式: Y:はい (通常) / N:いいえ (ハイライト = 安全側デフォルト)
        let y_btn = if app.lang_en { "  Y:Yes" } else { "  Y:はい" };
        let n_btn = if app.lang_en {
            "  N:No  "
        } else {
            "  N:いいえ  "
        };
        let yw = sw(y_btn);
        let nw = sw(n_btn);
        let pad = iw.saturating_sub(yw + nw);
        blit(buf, dx + 1, dy + 3, y_btn, yw, dim);
        blit(buf, dx + 1 + yw as u16, dy + 3, &" ".repeat(pad), pad, dim);
        blit(buf, dx + 1 + (yw + pad) as u16, dy + 3, n_btn, nw, rev);
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
    let dw = (area.width as usize).clamp(40, 72);
    let dx = ((area.width as usize).saturating_sub(dw) / 2) as u16;
    let dy = ((area.height as usize).saturating_sub(4) / 2) as u16;
    let iw = dw - 2;

    let buf = frame.buffer_mut();
    let red = Style::default().fg(Color::Red);
    let white = Style::default().fg(Color::White);
    let dim = Style::default().fg(Color::DarkGray);
    let cyan = app.ui_colors.border;
    let yellow = app.ui_colors.title;

    let title = if app.lang_en {
        " Error "
    } else {
        " エラー "
    };
    let hint = if app.lang_en {
        "  Press any key to close"
    } else {
        "  任意のキーで閉じる"
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
        red,
    );
    blit_ch(buf, dx + dw as u16 - 1, dy, '╮', cyan);

    blit_ch(buf, dx, dy + 1, '│', cyan);
    blit(buf, dx + 1, dy + 1, &padr(&trunc(msg, iw), iw), iw, white);
    blit_ch(buf, dx + dw as u16 - 1, dy + 1, '│', cyan);

    blit_ch(buf, dx, dy + 2, '│', cyan);
    blit(buf, dx + 1, dy + 2, &padr(&trunc(hint, iw), iw), iw, dim);
    blit_ch(buf, dx + dw as u16 - 1, dy + 2, '│', cyan);

    blit_ch(buf, dx, dy + 3, '╰', cyan);
    blit(buf, dx + 1, dy + 3, &"─".repeat(iw), iw, cyan);
    blit_ch(buf, dx + dw as u16 - 1, dy + 3, '╯', cyan);
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

            let base_style = ls_style(e, &app.ls_colors);
            let file_style = if is_cur {
                base_style.add_modifier(Modifier::REVERSED)
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
}
