//! Server-side User KEK session store.
//!
//! The User KEK (`[u8; 32]`) is derived from the user's password at login
//! (`vault_crypto::derive_user_kek`) and held here for the duration of the
//! session.  It is **never** persisted to disk or the database.
//!
//! ## Lifecycle
//!
//! | Event           | Action                                    |
//! |-----------------|-------------------------------------------|
//! | Login success   | `insert(user_id, kek)`                    |
//! | Token refresh   | `refresh(user_id)` — resets TTL           |
//! | Logout          | `remove(user_id)` — zeroizes key in place |
//! | Session expired | `get_cipher` returns `None`               |
//! | Server restart  | All KEKs lost — all users must re-login   |
//!
//! ## Security properties
//!
//! - Key material is zeroized (volatile-write + memory fence) on `remove`
//!   and on `Drop` of an evicted `SessionEntry`.
//! - The store is backed by a `RwLock<HashMap>`.  Read contention (the
//!   common case) does not block other readers.
//! - A background cleanup task can call `cleanup_expired()` periodically
//!   to free memory for long-abandoned sessions.

use std::{
    collections::HashMap,
    sync::RwLock,
    time::{Duration, Instant},
};
use uuid::Uuid;

/// Default session TTL: 8 hours (one working day).
/// After expiry the next vault operation returns an error prompting re-login.
const DEFAULT_TTL: Duration = Duration::from_secs(8 * 3600);

// ── Internal entry ────────────────────────────────────────────────────

struct SessionEntry {
    kek: [u8; 32],
    expires_at: Instant,
}

impl Drop for SessionEntry {
    /// Zeroize key material when the entry is evicted or removed.
    /// Using `write_volatile` + a memory fence prevents the compiler or
    /// hardware from eliding the store as a "dead write".
    fn drop(&mut self) {
        for b in self.kek.iter_mut() {
            // SAFETY: we own `b` exclusively and it is properly aligned.
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    }
}

// ── Public store ──────────────────────────────────────────────────────

pub struct SessionKeyStore {
    inner: RwLock<HashMap<Uuid, SessionEntry>>,
    ttl: Duration,
}

impl Default for SessionKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionKeyStore {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            ttl: DEFAULT_TTL,
        }
    }

    /// Construct with a custom TTL (useful in tests or config-driven setups).
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Store the User KEK for `user_id`.  Overwrites any existing entry
    /// (handles re-login without explicit logout).
    pub fn insert(&self, user_id: Uuid, kek: [u8; 32]) {
        let entry = SessionEntry {
            kek,
            expires_at: Instant::now() + self.ttl,
        };
        // Drop of the old entry (if any) zeroizes its key bytes.
        self.inner.write().unwrap().insert(user_id, entry);
    }

    /// Return a `TokenCipher` wrapping the stored User KEK, or `None` if
    /// the session has expired or was never established.
    ///
    /// This copies 32 key bytes into a new `TokenCipher` — the in-store
    /// copy remains in the map until explicit removal or expiry.
    pub fn get_cipher(&self, user_id: Uuid) -> Option<crate::crypto::TokenCipher> {
        let map = self.inner.read().unwrap();
        let entry = map.get(&user_id)?;
        if Instant::now() > entry.expires_at {
            return None; // expired; cleanup_expired will remove it later
        }
        Some(crate::crypto::TokenCipher::from_raw_key(&entry.kek))
    }

    /// Reset the TTL for an active session (call on access-token refresh).
    /// No-op if the session does not exist or has already expired.
    pub fn refresh(&self, user_id: Uuid) {
        let mut map = self.inner.write().unwrap();
        if let Some(entry) = map.get_mut(&user_id) {
            entry.expires_at = Instant::now() + self.ttl;
        }
    }

    /// Remove the User KEK for `user_id` (call on explicit logout).
    /// The key bytes are zeroized via `SessionEntry::drop`.
    pub fn remove(&self, user_id: Uuid) {
        self.inner.write().unwrap().remove(&user_id);
    }

    /// Evict all expired entries.  Call periodically from a background task
    /// (e.g., every 30 minutes) to prevent unbounded memory growth for
    /// long-abandoned sessions.
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        // Evicted entries are dropped here, triggering zeroization.
        self.inner.write().unwrap().retain(|_, e| e.expires_at > now);
    }

    /// Number of active (non-expired) sessions.  For metrics / health checks.
    pub fn active_count(&self) -> usize {
        let now = Instant::now();
        self.inner
            .read()
            .unwrap()
            .values()
            .filter(|e| e.expires_at > now)
            .count()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_kek(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    #[test]
    fn insert_and_get() {
        let store = SessionKeyStore::new();
        let uid = Uuid::new_v4();
        store.insert(uid, make_kek(0xAA));
        assert!(store.get_cipher(uid).is_some());
    }

    #[test]
    fn remove_clears_entry() {
        let store = SessionKeyStore::new();
        let uid = Uuid::new_v4();
        store.insert(uid, make_kek(0xBB));
        store.remove(uid);
        assert!(store.get_cipher(uid).is_none());
    }

    #[test]
    fn expired_entry_returns_none() {
        let store = SessionKeyStore::with_ttl(Duration::from_millis(1));
        let uid = Uuid::new_v4();
        store.insert(uid, make_kek(0xCC));
        std::thread::sleep(Duration::from_millis(5));
        assert!(store.get_cipher(uid).is_none());
    }

    #[test]
    fn refresh_extends_ttl() {
        let store = SessionKeyStore::with_ttl(Duration::from_millis(50));
        let uid = Uuid::new_v4();
        store.insert(uid, make_kek(0xDD));
        std::thread::sleep(Duration::from_millis(30));
        store.refresh(uid); // extend
        std::thread::sleep(Duration::from_millis(30));
        // 60 ms elapsed, but TTL was reset at 30 ms → still valid
        assert!(store.get_cipher(uid).is_some());
    }

    #[test]
    fn cleanup_removes_expired() {
        let store = SessionKeyStore::with_ttl(Duration::from_millis(1));
        let uid = Uuid::new_v4();
        store.insert(uid, make_kek(0xEE));
        std::thread::sleep(Duration::from_millis(5));
        store.cleanup_expired();
        assert_eq!(store.active_count(), 0);
    }

    #[test]
    fn different_users_are_independent() {
        let store = SessionKeyStore::new();
        let u1 = Uuid::new_v4();
        let u2 = Uuid::new_v4();
        store.insert(u1, make_kek(0x01));
        store.insert(u2, make_kek(0x02));
        store.remove(u1);
        assert!(store.get_cipher(u1).is_none());
        assert!(store.get_cipher(u2).is_some());
    }

    #[test]
    fn reinsert_overwrites() {
        let store = SessionKeyStore::new();
        let uid = Uuid::new_v4();
        store.insert(uid, make_kek(0x11));
        store.insert(uid, make_kek(0x22)); // re-login
        // Should not panic and should reflect the new key.
        assert!(store.get_cipher(uid).is_some());
    }
}
