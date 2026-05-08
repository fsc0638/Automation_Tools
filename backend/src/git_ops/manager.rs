use anyhow::{anyhow, Result};
use git2::{build::RepoBuilder, Cred, FetchOptions, RemoteCallbacks, Repository};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::api::projects::FileNode;

const MAX_DEPTH: usize = 6;
const IGNORED_DIRS: &[&str] = &[
    ".git", "node_modules", "target", ".next", "dist", "build", "__pycache__", ".venv",
];

#[derive(Debug, Clone)]
pub struct GitCredentials {
    pub username: String,
    pub access_token: String,
}

fn fetch_options(credentials: Option<&GitCredentials>) -> FetchOptions<'static> {
    let mut callbacks = RemoteCallbacks::new();
    if let Some(creds) = credentials.cloned() {
        callbacks.credentials(move |_url, username_from_url, _allowed| {
            let username = username_from_url.unwrap_or(&creds.username);
            Cred::userpass_plaintext(username, &creds.access_token)
        });
    }

    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    fetch_options
}

pub fn clone_repository(url: &str, dest: &str, credentials: Option<&GitCredentials>, branch: Option<&str>) -> Result<()> {
    // Embed credentials directly in the URL so git2 sends them on the first
    // request, instead of waiting for a 401 challenge. GitHub may return 403
    // (not 401) for private repos, which means the credentials callback is
    // never invoked and the clone fails even with valid credentials.
    let effective_url = match credentials {
        Some(creds) => embed_credentials_in_url(url, creds),
        None => url.to_string(),
    };

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fetch_options(credentials));
    if let Some(branch) = branch.filter(|b| !b.trim().is_empty()) {
        builder.branch(branch);
    }

    builder
        .clone(&effective_url, Path::new(dest))
        .map(|_| ())
        .map_err(|e| anyhow!("Clone failed: {}", e))
}

fn embed_credentials_in_url(url: &str, creds: &GitCredentials) -> String {
    // Only applies to HTTPS URLs. SSH URLs use key-based auth.
    let Some(rest) = url.strip_prefix("https://") else {
        return url.to_string();
    };
    // Strip any existing user info (e.g. https://user@host/...) to avoid duplication.
    let rest = if let Some(at) = rest.find('@') {
        &rest[at + 1..]
    } else {
        rest
    };
    // URL-encode only the characters that would break URL parsing.
    let token = creds.access_token.replace('@', "%40").replace(':', "%3A");
    format!("https://{}:{}@{}", creds.username, token, rest)
}

pub fn list_branches(repo_path: &str) -> Result<Vec<String>> {
    use std::collections::BTreeSet;
    let repo = Repository::open(repo_path)
        .map_err(|e| anyhow!("Not a git repository: {}", e))?;

    let mut names: BTreeSet<String> = BTreeSet::new();

    for branch in repo.branches(Some(git2::BranchType::Local))? {
        let (b, _) = branch?;
        if let Some(name) = b.name()? {
            names.insert(name.to_string());
        }
    }

    // Include remote tracking branches under origin/* so the dropdown
    // surfaces branches that exist remotely but haven't been checked out
    // locally yet. checkout_branch already creates the local copy on demand.
    for branch in repo.branches(Some(git2::BranchType::Remote))? {
        let (b, _) = branch?;
        if let Some(full_name) = b.name()? {
            if let Some(short) = full_name.strip_prefix("origin/") {
                if short != "HEAD" {
                    names.insert(short.to_string());
                }
            }
        }
    }

    Ok(names.into_iter().collect())
}

#[derive(Debug, Clone, Copy)]
pub enum SyncResult {
    AlreadyUpToDate,
    FastForwarded,
    NoRemoteBranch,
}

/// Fetch from origin and fast-forward the current branch to its remote
/// tracking ref. Errors out (without modifying state) when the local branch
/// has diverged — this is `git pull --ff-only` semantics on purpose.
pub fn sync_current_branch(
    repo_path: &str,
    credentials: Option<&GitCredentials>,
) -> Result<SyncResult> {
    let repo = Repository::open(repo_path)
        .map_err(|e| anyhow!("Not a git repository: {}", e))?;

    fetch_remote(repo_path, credentials)?;

    let head = repo.head()?;
    let branch_name = head
        .shorthand()
        .ok_or_else(|| anyhow!("HEAD is detached; check out a branch first"))?
        .to_string();

    let remote_refname = format!("refs/remotes/origin/{}", branch_name);
    let remote_ref = match repo.find_reference(&remote_refname) {
        Ok(r) => r,
        Err(_) => return Ok(SyncResult::NoRemoteBranch),
    };
    let remote_oid = remote_ref
        .target()
        .ok_or_else(|| anyhow!("remote ref has no target"))?;

    let annotated = repo.find_annotated_commit(remote_oid)?;
    let (analysis, _) = repo.merge_analysis(&[&annotated])?;

    if analysis.is_up_to_date() {
        return Ok(SyncResult::AlreadyUpToDate);
    }

    if analysis.is_fast_forward() {
        let local_refname = format!("refs/heads/{}", branch_name);
        let mut reference = repo.find_reference(&local_refname)?;
        reference.set_target(remote_oid, "Fast-forward via sync")?;
        repo.set_head(&local_refname)?;
        let mut checkout = git2::build::CheckoutBuilder::default();
        checkout.force();
        repo.checkout_head(Some(&mut checkout))?;
        return Ok(SyncResult::FastForwarded);
    }

    Err(anyhow!(
        "Local branch '{}' has diverged from origin; cannot fast-forward. \
        Resolve manually (commit, stash, or switch branches).",
        branch_name
    ))
}

pub fn fetch_remote(repo_path: &str, credentials: Option<&GitCredentials>) -> Result<()> {
    let repo = Repository::open(repo_path)
        .map_err(|e| anyhow!("Not a git repository: {}", e))?;
    let mut remote = repo.find_remote("origin")?;
    let mut opts = fetch_options(credentials);
    remote.fetch(&["+refs/heads/*:refs/remotes/origin/*"], Some(&mut opts), None)?;
    Ok(())
}

pub fn checkout_branch(repo_path: &str, branch_name: &str, credentials: Option<&GitCredentials>) -> Result<()> {
    if branch_name.trim().is_empty() {
        return Err(anyhow!("branch name is required"));
    }

    let repo = Repository::open(repo_path)
        .map_err(|e| anyhow!("Not a git repository: {}", e))?;

    let has_local = repo.find_branch(branch_name, git2::BranchType::Local).is_ok();
    if !has_local {
        let _ = fetch_remote(repo_path, credentials);
        let remote_branch = repo
            .find_branch(&format!("origin/{branch_name}"), git2::BranchType::Remote)
            .map_err(|_| anyhow!("Branch not found locally or on origin: {}", branch_name))?;
        let target = remote_branch
            .get()
            .target()
            .ok_or_else(|| anyhow!("Remote branch has no target commit"))?;
        let commit = repo.find_commit(target)?;
        repo.branch(branch_name, &commit, false)?;
    }

    let obj = repo.revparse_single(&format!("refs/heads/{branch_name}"))?;
    repo.checkout_tree(&obj, None)?;
    repo.set_head(&format!("refs/heads/{branch_name}"))?;
    Ok(())
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
    let repo = Repository::open(repo_path)
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

pub async fn list_remote_branches(url: &str, credentials: Option<&GitCredentials>) -> Result<Vec<String>> {
    if url.contains("github.com") {
        list_github_branches(url, credentials).await
    } else if url.contains("gitlab.com") {
        list_gitlab_branches(url, credentials).await
    } else {
        Ok(vec![])
    }
}

async fn list_github_branches(url: &str, credentials: Option<&GitCredentials>) -> Result<Vec<String>> {
    let (owner, repo) = parse_repo_slug(url)?;
    let mut req = reqwest::Client::new()
        .get(format!(
            "https://api.github.com/repos/{}/{}/branches?per_page=100",
            owner, repo
        ))
        .header("User-Agent", "kway-dev-platform/1.0")
        .header("Accept", "application/vnd.github.v3+json");

    if let Some(creds) = credentials {
        req = req.bearer_auth(&creds.access_token);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
        let msg = body["message"].as_str().unwrap_or("").to_string();
        // GitHub returns 404 for private repos when the token lacks access
        // (it deliberately doesn't reveal whether the repo exists). Make
        // that distinction visible to the user.
        let hint = match status.as_u16() {
            404 => Some("repo not found or token lacks access (org SSO authorization may be required for private repos)"),
            403 => Some("token rejected — check scopes or org SSO authorization"),
            401 => Some("token invalid"),
            _ => None,
        };
        return Err(match (msg.is_empty(), hint) {
            (true, None) => anyhow!("GitHub API returned {}", status),
            (true, Some(h)) => anyhow!("GitHub API {} ({})", status, h),
            (false, None) => anyhow!("GitHub API {}: {}", status, msg),
            (false, Some(h)) => anyhow!("GitHub API {}: {} ({})", status, msg, h),
        });
    }

    let data: Vec<serde_json::Value> = resp.json().await?;
    Ok(data
        .iter()
        .filter_map(|b| b["name"].as_str().map(|s| s.to_string()))
        .collect())
}

async fn list_gitlab_branches(url: &str, credentials: Option<&GitCredentials>) -> Result<Vec<String>> {
    let (owner, repo) = parse_repo_slug(url)?;
    let project_path = format!("{}/{}", owner, repo).replace('/', "%2F");
    let mut req = reqwest::Client::new()
        .get(format!(
            "https://gitlab.com/api/v4/projects/{}/repository/branches?per_page=100",
            project_path
        ))
        .header("User-Agent", "kway-dev-platform/1.0");

    if let Some(creds) = credentials {
        req = req.header("PRIVATE-TOKEN", &creds.access_token);
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
        let msg = body["message"].as_str().unwrap_or("").to_string();
        return Err(if msg.is_empty() {
            anyhow!("GitLab API returned {}", status)
        } else {
            anyhow!("GitLab API {}: {}", status, msg)
        });
    }

    let data: Vec<serde_json::Value> = resp.json().await?;
    Ok(data
        .iter()
        .filter_map(|b| b["name"].as_str().map(|s| s.to_string()))
        .collect())
}

fn parse_repo_slug(url: &str) -> Result<(String, String)> {
    let cleaned = url.trim_end_matches('/').trim_end_matches(".git");
    // Handle SSH: git@github.com:owner/repo
    let path = if let Some(colon_pos) = cleaned.rfind(':') {
        let prefix = &cleaned[..colon_pos];
        if prefix.contains('.') && !prefix.contains('/') {
            &cleaned[colon_pos + 1..]
        } else {
            cleaned
        }
    } else {
        cleaned
    };

    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        return Err(anyhow!("Cannot parse owner/repo from URL: {}", url));
    }
    Ok((
        parts[parts.len() - 2].to_string(),
        parts[parts.len() - 1].to_string(),
    ))
}
