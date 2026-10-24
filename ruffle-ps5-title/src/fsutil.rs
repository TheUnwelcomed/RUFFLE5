//! PS5_RUFFLE_FSUTIL_V1 — writing files so that a crash or a power cut leaves the old version or
//! the new one, never a half-written file; and a writer thread, so a game never waits on the disk.
//!
//! The pattern is Ruffle-Flash-PS5's (src/fsutil.rs, GPL-3.0, by ZiZc3): write beside, sync, rename
//! over, sync the folder; background jobs run in the order they were asked for.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;

/// Writes beside the file, syncs, renames over it and syncs the folder. Creates the folder.
pub fn write_durably(path: impl AsRef<Path>, body: impl AsRef<[u8]>) -> std::io::Result<()> {
    let path = path.as_ref();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut f = File::create(&tmp)?;
    f.write_all(body.as_ref())?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        let _ = File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

type Job = (PathBuf, Option<Vec<u8>>);
static WRITER: Mutex<Option<Sender<Job>>> = Mutex::new(None);

/// The same, on the writer thread.
pub fn write_in_background(path: impl Into<PathBuf>, body: Vec<u8>) {
    send(path.into(), Some(body));
}

/// Removes a file on the writer thread, after any write to it still waiting there.
pub fn remove_in_background(path: impl Into<PathBuf>) {
    send(path.into(), None);
}

fn send(path: PathBuf, body: Option<Vec<u8>>) {
    let mut writer = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    if writer.is_none() {
        let (tx, rx) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new().name("writer".into()).spawn(move || {
            for (path, body) in rx {
                match body {
                    None => {
                        let _ = fs::remove_file(&path);
                    }
                    Some(body) => {
                        if let Err(e) = write_durably(&path, &body) {
                            crate::say(&format!("warn cannot write {}: {e}", path.display()));
                        }
                    }
                }
            }
        });
        match spawned {
            Ok(_) => *writer = Some(tx),
            Err(e) => {
                // No thread: do it here and now.
                crate::say(&format!("warn writer thread: {e}"));
                drop(writer);
                match body {
                    None => {
                        let _ = fs::remove_file(&path);
                    }
                    Some(body) => {
                        let _ = write_durably(&path, &body);
                    }
                }
                return;
            }
        }
    }
    if let Some(tx) = writer.as_ref() {
        let _ = tx.send((path, body));
    }
}

/// Moves `from` to `to` (rename, or copy and remove across file systems). False if it did not.
pub fn move_file(from: &str, to: &str) -> bool {
    if let Some(dir) = Path::new(to).parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::rename(from, to).is_ok() {
        return true;
    }
    match fs::copy(from, to) {
        Ok(_) => {
            let _ = fs::remove_file(from);
            true
        }
        Err(_) => false,
    }
}
