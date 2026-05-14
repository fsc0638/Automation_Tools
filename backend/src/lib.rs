//! Library-side modules of kway-dev-backend.
//!
//! Anything reusable across binaries goes here. The main server (main.rs)
//! plus auxiliary tools under bin/ both consume these modules, which keeps
//! us from duplicating SQL queries or subprocess logic between executables.
//!
//! Modules of the server binary itself (api, db, config, …) stay in
//! main.rs's tree — they don't need to be shared and lifting them up here
//! would be a wide refactor for no win.

pub mod portal_book;
pub mod portal_directory_import;
pub mod portal_import;
pub mod portal_sync;
