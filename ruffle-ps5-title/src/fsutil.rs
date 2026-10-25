//! PS5_RUFFLE_FSUTIL_V1 — writing files so that a crash or a power cut leaves the old version or
//! the new one, never a half-written file; and a writer thread, so a game never waits on the disk.
//!
//! The pattern is Ruffle-Flash-PS5's (src/fsutil.rs, GPL-3.0, by ZiZc3): write beside, sync, rename
//! over, sync the folder; background jobs run in the order they were asked for.
//!
//! 1.000.032: `remove_tree` — a folder and everything in it, the way Delete and a failed fetch
//! remove a game's folder: std's `remove_dir_all` first (on the console it goes through the
//! platform layer's openat / unlinkat / fdopendir), then, if the folder is still there, by paths
//! (read_dir, remove_file, remove_dir: opendir, unlink, rmdir), saying what each answered.
//! On the console (1.000.031 log, 2026-10-25) every removal of a game folder was refused with
//! "Operation not permitted (os error 1)", while renaming worked (each fetched movie is written
//! as `.part` and renamed). So `remove_or_trash` moves what cannot be removed into the games
//! folder's `.ruffle5-trash` (the menu never looks in a folder whose name starts with a dot), and
//! `empty_trash` tries again at each start. `check_removal` tries each call once at start (a
//! file, an empty folder, renames, a tree) and gives one line for the log.
//!
//! 1.000.034: what that line said on the console (1.000.032 log, 2026-10-25, every start): `lstat`
//! is refused ("Operation not permitted") for the games folder, a file and an empty folder made
//! there, and inside a folder made there; `unlink`, `rmdir` and `rename` work. std's
//! `remove_dir_all` starts with `lstat`, so it is refused too. And 1.000.032 asked `lstat` whether
//! a game's folder was there before removing it: refused, it took the folder for gone, removed
//! nothing and said "deleted" (the rescan after each one still counted 77 games). Nothing here asks
//! `lstat` now: a folder's entries are told apart by the type `read_dir` gives (or by whether they
//! can be listed), whether something is there by `stat` — and only "not found" means gone.
//! Checked on a PC with `lstat` refused the same way (a preload library), as the console answers.
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

/// What is at a path, asked without `lstat` (refused on the console).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Missing,
    File,
    Folder,
}

/// `stat`; when that cannot tell (anything but "not found"), what can be listed is a folder and
/// anything else a file: it is there.
fn kind_of(p: &Path) -> Kind {
    match fs::metadata(p) {
        Ok(m) if m.is_dir() => Kind::Folder,
        Ok(_) => Kind::File,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Kind::Missing,
        Err(_) if fs::read_dir(p).is_ok() => Kind::Folder,
        Err(_) => Kind::File,
    }
}

/// Still there: only "not found" says it went.
fn is_there(p: &Path) -> bool {
    kind_of(p) != Kind::Missing
}

/// Removes `dir` and everything in it, by paths: read_dir, remove_file, remove_dir. An entry is a
/// folder when read_dir says so (or, saying nothing, when it can be listed); a link is removed,
/// not followed.
fn remove_by_paths(dir: &Path) -> std::io::Result<()> {
    // each error names the call and the path it was refused for
    let named = |call: &str, p: &Path, e: std::io::Error| std::io::Error::new(e.kind(), format!("{call} {}: {e}", p.display()));
    for entry in fs::read_dir(dir).map_err(|e| named("read_dir", dir, e))? {
        let entry = entry.map_err(|e| named("read_dir", dir, e))?;
        let path = entry.path();
        let folder = match entry.file_type() {
            Ok(t) => t.is_dir(),
            Err(_) => fs::read_dir(&path).is_ok(),
        };
        if folder {
            remove_by_paths(&path)?;
        } else {
            fs::remove_file(&path).map_err(|e| named("unlink", &path, e))?;
        }
    }
    fs::remove_dir(dir).map_err(|e| named("rmdir", dir, e))
}

fn outcome(r: &std::io::Result<()>) -> String {
    match r {
        Ok(()) => "ok".to_string(),
        Err(e) => format!("failed ({e}, {:?})", e.kind()),
    }
}

/// Removes the folder `dir` and everything in it: by paths (1.000.034: first, as std's
/// remove_dir_all asks `lstat`, refused on the console), then std's remove_dir_all if the folder is
/// still there. Err says what each way answered.
pub fn remove_tree(dir: &str) -> Result<(), String> {
    let path = Path::new(dir);
    if !is_there(path) {
        return Ok(());
    }
    let first = remove_by_paths(path);
    if !is_there(path) {
        return Ok(());
    }
    let second = fs::remove_dir_all(path);
    if !is_there(path) {
        crate::say(&format!("files: {dir}: removed by std::fs::remove_dir_all (by paths: {})", outcome(&first)));
        return Ok(());
    }
    Err(format!("by paths {}, std::fs::remove_dir_all {}", outcome(&first), outcome(&second)))
}

/// The folder in a games folder that deleted games wait in when the console refuses removing them.
pub const TRASH: &str = ".ruffle5-trash";

/// What became of a game's folder or file that was deleted.
#[derive(Debug, PartialEq)]
pub enum Removed {
    /// Removed from the disk.
    Gone,
    /// The console refused removing it (the reason): moved into the trash folder (the path).
    Trashed(String, String),
}

/// Removes `path` (a game's folder with everything in it, or a file); if the console refuses,
/// moves it into `root`'s trash folder instead (`root`: the games folder it is in). Err when it
/// is neither removed nor moved, saying what each step answered.
pub fn remove_or_trash(path: &str, root: &str) -> Result<Removed, String> {
    trash_unless_removed(path, root, &remove_any)
}

/// A folder with everything in it (remove_tree), or a file.
fn remove_any(path: &str, is_dir: bool) -> Result<(), String> {
    if is_dir {
        remove_tree(path)
    } else {
        fs::remove_file(path).map_err(|e| format!("unlink {e}"))
    }
}

fn trash_unless_removed(path: &str, root: &str, remove: &dyn Fn(&str, bool) -> Result<(), String>) -> Result<Removed, String> {
    let p = Path::new(path);
    let folder = match kind_of(p) {
        Kind::Missing => return Ok(Removed::Gone),
        k => k == Kind::Folder,
    };
    // gone only when it is no longer found, whatever the removal answered
    let refused = match remove(path, folder) {
        Ok(()) if !is_there(p) => return Ok(Removed::Gone),
        Ok(()) => "removed, yet still there".to_string(),
        Err(e) => e,
    };
    if !is_there(p) {
        return Ok(Removed::Gone);
    }
    let trash = format!("{root}/{TRASH}");
    if let Err(e) = fs::create_dir_all(&trash) {
        return Err(format!("{refused}; making {trash}: {e}"));
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "game".into());
    let mut target = format!("{trash}/{name}");
    let mut n = 2;
    while is_there(Path::new(&target)) {
        target = format!("{trash}/{name} {n}");
        n += 1;
    }
    match fs::rename(p, &target) {
        Ok(()) if !is_there(p) => Ok(Removed::Trashed(target, refused)),
        Ok(()) => Err(format!("{refused}; moved to {target}, yet still there")),
        Err(e) => Err(format!("{refused}; moving it to {trash}: {e}")),
    }
}

/// The bytes in `path` and below.
fn size_of(path: &Path) -> u64 {
    match fs::metadata(path) {
        Ok(m) if m.is_dir() => fs::read_dir(path)
            .map(|it| it.filter_map(|e| e.ok()).map(|e| size_of(&e.path())).sum())
            .unwrap_or(0),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

/// Tries again to remove what waits in `root`'s trash folder. A line for the log when there was
/// something in it: what went, and what is still there and why.
pub fn empty_trash(root: &str) -> Option<String> {
    empty_trash_with(root, &remove_any)
}

fn empty_trash_with(root: &str, remove: &dyn Fn(&str, bool) -> Result<(), String>) -> Option<String> {
    let trash = format!("{root}/{TRASH}");
    let names: Vec<String> = fs::read_dir(&trash).ok()?.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    if names.is_empty() {
        let _ = fs::remove_dir(&trash);
        return None;
    }
    let (mut gone, mut left, mut bytes, mut why) = (0, 0, 0u64, String::new());
    for n in &names {
        let path = format!("{trash}/{n}");
        match remove(&path, kind_of(Path::new(&path)) == Kind::Folder) {
            Ok(()) if !is_there(Path::new(&path)) => gone += 1,
            Ok(()) => {
                left += 1;
                if why.is_empty() {
                    why = "removed, yet still there".into();
                }
            }
            Err(e) => {
                left += 1;
                bytes += size_of(Path::new(&path));
                if why.is_empty() {
                    why = e;
                }
            }
        }
    }
    if left == 0 {
        let _ = fs::remove_dir(&trash);
        return Some(format!("files: emptied {trash} ({gone} deleted game(s) removed)"));
    }
    Some(format!(
        "files: {left} deleted game(s) still in {trash} ({:.1} MB; the console refused removing them: {why}){}",
        bytes as f64 / 1e6,
        if gone > 0 { format!(", {gone} removed") } else { String::new() }
    ))
}

/// What the console answers to each way of removing things in `parent`, once: a file, an empty
/// folder, a rename of each, a folder with a file and a sub-folder in it (std's remove_dir_all,
/// then by paths). One line for the log. Its folder is hidden (a dot name) from the menu.
pub fn check_removal(parent: &str) -> String {
    use std::os::unix::fs::MetadataExt;
    let base = format!("{parent}/.ruffle5-removal-check");
    let owner = |p: &str| match fs::metadata(p) {
        Ok(m) => format!("uid {} mode {:o}", m.uid(), m.mode() & 0o7777),
        Err(e) => format!("stat: {e}"),
    };
    let lstat = match fs::symlink_metadata(parent) {
        Ok(_) => "ok".to_string(),
        Err(e) => format!("refused ({e})"),
    };
    let mut out = vec![format!("files: removing in {parent} ({}; lstat {lstat})", owner(parent))];
    // what an earlier start could not remove is used again
    if let Err(e) = fs::create_dir_all(&base) {
        return format!("{}: cannot make {base}: {e}", out[0]);
    }
    let file = format!("{base}/file.txt");
    match fs::write(&file, b"x") {
        Ok(()) => out.push(format!("a file ({}): unlink {}", owner(&file), outcome(&fs::remove_file(&file)))),
        Err(e) => out.push(format!("a file: cannot write it ({e})")),
    }
    let empty = format!("{base}/empty");
    let made = fs::create_dir(&empty).or_else(|e| if Path::new(&empty).is_dir() { Ok(()) } else { Err(e) });
    match made {
        Ok(()) => out.push(format!("an empty folder ({}): rmdir {}", owner(&empty), outcome(&fs::remove_dir(&empty)))),
        Err(e) => out.push(format!("an empty folder: cannot make it ({e})")),
    }
    let (a, b) = (format!("{base}/rename-a"), format!("{base}/rename-b"));
    if fs::write(&a, b"a").is_ok() {
        out.push(format!("rename a file {}", outcome(&fs::rename(&a, &b))));
    }
    let _ = (fs::remove_file(&a), fs::remove_file(&b));
    let (c, d) = (format!("{base}/rename-c"), format!("{base}/rename-d"));
    if fs::create_dir(&c).is_ok() || Path::new(&c).is_dir() {
        out.push(format!("rename a folder {}", outcome(&fs::rename(&c, &d))));
    }
    let _ = (fs::remove_dir(&c), fs::remove_dir(&d));
    let tree = format!("{base}/tree");
    let made = fs::create_dir_all(format!("{tree}/sub"))
        .and_then(|_| fs::write(format!("{tree}/sub/a.txt"), b"a"))
        .and_then(|_| fs::write(format!("{tree}/b.txt"), b"b"));
    match made {
        Err(e) => out.push(format!("a folder with files: cannot make it ({e})")),
        Ok(()) => {
            // as a game's folder is removed (remove_tree: by paths, then std's remove_dir_all)
            let r = remove_tree(&tree);
            out.push(match r {
                Ok(()) => "a folder with files: removed".to_string(),
                Err(e) => format!("a folder with files: {e}, still there"),
            });
        }
    }
    let last = fs::remove_dir(&base);
    if last.is_err() {
        out.push(format!("the check folder: rmdir {} (kept for the next start)", outcome(&last)));
    }
    out.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_is_removed_and_checked() {
        let root = std::env::temp_dir().join(format!("ruffle5-fsutil-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game = root.join("Some Game");
        fs::create_dir_all(game.join("sub/deeper")).unwrap();
        fs::write(game.join("game.swf"), b"FWS").unwrap();
        fs::write(game.join("sub/deeper/x.txt"), b"x").unwrap();
        assert_eq!(remove_tree(game.to_str().unwrap()), Ok(()));
        assert!(!game.exists());
        // a folder that is not there is not an error (nothing to remove)
        assert_eq!(remove_tree(game.to_str().unwrap()), Ok(()));
        // by paths alone, as the fallback does it
        fs::create_dir_all(game.join("sub")).unwrap();
        fs::write(game.join("sub/a"), b"a").unwrap();
        remove_by_paths(&game).unwrap();
        assert!(!game.exists());
        let line = check_removal(root.to_str().unwrap());
        for part in ["a file (uid ", "): unlink ok", "rmdir ok", "rename a file ok", "rename a folder ok", "a folder with files: removed"] {
            assert!(line.contains(part), "{part}: {line}");
        }
        assert!(!root.join(".ruffle5-removal-check").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn what_cannot_be_removed_goes_to_the_trash() {
        let root = std::env::temp_dir().join(format!("ruffle5-trash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let r = root.to_str().unwrap().to_string();
        // removable: gone
        let game = root.join("Game");
        fs::create_dir_all(game.join("sub")).unwrap();
        fs::write(game.join("sub/x"), b"x").unwrap();
        assert_eq!(remove_or_trash(game.to_str().unwrap(), &r), Ok(Removed::Gone));
        assert!(!game.exists());
        let movie = root.join("m.swf");
        fs::write(&movie, b"FWS").unwrap();
        assert_eq!(remove_or_trash(movie.to_str().unwrap(), &r), Ok(Removed::Gone));
        // not there: nothing to do
        assert_eq!(remove_or_trash(movie.to_str().unwrap(), &r), Ok(Removed::Gone));
        // what the console refuses (as it did every removal in the 1.000.031 log): moved instead
        let refuse = |_: &str, _: bool| -> Result<(), String> { Err("unlink Operation not permitted (os error 1)".into()) };
        let stuck = root.join("Stuck");
        fs::create_dir_all(stuck.join("sub")).unwrap();
        fs::write(stuck.join("sub/f"), b"f").unwrap();
        let s1 = stuck.to_str().unwrap();
        match trash_unless_removed(s1, &r, &refuse) {
            Ok(Removed::Trashed(to, why)) => {
                assert_eq!(to, format!("{r}/{TRASH}/Stuck"));
                assert!(Path::new(&to).join("sub/f").exists() && !stuck.exists());
                assert!(why.contains("Operation not permitted"), "{why}");
            }
            other => panic!("{other:?}"),
        }
        // a second one of the same name gets its own place; a file goes the same way
        fs::create_dir_all(&stuck).unwrap();
        assert!(matches!(trash_unless_removed(s1, &r, &refuse), Ok(Removed::Trashed(t, _)) if t.ends_with("/Stuck 2")));
        fs::write(&movie, b"FWS").unwrap();
        assert!(matches!(trash_unless_removed(movie.to_str().unwrap(), &r, &refuse), Ok(Removed::Trashed(t, _)) if t.ends_with("/m.swf")));
        // the menu does not list the trash (a dot folder)
        assert!(TRASH.starts_with('.'));
        // emptying while still refused: said, and kept
        let line = empty_trash_with(&r, &refuse).unwrap();
        assert!(line.contains("3 deleted game(s) still in") && line.contains("Operation not permitted"), "{line}");
        assert!(root.join(TRASH).join("Stuck 2").exists());
        // once removing works, the trash is emptied and goes
        if root.join(TRASH).exists() {
            let line = empty_trash(&r).unwrap();
            assert!(line.contains("emptied"), "{line}");
        }
        assert!(!root.join(TRASH).exists());
        assert_eq!(empty_trash(&r), None);
        let _ = fs::remove_dir_all(&root);
    }
}
