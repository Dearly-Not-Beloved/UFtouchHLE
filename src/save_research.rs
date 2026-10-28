/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Instrumentation for researching MHP2G's save format: dumps of the save
//! data, of the buffers around its encryption and of memory accesses to them,
//! written to [DUMP_DIR] for the tools in `mhp2g-save-tools/`.
//!
//! It's off unless `--mhp2g-save-research` is given: the hooks slow down
//! every app, and the folder doesn't exist on Android. Writing a dump never
//! panics; a failure is logged instead.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub const DUMP_DIR: &str = "/tmp/touchhle_dump";

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether the hooks should run. This is checked on hot paths (e.g. every
/// guest memory access), so it's just an atomic load.
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// The path of a dump file, creating [DUMP_DIR] if needed.
fn dump_path(name: &str) -> Option<PathBuf> {
    if let Err(e) = std::fs::create_dir_all(DUMP_DIR) {
        log!("Couldn't create {}: {}", DUMP_DIR, e);
        return None;
    }
    Some(PathBuf::from(DUMP_DIR).join(name))
}

/// Writes a dump file, replacing any previous one.
pub fn write(name: &str, data: &[u8]) {
    if let Some(path) = dump_path(name) {
        if let Err(e) = std::fs::write(&path, data) {
            log!("Couldn't write {}: {}", path.display(), e);
        }
    }
}

/// Appends to a dump file.
pub fn append(name: &str, data: &[u8]) {
    let Some(path) = dump_path(name) else {
        return;
    };
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| file.write_all(data));
    if let Err(e) = result {
        log!("Couldn't append to {}: {}", path.display(), e);
    }
}
