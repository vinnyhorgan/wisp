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

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use notify::event::ModifyKind;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// queue cap between the backend thread and `poll()`: a mass churn (a
/// branch switch, a build) must not grow memory without bound on a slow
/// poller. overflow collapses into a single "rescan" -- the honest
/// answer to more activity than anyone wants itemized
const QUEUE_CAP: usize = 4096;

pub struct Change {
    pub kind: &'static str,
    pub path: PathBuf,
}

pub struct Watch {
    root: PathBuf,
    /// kept alive for the stream; dropping it stops the backend thread
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<notify::Result<notify::Event>>,
    /// the backend hit a full queue; latched until the next poll
    overflowed: Arc<AtomicBool>,
    /// the backend died; reported as one final "rescan", then silence
    dead: bool,
}

impl Watch {
    /// watches `root` and everything under it
    pub fn open(root: &Path) -> Result<Watch, String> {
        Watch::open_with_cap(root, QUEUE_CAP)
    }

    fn open_with_cap(root: &Path, cap: usize) -> Result<Watch, String> {
        let (tx, rx) = mpsc::sync_channel(cap);
        let overflowed = Arc::new(AtomicBool::new(false));
        let flag = overflowed.clone();
        let mut watcher = notify::recommended_watcher(move |res| {
            // never block the backend thread: on a full queue, drop the
            // event and latch the flag instead
            if let Err(mpsc::TrySendError::Full(_)) = tx.try_send(res) {
                flag.store(true, Ordering::Relaxed);
            }
        })
        .map_err(|err| format!("failed to watch: {err}"))?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|err| format!("failed to watch: {err}"))?;
        Ok(Watch {
            root: root.to_owned(),
            _watcher: watcher,
            rx,
            overflowed,
            dead: false,
        })
    }

    /// everything that happened since the last poll; never blocks
    pub fn poll(&mut self) -> Vec<Change> {
        let mut out: Vec<Change> = Vec::new();
        // fs activity comes in bursts of duplicates; keep one of each
        let mut seen: HashSet<(&'static str, PathBuf)> = HashSet::new();
        let mut push = |out: &mut Vec<Change>, kind: &'static str, path: PathBuf| {
            if seen.insert((kind, path.clone())) {
                out.push(Change { kind, path });
            }
        };
        if self.overflowed.swap(false, Ordering::Relaxed) {
            push(&mut out, "rescan", self.root.clone());
        }
        loop {
            let res = match self.rx.try_recv() {
                Ok(res) => res,
                Err(mpsc::TryRecvError::Empty) => break,
                // the backend thread is gone: whatever happens on disk
                // from here on is invisible. say "rescan" once so the
                // consumer falls back to scanning, then stay quiet
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.dead {
                        self.dead = true;
                        push(&mut out, "rescan", self.root.clone());
                    }
                    break;
                }
            };
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

    #[test]
    fn a_flood_overflows_into_a_single_rescan() {
        // more events than the queue holds must not grow memory or get
        // lost silently: the overflow is reported as a rescan
        let dir = scratch("flood");
        let mut w = Watch::open_with_cap(&dir, 4).unwrap();
        for i in 0..50 {
            std::fs::write(dir.join(format!("f{i}")), b"x").unwrap();
        }
        assert!(
            poll_until(&mut w, 5000, |seen| seen.iter().any(|c| c.kind == "rescan")),
            "queue overflow never reported a rescan"
        );
    }

    #[test]
    fn a_dead_backend_says_rescan_once_then_stays_quiet() {
        let dir = scratch("dead");
        let mut w = Watch::open(&dir).unwrap();
        // sever the channel the way a dying backend thread would: by
        // dropping the sender side
        let (_, rx) = mpsc::sync_channel(1);
        w.rx = rx;
        let first = w.poll();
        assert!(
            first.iter().any(|c| c.kind == "rescan"),
            "backend death went silent instead of saying rescan"
        );
        assert!(w.poll().is_empty(), "the final rescan repeated");
    }

    #[test]
    fn deleting_the_watched_root_is_reported() {
        let dir = scratch("gone");
        let mut w = Watch::open(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        // the exact shape is backend-specific (a delete of the root or
        // a rescan); what matters is that it is not silence
        assert!(
            poll_until(&mut w, 5000, |seen| {
                seen.iter()
                    .any(|c| c.kind == "delete" || c.kind == "rescan")
            }),
            "root deletion went unreported"
        );
    }
}
