use super::*;
use crate::config::{ColorsConfig, LABELS_JA, UiColors};
use tempfile::tempdir;

/// テスト用の最小構成 App を指定ディレクトリで生成する。
fn make_test_app(dir: PathBuf) -> App {
    let ui_colors = UiColors::from_config(&ColorsConfig::default());
    let keymap = build_keymap(&HashMap::new());
    let menu_items = MENU_ACTIONS
        .iter()
        .map(|&(action, label_ja, _)| (key_label(&keymap, action), label_ja.to_string()))
        .collect();
    let mut app = App {
        current_dir: dir,
        entries: Vec::new(),
        cursor: 0,
        tagged: Vec::new(),
        col_mode: 2,
        quit: false,
        tree_open: false,
        tree_focus: false,
        tree_cursor: 0,
        tree_offset: 0,
        tree_nodes: Vec::new(),
        free_bytes: 0,
        total_bytes: 0,
        used_bytes: 0,
        volume_info: String::new(),
        file_type: String::new(),
        owner_s: String::new(),
        git_branch: None,
        git_ahead: 0,
        git_behind: 0,
        git_status: HashMap::new(),
        ls_colors: HashMap::new(),
        ui_colors,
        labels: &LABELS_JA,
        lang_en: false,
        keymap,
        menu_items,
        sort_mode: SortMode::None,
        sort_asc: true,
        show_sort_dialog: false,
        sort_cursor: 0,
        git_menu_cursor: 0,
        error_msg: None,
        success_msg: None,
        show_help: false,
        run_dialog: None,
        func_dialog: None,
        dir_jump_dialog: None,
        search: None,
        last_search: String::new(),
        git_dialog: None,
        git_running: false,
        file_dialog: None,
        show_hidden: false,
        pager: "less".to_string(),
        editor: "nano".to_string(),
        ext_programs: HashMap::new(),
        proc_mode: false,
        proc_entries: Vec::new(),
        proc_cursor: 0,
        proc_offset: 0,
        proc_sort: crate::proc::ProcSortMode::Cpu,
        proc_sort_asc: false,
        proc_signal_menu: None,
        proc_detail: crate::proc::ProcDetail::default(),
        proc_tree: false,
        proc_tree_rows: Vec::new(),
        right_panel: crate::proc::RightPanel::None,
        right_panel_focus: false,
        thread_entries: Vec::new(),
        thread_error: None,
        thread_cursor: 0,
        thread_offset: 0,
        thread_pid: 0,
        fd_pid: 0,
        fd_proc_name: String::new(),
        fd_entries: Vec::new(),
        fd_error: None,
        fd_cursor: 0,
        fd_offset: 0,
        is_root: false,
    };
    app.load_entries().unwrap();
    app.update_file_info();
    app
}

fn canonical(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

// ── load_entries ───────────────────────────────────────────────────

#[test]
fn test_load_entries_empty_dir_has_dotdot() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    // 空ディレクトリでも ".." エントリが必ず存在する
    assert_eq!(app.entries.len(), 1);
    assert_eq!(app.entries[0].name, "..");
    assert!(app.entries[0].is_dir);
}

#[test]
fn test_load_entries_dirs_before_files() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("aaa.txt"), "").unwrap();
    std::fs::create_dir(dir.path().join("bbb")).unwrap();

    let app = make_test_app(dir.path().to_path_buf());
    // 順序: ".." → ディレクトリ → ファイル
    assert_eq!(app.entries[0].name, "..");
    assert_eq!(app.entries[1].name, "bbb");
    assert!(app.entries[1].is_dir);
    assert_eq!(app.entries[2].name, "aaa.txt");
}

#[test]
fn test_load_entries_alphabetical_sort() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("charlie.txt"), "").unwrap();
    std::fs::write(dir.path().join("alpha.txt"), "").unwrap();
    std::fs::write(dir.path().join("bravo.txt"), "").unwrap();

    let app = make_test_app(dir.path().to_path_buf());
    let names: Vec<&str> = app
        .entries
        .iter()
        .skip(1)
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, vec!["alpha.txt", "bravo.txt", "charlie.txt"]);
}

#[test]
fn test_load_entries_hides_dotfiles_by_default() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("visible.txt"), "").unwrap();
    std::fs::write(dir.path().join(".hidden"), "").unwrap();

    let app = make_test_app(dir.path().to_path_buf());
    let names: Vec<&str> = app.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"visible.txt"));
    assert!(!names.contains(&".hidden"));
}

#[test]
fn test_load_entries_shows_dotfiles_when_enabled() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join(".hidden"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.show_hidden = true;
    app.load_entries().unwrap();

    let names: Vec<&str> = app.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&".hidden"));
}

// ── file_count / dir_count / current_total_bytes ───────────────────

#[test]
fn test_file_and_dir_count() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();
    std::fs::create_dir(dir.path().join("mydir")).unwrap();

    let app = make_test_app(dir.path().to_path_buf());
    assert_eq!(app.file_count(), 2);
    assert_eq!(app.dir_count(), 1); // ".." は除外される
}

#[test]
fn test_current_total_bytes() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello").unwrap(); // 5 bytes
    std::fs::write(dir.path().join("b.txt"), "world!").unwrap(); // 6 bytes

    let app = make_test_app(dir.path().to_path_buf());
    assert_eq!(app.current_total_bytes(), 11);
}

// ── enter_dir ──────────────────────────────────────────────────────

#[test]
fn test_enter_subdir_resets_cursor() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("subdir")).unwrap();
    std::fs::write(dir.path().join("subdir").join("file.txt"), "hi").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.cursor = app.entries.iter().position(|e| e.name == "subdir").unwrap();
    app.enter_dir("subdir").unwrap();

    assert_eq!(app.current_dir, dir.path().join("subdir"));
    assert_eq!(app.cursor, 0);
    assert!(app.entries.iter().any(|e| e.name == "file.txt"));
}

#[test]
fn test_enter_parent_dir_restores_cursor() {
    let dir = tempdir().unwrap();
    let child = dir.path().join("child");
    std::fs::create_dir(&child).unwrap();

    let mut app = make_test_app(child);
    app.enter_dir("..").unwrap();

    assert_eq!(app.current_dir, dir.path().to_path_buf());
    // カーソルが "child" エントリを指している
    let cur = &app.entries[app.cursor];
    assert_eq!(cur.name, "child");
}

// ── タグ操作 ───────────────────────────────────────────────────────

#[test]
fn test_tag_toggle() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.txt"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    // index 1 = "foo.txt" (.." が index 0)
    app.cursor = 1;
    assert!(!app.tagged[1]);
    app.tag_toggle();
    assert!(app.tagged[1]);
    app.tag_toggle();
    assert!(!app.tagged[1]);
}

#[test]
fn test_tag_all_tags_and_untags() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    // 全タグ OFF → 全タグ ON
    app.tag_all();
    assert!(!app.tagged[0]);
    assert!(app.tagged[1..].iter().all(|&t| t));
    // 全タグ ON → 全タグ OFF
    app.tag_all();
    assert!(app.tagged.iter().all(|&t| !t));
}

#[test]
fn test_tag_toggle_move_advances_cursor() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.cursor = 0;
    app.tag_toggle_move(10);
    assert!(!app.tagged[0]); // ".." はタグ対象外
    assert_eq!(app.cursor, 1); // カーソルが進む
}

#[test]
fn test_tag_toggle_ignores_dotdot() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.txt"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.cursor = 0;
    app.tag_toggle();
    assert!(!app.tagged[0]);
}

// ── カーソル移動 ───────────────────────────────────────────────────

#[test]
fn test_move_down_and_up() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf()); // 3 entries: .., a, b
    assert_eq!(app.cursor, 0);
    app.move_down(10);
    assert_eq!(app.cursor, 1);
    app.move_down(10);
    assert_eq!(app.cursor, 2);
    app.move_down(10); // 末尾 → 動かない
    assert_eq!(app.cursor, 2);
    app.move_up(10);
    assert_eq!(app.cursor, 1);
    app.move_up(10);
    assert_eq!(app.cursor, 0);
    app.move_up(10); // 先頭 → 動かない
    assert_eq!(app.cursor, 0);
}

#[test]
fn test_page_down_and_up() {
    let dir = tempdir().unwrap();
    for i in 0..9 {
        std::fs::write(dir.path().join(format!("{:02}.txt", i)), "").unwrap();
    }
    // エントリ数: ".." + 9ファイル = 10; col_mode=2, lh=3 → per_page=6
    let mut app = make_test_app(dir.path().to_path_buf());
    let lh = 3;
    assert_eq!(app.current_page(lh), 0);
    app.page_down(lh);
    assert_eq!(app.current_page(lh), 1);
    app.page_up(lh);
    assert_eq!(app.current_page(lh), 0);
}

#[test]
fn test_first_and_last_page() {
    let dir = tempdir().unwrap();
    for i in 0..5 {
        std::fs::write(dir.path().join(format!("{}.txt", i)), "").unwrap();
    }
    let mut app = make_test_app(dir.path().to_path_buf());
    let lh = 2;
    let last = app.entries.len() - 1;
    app.cursor = last;
    assert_eq!(app.current_page(lh), app.total_pages(lh) - 1);
    app.cursor = 0;
    assert_eq!(app.current_page(lh), 0);
}

#[test]
fn test_first_list_entry_index_skips_dotdot() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();

    let app = make_test_app(dir.path().to_path_buf());
    assert_eq!(app.first_list_entry_index(), 1);
}

#[test]
fn test_first_list_entry_index_at_root_is_zero() {
    let app = make_test_app(PathBuf::from("/"));
    assert_eq!(app.first_list_entry_index(), 0);
}

#[test]
fn test_last_list_entry_index_points_to_last_entry() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();

    let app = make_test_app(dir.path().to_path_buf());
    assert_eq!(app.last_list_entry_index(), app.entries.len() - 1);
}

// ── cols / per_page ────────────────────────────────────────────────

#[test]
fn test_cols_and_per_page() {
    let dir = tempdir().unwrap();
    let mut app = make_test_app(dir.path().to_path_buf());

    app.col_mode = 1;
    assert_eq!(app.cols(), 1);
    assert_eq!(app.per_page(10), 10);
    app.col_mode = 2;
    assert_eq!(app.cols(), 2);
    assert_eq!(app.per_page(10), 20);
    app.col_mode = 3;
    assert_eq!(app.cols(), 3);
    assert_eq!(app.per_page(10), 30);
    app.col_mode = 5;
    assert_eq!(app.cols(), 5);
    assert_eq!(app.per_page(10), 50);
}

// ── tree_build ─────────────────────────────────────────────────────

#[test]
fn test_tree_build_root_node() {
    let dir = tempdir().unwrap();
    let dir = canonical(dir.path());
    // tempfile が作るディレクトリは /tmp/.tmpXXX のように隠しディレクトリに
    // なることがあるため show_hidden=true で確実に祖先パスを辿れるようにする
    let (nodes, cursor) = tree_build(&dir, true);
    assert_eq!(nodes[0].name, "/");
    assert_eq!(nodes[0].depth, 0);
    assert_eq!(nodes[cursor].path, dir);
}

// ── collect_op_targets ─────────────────────────────────────────────

#[test]
fn test_collect_op_targets_cursor() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    let mut app = make_test_app(dir.path().to_path_buf());
    app.cursor = 1; // "a.txt"
    let targets = app.collect_op_targets();
    assert_eq!(targets, vec!["a.txt"]);
}

#[test]
fn test_collect_op_targets_tagged() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();
    let mut app = make_test_app(dir.path().to_path_buf());
    app.tagged[2] = true; // tag "b.txt"
    let targets = app.collect_op_targets();
    assert_eq!(targets, vec!["b.txt"]);
}

#[test]
fn test_collect_op_targets_excludes_dotdot() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    // cursor=0 is ".."
    let targets = app.collect_op_targets();
    assert!(targets.is_empty());
}

// ── exec_delete ────────────────────────────────────────────────────

#[test]
fn test_exec_delete_file() {
    let dir = tempdir().unwrap();
    let f = dir.path().join("del.txt");
    std::fs::write(&f, "x").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    app.exec_delete(&["del.txt".to_string()]).unwrap();
    assert!(!f.exists());
}

#[test]
fn test_exec_delete_dir_recursive() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("f.txt"), "x").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    app.exec_delete(&["sub".to_string()]).unwrap();
    assert!(!sub.exists());
}

#[test]
fn test_exec_delete_permission_error_propagated() {
    use std::os::unix::fs::PermissionsExt;
    // root は権限を無視するのでスキップ
    let uid: u32 = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1);
    if uid == 0 {
        return;
    }

    let dir = tempdir().unwrap();
    let sub = dir.path().join("sub");
    let inner = sub.join("locked");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join("file.txt"), "x").unwrap();
    // inner を mode 000 にして中を削除不可能にする
    std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o000)).unwrap();

    let app = make_test_app(dir.path().to_path_buf());
    let result = app.exec_delete(&["sub".to_string()]);

    // クリーンアップできるよう権限を戻す
    std::fs::set_permissions(&inner, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(result.is_err(), "権限エラーが伝播すること");
}

#[test]
fn test_exec_delete_skips_dotdot() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    // ".." should be silently skipped, not cause an error
    app.exec_delete(&["..".to_string()]).unwrap();
}

// ── exec_rename ────────────────────────────────────────────────────

#[test]
fn test_exec_rename() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("old.txt"), "x").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    app.exec_rename("new.txt", &["old.txt".to_string()])
        .unwrap();
    assert!(!dir.path().join("old.txt").exists());
    assert!(dir.path().join("new.txt").exists());
}

#[test]
fn test_exec_rename_empty_name_errors() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("foo.txt"), "").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    assert!(app.exec_rename("  ", &["foo.txt".to_string()]).is_err());
}

// ── exec_mkdir ─────────────────────────────────────────────────────

#[test]
fn test_exec_mkdir() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    app.exec_mkdir("newdir").unwrap();
    assert!(dir.path().join("newdir").is_dir());
}

#[test]
fn test_exec_mkdir_empty_name_errors() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    assert!(app.exec_mkdir("").is_err());
}

// ── exec_attr ──────────────────────────────────────────────────────

#[test]
fn test_exec_attr_chmod() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let f = dir.path().join("f.txt");
    std::fs::write(&f, "x").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    app.exec_attr("644", &["f.txt".to_string()]).unwrap();
    let mode = std::fs::metadata(&f).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644);
}

#[test]
fn test_exec_attr_invalid_octal_errors() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    assert!(app.exec_attr("999", &["f.txt".to_string()]).is_err());
    assert!(app.exec_attr("xyz", &["f.txt".to_string()]).is_err());
}

// ── begin_copy / process_copy_seq ──────────────────────────────────

#[test]
fn test_begin_copy_file() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
    let dst_dir = dir.path().join("dst");
    let app = make_test_app(dir.path().to_path_buf());
    let r = app
        .begin_copy(&dst_dir.to_string_lossy(), &["a.txt".to_string()])
        .unwrap();
    assert!(r.is_none()); // no conflict
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"hello");
    assert!(dir.path().join("a.txt").exists()); // original still there
}

#[test]
fn test_begin_copy_dir_recursive() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("src_dir");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("f.txt"), b"x").unwrap();
    let dst_dir = dir.path().join("dst");
    let app = make_test_app(dir.path().to_path_buf());
    let r = app
        .begin_copy(&dst_dir.to_string_lossy(), &["src_dir".to_string()])
        .unwrap();
    assert!(r.is_none());
    assert!(dst_dir.join("src_dir").join("f.txt").exists());
}

#[test]
fn test_begin_copy_detects_conflict() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"src").unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    std::fs::write(dst_dir.join("a.txt"), b"existing").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let r = app
        .begin_copy(&dst_dir.to_string_lossy(), &["a.txt".to_string()])
        .unwrap();
    assert!(r.is_some());
    let prompt = r.unwrap();
    assert_eq!(prompt.conflict, "a.txt");
    assert!(prompt.todo.is_empty());
    // existing file NOT overwritten yet
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"existing");
}

#[test]
fn test_begin_copy_cyclic_guard() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    // copying "sub" into "sub/inner" would be cyclic
    let inner = sub.join("inner");
    let r = app.begin_copy(&inner.to_string_lossy(), &["sub".to_string()]);
    assert!(r.is_err());
}

// ── resume_overwrite / resume_if_newer / resume_skip / batch / rename ──

fn make_conflict_prompt(
    dst_dir: &std::path::Path,
    conflict: &str,
    todo: Vec<String>,
    is_move: bool,
) -> OverwritePrompt {
    OverwritePrompt {
        dest: dst_dir.to_path_buf(),
        conflict: conflict.to_string(),
        todo,
        is_move,
        batch: None,
        cursor: 0,
    }
}

#[test]
fn test_resume_overwrite() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"new").unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    std::fs::write(dst_dir.join("a.txt"), b"old").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
    let r = app.resume_overwrite(prompt).unwrap();
    assert!(r.is_none());
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"new");
}

#[test]
fn test_resume_skip() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"new").unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    std::fs::write(dst_dir.join("a.txt"), b"old").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
    let r = app.resume_skip(prompt).unwrap();
    assert!(r.is_none());
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"old"); // not overwritten
}

#[test]
fn test_resume_overwrite_batch_applies_to_remaining() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"new_a").unwrap();
    std::fs::write(dir.path().join("b.txt"), b"new_b").unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    std::fs::write(dst_dir.join("a.txt"), b"old_a").unwrap();
    std::fs::write(dst_dir.join("b.txt"), b"old_b").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec!["b.txt".to_string()], false);
    let r = app.resume_overwrite_batch(prompt).unwrap();
    assert!(r.is_none()); // no more conflicts — batch processed b.txt automatically
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"new_a");
    assert_eq!(std::fs::read(dst_dir.join("b.txt")).unwrap(), b"new_b");
}

#[test]
fn test_resume_skip_batch_applies_to_remaining() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"new_a").unwrap();
    std::fs::write(dir.path().join("b.txt"), b"new_b").unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    std::fs::write(dst_dir.join("a.txt"), b"old_a").unwrap();
    std::fs::write(dst_dir.join("b.txt"), b"old_b").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec!["b.txt".to_string()], false);
    let r = app.resume_skip_batch(prompt).unwrap();
    assert!(r.is_none()); // all skipped — no conflict returned
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"old_a"); // unchanged
    assert_eq!(std::fs::read(dst_dir.join("b.txt")).unwrap(), b"old_b"); // unchanged
}

#[test]
fn test_resume_if_newer_overwrites_when_src_newer() {
    let dir = tempdir().unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    // write old dst first, then new src
    std::fs::write(dst_dir.join("a.txt"), b"old").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(dir.path().join("a.txt"), b"new").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
    let r = app.resume_if_newer(prompt).unwrap();
    assert!(r.is_none());
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"new");
}

#[test]
fn test_resume_if_newer_skips_when_src_older() {
    let dir = tempdir().unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    // write new dst first, then old src
    std::fs::write(dir.path().join("a.txt"), b"old").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(dst_dir.join("a.txt"), b"newer").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
    let r = app.resume_if_newer(prompt).unwrap();
    assert!(r.is_none());
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"newer"); // not overwritten
}

#[test]
fn test_resume_rename_copies_with_new_name() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    std::fs::write(dst_dir.join("a.txt"), b"existing").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
    let r = app.resume_rename("a_copy.txt", prompt).unwrap();
    assert!(r.is_none());
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"existing"); // original untouched
    assert_eq!(std::fs::read(dst_dir.join("a_copy.txt")).unwrap(), b"data");
}

#[test]
fn test_resume_rename_errors_if_new_name_exists() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"data").unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    std::fs::write(dst_dir.join("a.txt"), b"existing").unwrap();
    std::fs::write(dst_dir.join("b.txt"), b"also").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec![], false);
    assert!(app.resume_rename("b.txt", prompt).is_err());
}

#[test]
fn test_resume_skip_next_conflict_propagates() {
    // skipping first conflict still returns next conflict
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"new_a").unwrap();
    std::fs::write(dir.path().join("b.txt"), b"new_b").unwrap();
    let dst_dir = dir.path().join("dst");
    std::fs::create_dir(&dst_dir).unwrap();
    std::fs::write(dst_dir.join("a.txt"), b"old_a").unwrap();
    std::fs::write(dst_dir.join("b.txt"), b"old_b").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let prompt = make_conflict_prompt(&dst_dir, "a.txt", vec!["b.txt".to_string()], false);
    let r = app.resume_skip(prompt).unwrap();
    assert!(r.is_some()); // b.txt conflict returned
    assert_eq!(r.unwrap().conflict, "b.txt");
    assert_eq!(std::fs::read(dst_dir.join("a.txt")).unwrap(), b"old_a"); // unchanged
}

// ── exec_copy_newname ──────────────────────────────────────────────

#[test]
fn test_exec_copy_newname_basic() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("orig.txt"), b"data").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    app.exec_copy_newname(
        "copy.txt",
        &["orig.txt".to_string()],
        &dir.path().to_path_buf(),
    )
    .unwrap();
    assert!(dir.path().join("orig.txt").exists()); // 元は残る
    assert_eq!(std::fs::read(dir.path().join("copy.txt")).unwrap(), b"data");
}

#[test]
fn test_exec_copy_newname_same_name_errors() {
    // 同名を入力した場合（dst.exists()チェックで弾く）
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("orig.txt"), b"data").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let result = app.exec_copy_newname(
        "orig.txt",
        &["orig.txt".to_string()],
        &dir.path().to_path_buf(),
    );
    assert!(result.is_err(), "同名コピーはエラーになること");
    // 元ファイルが壊れていないこと
    assert_eq!(std::fs::read(dir.path().join("orig.txt")).unwrap(), b"data");
}

#[test]
fn test_exec_copy_newname_dest_exists_errors() {
    // 新名前のファイルが既に存在する場合もエラー
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("src.txt"), b"src").unwrap();
    std::fs::write(dir.path().join("existing.txt"), b"existing").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    let result = app.exec_copy_newname(
        "existing.txt",
        &["src.txt".to_string()],
        &dir.path().to_path_buf(),
    );
    assert!(result.is_err());
    // 既存ファイルが上書きされていないこと
    assert_eq!(
        std::fs::read(dir.path().join("existing.txt")).unwrap(),
        b"existing"
    );
}

#[test]
fn test_exec_copy_newname_empty_name_errors() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("orig.txt"), "").unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    assert!(
        app.exec_copy_newname("  ", &["orig.txt".to_string()], &dir.path().to_path_buf())
            .is_err()
    );
}

// ── begin_move ─────────────────────────────────────────────────────

#[test]
fn test_begin_move_file() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), b"data").unwrap();
    let dst_dir = dir.path().join("dst");
    let app = make_test_app(dir.path().to_path_buf());
    let r = app
        .begin_move(&dst_dir.to_string_lossy(), &["f.txt".to_string()])
        .unwrap();
    assert!(r.is_none());
    assert!(!dir.path().join("f.txt").exists());
    assert_eq!(std::fs::read(dst_dir.join("f.txt")).unwrap(), b"data");
}

// ── permission error handling ───────────────────────────────────────

#[test]
fn test_enter_dir_permission_denied_rolls_back() {
    // root はパーミッション制限を無視するためスキップ
    let is_root = std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        == Some(0);
    if is_root {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = tempdir().unwrap();
    let sub = dir.path().join("locked");
    std::fs::create_dir(&sub).unwrap();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000)).unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    let original_dir = app.current_dir.clone();

    let result = app.enter_dir("locked");
    // パーミッションを戻してから assert（後片付け保証）
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        result.is_err(),
        "enter_dir should fail on permission-denied directory"
    );
    assert_eq!(
        app.current_dir, original_dir,
        "current_dir should be rolled back on failure"
    );
}

#[test]
fn test_reload_sets_error_msg_on_invalid_dir() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();

    let mut app = make_test_app(sub.clone());
    assert!(app.error_msg.is_none());

    std::fs::remove_dir(&sub).unwrap();
    app.reload();

    assert!(
        app.error_msg.is_some(),
        "reload() should set error_msg when directory no longer exists"
    );
}

#[test]
fn test_tree_rebuild_updates_nodes() {
    let dir = tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let dir = canonical(dir.path());

    let mut app = make_test_app(dir.clone());
    app.show_hidden = true; // 同上: 隠しパスを確実に辿るため
    assert!(app.tree_nodes.is_empty());
    app.tree_rebuild();
    assert!(!app.tree_nodes.is_empty());
    assert_eq!(app.tree_nodes[app.tree_cursor].path, dir);
}

#[test]
fn test_system_symlink_dirs_are_enterable() {
    for name in ["tmp", "etc"] {
        let path = PathBuf::from("/").join(name);
        if !path.exists()
            || !std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        {
            continue;
        }

        let mut app = make_test_app(PathBuf::from("/"));
        let idx = app.entries.iter().position(|e| e.name == name).unwrap();
        assert!(
            app.entries[idx].is_link,
            "{} should be recognized as symlink",
            name
        );
        assert!(
            app.entries[idx].is_dir,
            "{} should be treated as enterable directory",
            name
        );

        app.enter_dir(name).unwrap();
        assert_eq!(app.current_dir, PathBuf::from("/").join(name));
        assert!(
            !app.entries.is_empty(),
            "{} target should be readable after enter",
            name
        );
    }
}

// ── GitDialog / GitDialogState ─────────────────────────────────────

#[test]
fn test_git_dialog_stash_msg_initial_state() {
    let state = GitDialogState::StashMsg { input: vec![], cursor: 0 };
    if let GitDialogState::StashMsg { input, cursor } = state {
        assert!(input.is_empty());
        assert_eq!(cursor, 0);
    } else {
        panic!("wrong variant");
    }
}

#[test]
fn test_git_dialog_stash_msg_with_content() {
    let chars: Vec<char> = "wip".chars().collect();
    let state = GitDialogState::StashMsg { input: chars.clone(), cursor: 3 };
    if let GitDialogState::StashMsg { input, cursor } = state {
        assert_eq!(input, chars);
        assert_eq!(cursor, 3);
    } else {
        panic!("wrong variant");
    }
}

// ── SortMode ───────────────────────────────────────────────────────

#[test]
fn test_sort_dotdot_always_first() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("zzz.txt"), "").unwrap();
    std::fs::create_dir(dir.path().join("aaa")).unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.sort_mode = SortMode::Name;
    app.sort_asc = false; // 逆順でも ".." は先頭のまま
    app.load_entries().unwrap();

    assert_eq!(app.entries[0].name, "..");
}

#[test]
fn test_sort_name_ascending() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("charlie.txt"), "").unwrap();
    std::fs::write(dir.path().join("alpha.txt"), "").unwrap();
    std::fs::write(dir.path().join("bravo.txt"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.sort_mode = SortMode::Name;
    app.sort_asc = true;
    app.load_entries().unwrap();

    let names: Vec<&str> = app.entries.iter().skip(1).map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["alpha.txt", "bravo.txt", "charlie.txt"]);
}

#[test]
fn test_sort_name_descending() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("charlie.txt"), "").unwrap();
    std::fs::write(dir.path().join("alpha.txt"), "").unwrap();
    std::fs::write(dir.path().join("bravo.txt"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.sort_mode = SortMode::Name;
    app.sort_asc = false;
    app.load_entries().unwrap();

    let names: Vec<&str> = app.entries.iter().skip(1).map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["charlie.txt", "bravo.txt", "alpha.txt"]);
}

#[test]
fn test_sort_by_extension() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("b.rs"), "").unwrap();
    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("c.md"), "").unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.sort_mode = SortMode::Ext;
    app.sort_asc = true;
    app.load_entries().unwrap();

    // 拡張子順: md < rs < txt
    let names: Vec<&str> = app.entries.iter().skip(1).map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["c.md", "b.rs", "a.txt"]);
}

#[test]
fn test_sort_by_size_ascending() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("big.txt"), "hello world foo").unwrap(); // 15 bytes
    std::fs::write(dir.path().join("small.txt"), "hi").unwrap(); // 2 bytes
    std::fs::write(dir.path().join("mid.txt"), "hello").unwrap(); // 5 bytes

    let mut app = make_test_app(dir.path().to_path_buf());
    app.sort_mode = SortMode::Size;
    app.sort_asc = true;
    app.load_entries().unwrap();

    let names: Vec<&str> = app.entries.iter().skip(1).map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["small.txt", "mid.txt", "big.txt"]);
}

#[test]
fn test_sort_dirs_and_files_sorted_independently() {
    // ディレクトリとファイルはそれぞれ独立してソートされる（ディレクトリが先）
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("a_file.txt"), "").unwrap();
    std::fs::create_dir(dir.path().join("z_dir")).unwrap();
    std::fs::create_dir(dir.path().join("a_dir")).unwrap();

    let mut app = make_test_app(dir.path().to_path_buf());
    app.sort_mode = SortMode::Name;
    app.sort_asc = true;
    app.load_entries().unwrap();

    // ".." → a_dir → z_dir → a_file.txt
    assert_eq!(app.entries[0].name, "..");
    assert_eq!(app.entries[1].name, "a_dir");
    assert_eq!(app.entries[2].name, "z_dir");
    assert_eq!(app.entries[3].name, "a_file.txt");
}

// ── SearchState ───────────────────────────────────────────────────

#[test]
fn test_search_state_initial_not_confirmed() {
    let state = SearchState {
        input: vec!['a'],
        cursor: 1,
        matches: vec![0],
        match_idx: 0,
        origin: 0,
        confirmed: false,
    };
    assert!(!state.confirmed);
}

#[test]
fn test_search_state_confirmed_flag() {
    let mut state = SearchState {
        input: vec!['a'],
        cursor: 1,
        matches: vec![0],
        match_idx: 0,
        origin: 0,
        confirmed: false,
    };
    state.confirmed = true;
    assert!(state.confirmed);
}

// ── success_msg / show_help initial state ─────────────────────────

#[test]
fn test_success_msg_none_by_default() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    assert!(app.success_msg.is_none());
}

#[test]
fn test_show_help_false_by_default() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    assert!(!app.show_help);
}

#[test]
fn test_sort_mode_none_by_default() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    assert_eq!(app.sort_mode, SortMode::None);
    assert!(app.sort_asc);
}

#[test]
fn test_git_dialog_none_by_default() {
    let dir = tempdir().unwrap();
    let app = make_test_app(dir.path().to_path_buf());
    assert!(app.git_dialog.is_none());
}

#[test]
fn test_git_dialog_open_sets_menu_state() {
    let dir = tempdir().unwrap();
    let mut app = make_test_app(dir.path().to_path_buf());
    app.git_dialog = Some(GitDialog { state: GitDialogState::Menu });
    assert!(matches!(
        app.git_dialog.as_ref().unwrap().state,
        GitDialogState::Menu
    ));
}

#[test]
fn test_git_dialog_transition_to_stash_msg() {
    let dir = tempdir().unwrap();
    let mut app = make_test_app(dir.path().to_path_buf());
    app.git_dialog = Some(GitDialog {
        state: GitDialogState::StashMsg { input: vec![], cursor: 0 },
    });
    assert!(matches!(
        app.git_dialog.as_ref().unwrap().state,
        GitDialogState::StashMsg { .. }
    ));
}
