use super::*;
use std::{fs, path::{Path, PathBuf}};
use anyhow::Result;
use crate::fs_utils::{copy_path, move_path, tree_has_children, tree_list_subdirs};

/// src の mtime が dst より新しいか
fn is_src_newer(src: &std::path::Path, dst: &std::path::Path) -> bool {
    match (src.metadata(), dst.metadata()) {
        (Ok(sm), Ok(dm)) => match (sm.modified(), dm.modified()) {
            (Ok(st), Ok(dt)) => st > dt,
            _ => false,
        },
        _ => false,
    }
}

impl App {
    // ── File operations ─────────────────────────────────────────────────

    /// Returns tagged filenames (excluding ".."), or the cursor entry if none tagged.
    pub fn collect_op_targets(&self) -> Vec<String> {
        let tagged: Vec<String> = self
            .entries
            .iter()
            .zip(self.tagged.iter())
            .filter(|(e, t)| **t && e.name != "..")
            .map(|(e, _)| e.name.clone())
            .collect();
        if !tagged.is_empty() {
            tagged
        } else if let Some(e) = self.entries.get(self.cursor) {
            if e.name != ".." {
                vec![e.name.clone()]
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    }

    pub fn resolve_dest(&self, s: &str) -> PathBuf {
        let p = PathBuf::from(s);
        if p.is_absolute() {
            p
        } else {
            self.current_dir.join(p)
        }
    }

    fn process_copy_seq(
        &self,
        files: &[String],
        dest: &std::path::Path,
        is_move: bool,
        batch: Option<ConflictChoice>,
    ) -> Result<Option<OverwritePrompt>> {
        for (i, fname) in files.iter().enumerate() {
            let src = self.current_dir.join(fname);
            let dst = dest.join(fname);
            if dst.exists() {
                match batch {
                    None => {
                        return Ok(Some(OverwritePrompt {
                            dest: dest.to_path_buf(),
                            conflict: fname.clone(),
                            todo: files[i + 1..].to_vec(),
                            is_move,
                            batch: None,
                            cursor: 0,
                        }));
                    }
                    Some(ConflictChoice::Overwrite) => {
                        if is_move {
                            move_path(&src, &dst)?;
                        } else {
                            copy_path(&src, &dst)?;
                        }
                    }
                    Some(ConflictChoice::IfNewer) => {
                        if is_src_newer(&src, &dst) {
                            if is_move {
                                move_path(&src, &dst)?;
                            } else {
                                copy_path(&src, &dst)?;
                            }
                        }
                    }
                    Some(ConflictChoice::Skip) => {}
                }
            } else if is_move {
                move_path(&src, &dst)?;
            } else {
                copy_path(&src, &dst)?;
            }
        }
        Ok(None)
    }

    pub fn begin_copy(
        &self,
        dest_str: &str,
        targets: &[String],
    ) -> Result<Option<OverwritePrompt>> {
        let dest = self.resolve_dest(dest_str);
        fs::create_dir_all(&dest)?;
        for fname in targets {
            let src = self.current_dir.join(fname);
            if src.is_dir() {
                let canon_src = src.canonicalize().unwrap_or_else(|_| src.clone());
                let canon_dest = dest.canonicalize().unwrap_or_else(|_| dest.clone());
                if canon_dest.starts_with(&canon_src) {
                    anyhow::bail!("コピー先がコピー元の配下です: {}", fname);
                }
            }
        }
        self.process_copy_seq(targets, &dest, false, None)
    }

    pub fn begin_move(
        &self,
        dest_str: &str,
        targets: &[String],
    ) -> Result<Option<OverwritePrompt>> {
        let dest = self.resolve_dest(dest_str);
        fs::create_dir_all(&dest)?;
        self.process_copy_seq(targets, &dest, true, None)
    }

    // ── Conflict resolution: apply choice to current conflict, continue ──

    fn resume_conflict(
        &self,
        prompt: OverwritePrompt,
        choice: ConflictChoice,
        batch: Option<ConflictChoice>,
    ) -> Result<Option<OverwritePrompt>> {
        let src = self.current_dir.join(&prompt.conflict);
        let dst = prompt.dest.join(&prompt.conflict);
        let do_op = match choice {
            ConflictChoice::IfNewer => is_src_newer(&src, &dst),
            ConflictChoice::Overwrite => true,
            ConflictChoice::Skip => false,
        };
        if do_op {
            if prompt.is_move {
                move_path(&src, &dst)?;
            } else {
                copy_path(&src, &dst)?;
            }
        }
        self.process_copy_seq(&prompt.todo, &prompt.dest, prompt.is_move, batch)
    }

    /// O: 上書き（一件）
    pub fn resume_overwrite(&self, prompt: OverwritePrompt) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(prompt, ConflictChoice::Overwrite, None)
    }
    /// U: タイムスタンプが新しい時のみ（一件）
    pub fn resume_if_newer(&self, prompt: OverwritePrompt) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(prompt, ConflictChoice::IfNewer, None)
    }
    /// N: スキップ（一件）
    pub fn resume_skip(&self, prompt: OverwritePrompt) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(prompt, ConflictChoice::Skip, None)
    }
    /// Shift+O: 上書き（以降全件）
    pub fn resume_overwrite_batch(
        &self,
        prompt: OverwritePrompt,
    ) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(
            prompt,
            ConflictChoice::Overwrite,
            Some(ConflictChoice::Overwrite),
        )
    }
    /// Shift+U: タイムスタンプが新しい時のみ（以降全件）
    pub fn resume_if_newer_batch(
        &self,
        prompt: OverwritePrompt,
    ) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(
            prompt,
            ConflictChoice::IfNewer,
            Some(ConflictChoice::IfNewer),
        )
    }
    /// Shift+N: スキップ（以降全件）
    pub fn resume_skip_batch(&self, prompt: OverwritePrompt) -> Result<Option<OverwritePrompt>> {
        self.resume_conflict(prompt, ConflictChoice::Skip, Some(ConflictChoice::Skip))
    }
    /// C: 名前を変更して複写/移動
    pub fn resume_rename(
        &self,
        new_name: &str,
        prompt: OverwritePrompt,
    ) -> Result<Option<OverwritePrompt>> {
        let n = new_name.trim();
        if n.is_empty() {
            anyhow::bail!("名前が空です");
        }
        let src = self.current_dir.join(&prompt.conflict);
        let dst = prompt.dest.join(n);
        if dst.exists() {
            anyhow::bail!("\"{}\" は既に存在します", n);
        }
        if prompt.is_move {
            move_path(&src, &dst)?;
        } else {
            copy_path(&src, &dst)?;
        }
        self.process_copy_seq(&prompt.todo, &prompt.dest, prompt.is_move, prompt.batch)
    }

    pub fn exec_delete(&self, targets: &[String]) -> Result<()> {
        for fname in targets {
            if fname == ".." {
                continue;
            }
            let path = self.current_dir.join(fname);
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path)?;
            }
        }
        Ok(())
    }

    pub fn exec_rename(&self, new_name: &str, targets: &[String]) -> Result<()> {
        let new = new_name.trim();
        if new.is_empty() {
            anyhow::bail!("名前が空です");
        }
        if let Some(fname) = targets.first().filter(|f| *f != "..") {
            fs::rename(self.current_dir.join(fname), self.current_dir.join(new))?;
        }
        Ok(())
    }

    pub fn exec_mkdir(&self, name: &str) -> Result<()> {
        let n = name.trim();
        if n.is_empty() {
            anyhow::bail!("名前が空です");
        }
        fs::create_dir_all(self.current_dir.join(n))?;
        Ok(())
    }

    /// 同ディレクトリへの複写: 新ファイル名を指定してコピー
    pub fn exec_copy_newname(
        &self,
        new_name: &str,
        targets: &[String],
        dest_dir: &Path,
    ) -> Result<()> {
        let n = new_name.trim();
        if n.is_empty() {
            anyhow::bail!("名前が空です");
        }
        let src = match targets.first() {
            Some(f) => self.current_dir.join(f),
            None => anyhow::bail!("複写元ファイルが指定されていません"),
        };
        let dst = dest_dir.join(n);
        if dst.exists() {
            anyhow::bail!("\"{}\" は既に存在します", n);
        }
        copy_path(&src, &dst)?;
        Ok(())
    }

    pub fn exec_attr(&self, perm_str: &str, targets: &[String]) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let s = perm_str.trim();
        if s.is_empty() {
            anyhow::bail!("権限が空です");
        }
        let mode =
            u32::from_str_radix(s, 8).map_err(|_| anyhow::anyhow!("無効なオクタル値: {}", s))?;
        for fname in targets {
            if fname == ".." {
                continue;
            }
            let path = self.current_dir.join(fname);
            fs::set_permissions(&path, fs::Permissions::from_mode(mode))?;
        }
        Ok(())
    }
}

// ── Tree build (standalone, not part of App) ──────────────────────────

fn add_tree_node(
    nodes: &mut Vec<TreeNode>,
    path: PathBuf,
    depth: usize,
    ancestors: &std::collections::HashSet<PathBuf>,
    show_hidden: bool,
) {
    let name = if depth == 0 {
        "/".to_string()
    } else {
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let expanded = ancestors.contains(&path);
    let has_children = tree_has_children(&path, show_hidden);
    nodes.push(TreeNode {
        path: path.clone(),
        name,
        depth,
        expanded,
        has_children,
    });
    if expanded {
        for subpath in tree_list_subdirs(&path, show_hidden) {
            add_tree_node(nodes, subpath, depth + 1, ancestors, show_hidden);
        }
    }
}

/// tree_build はテストから直接呼びやすいよう pub のままにしておく
pub fn tree_build(current_dir: &PathBuf, show_hidden: bool) -> (Vec<TreeNode>, usize) {
    let mut ancestors: Vec<PathBuf> = Vec::new();
    let mut p: &std::path::Path = current_dir.as_path();
    loop {
        ancestors.push(p.to_path_buf());
        match p.parent() {
            Some(parent) if parent != p => p = parent,
            _ => break,
        }
    }
    ancestors.reverse();
    let ancestor_set: std::collections::HashSet<PathBuf> = ancestors.into_iter().collect();
    let mut nodes = Vec::new();
    add_tree_node(
        &mut nodes,
        PathBuf::from("/"),
        0,
        &ancestor_set,
        show_hidden,
    );
    let cursor = nodes
        .iter()
        .position(|n| &n.path == current_dir)
        .unwrap_or(0);
    (nodes, cursor)
}
