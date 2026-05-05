use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::api::projects::FileNode;

const MAX_DEPTH: usize = 6;
const IGNORED_DIRS: &[&str] = &[
    ".git", "node_modules", "target", ".next", "dist", "build", "__pycache__", ".venv",
];

pub fn clone_repository(url: &str, dest: &str) -> Result<()> {
    git2::Repository::clone(url, dest)
        .map(|_| ())
        .map_err(|e| anyhow!("Clone failed: {}", e))
}

pub fn list_files(root: &str, dir: &str, depth: usize) -> Result<Vec<FileNode>> {
    if depth > MAX_DEPTH {
        return Ok(vec![]);
    }

    let root_path = Path::new(root);
    let dir_path = Path::new(dir);
    let mut entries = std::fs::read_dir(dir_path)?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        b_is_dir.cmp(&a_is_dir).then(a.file_name().cmp(&b.file_name()))
    });

    let mut nodes = vec![];
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name != ".env.example" {
            continue;
        }

        let file_type = entry.file_type()?;
        let full_path = entry.path();
        let relative = full_path
            .strip_prefix(root_path)
            .unwrap_or(&full_path)
            .to_string_lossy()
            .replace('\\', "/");

        if file_type.is_dir() {
            if IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            let children = list_files(root, full_path.to_str().unwrap_or(""), depth + 1)?;
            nodes.push(FileNode {
                name,
                path: relative,
                is_dir: true,
                children: Some(children),
            });
        } else {
            nodes.push(FileNode {
                name,
                path: relative,
                is_dir: false,
                children: None,
            });
        }
    }

    Ok(nodes)
}

pub fn read_file_content(root: &str, file_path: &str) -> Result<String> {
    let safe_path = sanitize_path(root, file_path)?;
    if safe_path.metadata().map(|m| m.len()).unwrap_or(0) > 2 * 1024 * 1024 {
        return Err(anyhow!("File too large (max 2MB)"));
    }
    std::fs::read_to_string(&safe_path).map_err(|e| anyhow!("Read error: {}", e))
}

pub fn write_file_content(root: &str, file_path: &str, content: &str) -> Result<()> {
    let safe_path = sanitize_path(root, file_path)?;
    if let Some(parent) = safe_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&safe_path, content).map_err(|e| anyhow!("Write error: {}", e))
}

pub fn git_status(repo_path: &str) -> Result<Value> {
    let repo = git2::Repository::open(repo_path)
        .map_err(|e| anyhow!("Not a git repository: {}", e))?;

    let head = repo
        .head()
        .ok()
        .and_then(|h| h.shorthand().map(|s| s.to_string()))
        .unwrap_or_else(|| "detached".into());

    let statuses = repo.statuses(None)?;
    let mut changed = vec![];
    let mut staged = vec![];
    let mut untracked = vec![];

    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("").to_string();
        let flags = entry.status();

        if flags.is_wt_modified() || flags.is_wt_deleted() {
            changed.push(path.clone());
        }
        if flags.is_index_modified() || flags.is_index_new() || flags.is_index_deleted() {
            staged.push(path.clone());
        }
        if flags.is_wt_new() {
            untracked.push(path.clone());
        }
    }

    Ok(json!({
        "branch": head,
        "changed": changed,
        "staged": staged,
        "untracked": untracked,
    }))
}

fn sanitize_path(root: &str, file_path: &str) -> Result<PathBuf> {
    let root = PathBuf::from(root).canonicalize()?;
    let joined = root.join(file_path);
    let canonical = joined
        .canonicalize()
        .map_err(|_| anyhow!("File not found: {}", file_path))?;

    if !canonical.starts_with(&root) {
        return Err(anyhow!("Path traversal not allowed"));
    }

    Ok(canonical)
}
