//! native fs events for the lua layer: know when the project changed
//! instead of rescanning it on a timer.
//!
//! notify's recommended backend (inotify on linux) runs its own thread;
//! that thread stays entirely inside this module. changes queue on a
//! channel and `poll()` drains it -- never blocking, from a lua
//! coroutine, exactly like the process and terminal apis.
//!
//! kinds are a deliberately small language: "create", "modify",
//! "delete", "rename", and "rescan" when events were dropped or too
//! murky to trust -- the consumer's cue to walk the tree again. a
//! rename reports both paths when the backend has them.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub struct Change {
    pub kind: &'static str,
    pub path: PathBuf,
}

pub struct Watch {
    root: PathBuf,
    /// kept alive for the stream; dropping it stops the backend thread
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<notify::Result<notify::Event>>,
}

impl Watch {
    /// watches `root` and everything under it
    pub fn open(root: &Path) -> Result<Watch, String> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })
        .map_err(|err| format!("failed to watch: {err}"))?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|err| format!("failed to watch: {err}"))?;
        Ok(Watch {
            root: root.to_owned(),
            _watcher: watcher,
            rx,
        })
    }

    /// everything that happened since the last poll; never blocks
    pub fn poll(&mut self) -> Vec<Change> {
        let mut out: Vec<Change> = Vec::new();
        let push = |out: &mut Vec<Change>, kind: &'static str, path: PathBuf| {
            // fs activity comes in bursts of duplicates; keep one
            if !out.iter().any(|c| c.kind == kind && c.path == path) {
                out.push(Change { kind, path });
            }
        };
        while let Ok(res) = self.rx.try_recv() {
            let event = match res {
                Ok(event) => event,
                // a backend error means changes may have been missed
                Err(_) => {
                    push(&mut out, "rescan", self.root.clone());
                    continue;
                }
            };
            if event.need_rescan() {
                push(&mut out, "rescan", self.root.clone());
                continue;
            }
            let kind = match event.kind {
                EventKind::Create(_) => "create",
                EventKind::Remove(_) => "delete",
                EventKind::Modify(ModifyKind::Name(_)) => "rename",
                EventKind::Modify(_) => "modify",
                EventKind::Access(_) => continue,
                // the backend saw something it cannot classify
                EventKind::Any | EventKind::Other => {
                    push(&mut out, "rescan", self.root.clone());
                    continue;
                }
            };
            for path in event.paths {
                push(&mut out, kind, path);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wisp-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// fs events are asynchronous even on a local disk: poll on real
    /// time until the expected change shows up
    fn poll_until(w: &mut Watch, ms: u64, mut done: impl FnMut(&[Change]) -> bool) -> bool {
        let start = std::time::Instant::now();
        let mut seen = Vec::new();
        while start.elapsed().as_millis() < ms as u128 {
            seen.extend(w.poll());
            if done(&seen) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    fn saw(seen: &[Change], kind: &str, name: &str) -> bool {
        seen.iter()
            .any(|c| c.kind == kind && c.path.to_string_lossy().contains(name))
    }

    #[test]
    fn a_created_file_is_reported() {
        let dir = scratch("create");
        let mut w = Watch::open(&dir).unwrap();
        std::fs::write(dir.join("born.txt"), b"hi").unwrap();
        assert!(
            poll_until(&mut w, 5000, |seen| saw(seen, "create", "born.txt")),
            "creation never reported"
        );
    }

    #[test]
    fn writes_and_deletes_are_reported_with_their_paths() {
        let dir = scratch("touch");
        let file = dir.join("noted.txt");
        std::fs::write(&file, b"one").unwrap();
        let mut w = Watch::open(&dir).unwrap();
        std::fs::write(&file, b"two").unwrap();
        assert!(
            poll_until(&mut w, 5000, |seen| saw(seen, "modify", "noted.txt")),
            "write never reported"
        );
        std::fs::remove_file(&file).unwrap();
        assert!(
            poll_until(&mut w, 5000, |seen| saw(seen, "delete", "noted.txt")),
            "deletion never reported"
        );
    }

    #[test]
    fn renames_report_both_ends() {
        let dir = scratch("rename");
        std::fs::write(dir.join("old.txt"), b"x").unwrap();
        let mut w = Watch::open(&dir).unwrap();
        std::fs::rename(dir.join("old.txt"), dir.join("new.txt")).unwrap();
        assert!(
            poll_until(&mut w, 5000, |seen| {
                saw(seen, "rename", "old.txt") && saw(seen, "rename", "new.txt")
            }),
            "rename endpoints never reported"
        );
    }

    #[test]
    fn events_reach_below_new_subdirectories() {
        // recursive means recursive: a directory created after the
        // watch begins is itself watched
        let dir = scratch("deep");
        let mut w = Watch::open(&dir).unwrap();
        std::fs::create_dir(dir.join("sub")).unwrap();
        assert!(poll_until(&mut w, 5000, |seen| saw(seen, "create", "sub")));
        std::fs::write(dir.join("sub/leaf.txt"), b"x").unwrap();
        assert!(
            poll_until(&mut w, 5000, |seen| saw(seen, "create", "leaf.txt")),
            "no event from inside the new subdirectory"
        );
    }

    #[test]
    fn watching_a_missing_path_is_an_error_not_a_panic() {
        assert!(Watch::open(Path::new("/nonexistent/wisp-void")).is_err());
    }
}
