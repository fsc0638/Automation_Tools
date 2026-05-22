//! Per-user encrypted disk images (macOS only).
//!
//! ## Design
//!
//! Each Kway user gets a personal APFS-encrypted sparse image stored at
//! `<DMG_ROOT>/<user_id>.sparseimage`.  The image is mounted at
//! `<PROJECT_DATA_ROOT>/users/<user_id>/` — the same directory that
//! `projects.rs` and `meetings.rs` write project/meeting files into, so
//! those files land inside the encrypted container automatically.
//!
//! The 32-byte random passphrase is stored in the Kway vault as
//! `object_type = "user_dmg_key"`, `object_id = <user_id>`.  It is
//! wrapped by both the user's KEK and the system recovery KEK, so it
//! survives a user password reset (via system recovery) and is purged
//! when the user account is deleted.
//!
//! ## Lifecycle
//!
//! | Event          | Action                                        |
//! |----------------|-----------------------------------------------|
//! | First login    | generate key → seal in vault → create image → mount |
//! | Subsequent login | open key from vault → mount (idempotent)   |
//! | Logout         | detach image (best-effort)                    |
//! | Password change | no-op (vault wrapping re-keyed automatically) |
//! | Account delete | purge vault entry + delete `.sparseimage` file |
//!
//! ## macOS compatibility
//!
//! Requires macOS 10.13+ (APFS + `hdiutil` with `-stdinpass`).
//! The Mac Mini used in production runs macOS 14 (Sonoma) — no issue.
//!
//! ## Non-macOS builds
//!
//! All public functions are compiled to no-ops on Linux / Windows.
//! This keeps `auth.rs` clean — no `#[cfg]` guards at the call site.

use anyhow::Result;
use std::path::PathBuf;
use uuid::Uuid;

// ── Path helpers (used on all platforms) ─────────────────────────────────────

/// Canonical path for the per-user sparse image file.
///
/// `hdiutil create -type SPARSE` appends `.sparseimage`; we match that here so
/// callers always get a consistent path regardless of platform.
pub fn image_path(dmg_root: &str, user_id: Uuid) -> PathBuf {
    PathBuf::from(dmg_root).join(format!("{user_id}.sparseimage"))
}

/// Canonical mount-point for the per-user encrypted volume.
/// This is also the root of all on-disk project/meeting files for this user.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn mount_point(data_root: &str, user_id: Uuid) -> PathBuf {
    PathBuf::from(data_root)
        .join("users")
        .join(user_id.to_string())
}

// ── macOS implementation ──────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use anyhow::{anyhow, Context};
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    /// Convert raw key bytes to a printable passphrase for `hdiutil`.
    /// We base64-encode the 32-byte key so it's 44 printable ASCII characters —
    /// safe to pipe via stdin and unambiguous across locales.
    fn passphrase(raw_key: &[u8]) -> String {
        B64.encode(raw_key)
    }

    /// Create a new APFS-encrypted sparse image for `user_id`.
    ///
    /// - Writes `<dmg_root>/<user_id>.sparseimage`
    /// - Capacity capped at `size_mb` MiB (sparse = no pre-allocation)
    /// - Mount point directory is created but the image is NOT mounted here;
    ///   call `mount()` immediately after.
    pub fn create(dmg_root: &str, size_mb: u32, user_id: Uuid, raw_key: &[u8]) -> Result<()> {
        std::fs::create_dir_all(dmg_root)
            .with_context(|| format!("create dmg_root {dmg_root}"))?;

        // `hdiutil create` with a path that has no extension: it appends
        // `.sparseimage` for -type SPARSE.  The resulting file matches
        // `image_path()`.
        let stem = PathBuf::from(dmg_root).join(user_id.to_string());
        let stem_str = stem.to_string_lossy();
        let size_arg = format!("{size_mb}m");
        let volname = format!("kway-{}", &user_id.to_string()[..8]);

        let mut child = Command::new("hdiutil")
            .args([
                "create",
                "-type", "SPARSE",
                "-size", &size_arg,
                "-fs", "APFS",
                "-encryption", "AES-256",
                "-stdinpass",
                "-volname", &volname,
                stem_str.as_ref(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("hdiutil create: spawn")?;

        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "{}", passphrase(raw_key))?;
        }

        let out = child.wait_with_output().context("hdiutil create: wait")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow!(
                "hdiutil create failed for {user_id}: {stderr}"
            ));
        }
        Ok(())
    }

    /// Mount the sparse image at `<data_root>/users/<user_id>/`.
    /// Idempotent — returns `Ok(())` if already mounted.
    pub fn mount(dmg_root: &str, data_root: &str, user_id: Uuid, raw_key: &[u8]) -> Result<()> {
        if is_mounted(data_root, user_id) {
            return Ok(());
        }

        let img = image_path(dmg_root, user_id);
        let mp = mount_point(data_root, user_id);
        std::fs::create_dir_all(&mp)
            .with_context(|| format!("create mount point {}", mp.display()))?;

        let mp_str = mp.to_string_lossy();

        let mut child = Command::new("hdiutil")
            .args([
                "attach",
                "-stdinpass",
                "-mountpoint", mp_str.as_ref(),
                "-nobrowse",  // don't show in Finder sidebar
                img.to_string_lossy().as_ref(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("hdiutil attach: spawn")?;

        if let Some(mut stdin) = child.stdin.take() {
            writeln!(stdin, "{}", passphrase(raw_key))?;
        }

        let out = child.wait_with_output().context("hdiutil attach: wait")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow!(
                "hdiutil attach failed for {user_id}: {stderr}"
            ));
        }
        Ok(())
    }

    /// Detach the sparse image for `user_id`.  Best-effort — if it's already
    /// unmounted `Ok(())` is returned immediately.  If graceful detach fails
    /// (open files) we retry once with `-force`.
    pub fn unmount(data_root: &str, user_id: Uuid) -> Result<()> {
        if !is_mounted(data_root, user_id) {
            return Ok(());
        }

        let mp = mount_point(data_root, user_id);
        let mp_str = mp.to_string_lossy();

        let status = Command::new("hdiutil")
            .args(["detach", mp_str.as_ref()])
            .status()
            .context("hdiutil detach: spawn")?;

        if !status.success() {
            // Retry with -force (may leave open file handles in a bad state,
            // but better than leaving the image permanently mounted after logout).
            tracing::warn!(
                user_id = %user_id,
                "hdiutil detach failed — retrying with -force"
            );
            let _ = Command::new("hdiutil")
                .args(["detach", "-force", mp_str.as_ref()])
                .status();
        }
        Ok(())
    }

    /// Returns `true` if the mount-point directory is an active hdiutil mount.
    ///
    /// Checks the kernel mount table via `/bin/mount` (fast, no hdiutil fork).
    fn is_mounted(data_root: &str, user_id: Uuid) -> bool {
        let mp = mount_point(data_root, user_id);
        let mp_str = mp.to_string_lossy();
        Command::new("/bin/mount")
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .contains(mp_str.as_ref())
            })
            .unwrap_or(false)
    }
}

// ── Non-macOS no-ops ──────────────────────────────────────────────────────────

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    pub fn create(_dmg_root: &str, _size_mb: u32, _user_id: Uuid, _raw_key: &[u8]) -> Result<()> {
        Ok(())
    }
    pub fn mount(
        _dmg_root: &str,
        _data_root: &str,
        _user_id: Uuid,
        _raw_key: &[u8],
    ) -> Result<()> {
        Ok(())
    }
    pub fn unmount(_data_root: &str, _user_id: Uuid) -> Result<()> {
        Ok(())
    }
}

// ── Public surface (delegates to platform mod) ────────────────────────────────

/// Create a new encrypted sparse image for `user_id`.
/// Blocking — run inside `tokio::task::spawn_blocking` from async code.
pub fn create(dmg_root: &str, size_mb: u32, user_id: Uuid, raw_key: &[u8]) -> Result<()> {
    platform::create(dmg_root, size_mb, user_id, raw_key)
}

/// Mount the encrypted image at `<data_root>/users/<user_id>/`.
/// Idempotent.  Blocking — run inside `tokio::task::spawn_blocking`.
pub fn mount(dmg_root: &str, data_root: &str, user_id: Uuid, raw_key: &[u8]) -> Result<()> {
    platform::mount(dmg_root, data_root, user_id, raw_key)
}

/// Detach the image.  Best-effort.  Blocking.
pub fn unmount(data_root: &str, user_id: Uuid) -> Result<()> {
    platform::unmount(data_root, user_id)
}

/// Returns `true` if the sparse image file exists on disk.
/// Works on all platforms (image_path is platform-independent).
pub fn image_exists(dmg_root: &str, user_id: Uuid) -> bool {
    image_path(dmg_root, user_id).exists()
}
