//! the data/ tree, baked into the desktop binary.
//!
//! a wisp binary with no data/ beside it (a single installed file)
//! unpacks its embedded copy into the user data dir and runs from there.
//! real files, deliberately: the unpacked tree stays as live-editable as
//! a checkout, and no fs api has to know embedded files exist. core
//! files are unpacked again whenever the version changes; user/ belongs
//! to the user and an existing file there is never overwritten.

use include_dir::{Dir, include_dir};
use std::path::{Path, PathBuf};

static DATA: Dir = include_dir!("$CARGO_MANIFEST_DIR/data");

/// unpack into $XDG_DATA_HOME/wisp (~/.local/share/wisp by default) and
/// return the directory that then contains data/
pub fn unpack() -> std::io::Result<PathBuf> {
    let root = data_home()?.join("wisp");
    unpack_into(&root)?;
    Ok(root)
}

fn data_home() -> std::io::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME")
        // the xdg spec: a relative value is invalid and must be ignored
        && !dir.is_empty()
        && Path::new(&dir).is_absolute()
    {
        return Ok(dir.into());
    }
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(".local/share")),
        _ => Err(std::io::Error::other("no home directory")),
    }
}

fn unpack_into(root: &Path) -> std::io::Result<()> {
    let stamp = root.join("version");
    // a current stamp means hands off, even if the tree was edited or
    // damaged by hand: repairing would clobber live edits, and deleting
    // the version file is the explicit way to ask for a fresh tree
    if std::fs::read_to_string(&stamp).is_ok_and(|v| v == env!("CARGO_PKG_VERSION")) {
        return Ok(());
    }
    // prune before writing: plugins autoload by directory listing, so a
    // file removed from data/ upstream must not keep loading forever
    // from the unpacked tree. user/ belongs to the user and stays
    prune(&root.join("data"))?;
    write_dir(&DATA, &root.join("data"))?;
    write_file(&stamp, env!("CARGO_PKG_VERSION").as_bytes())
}

fn prune(data: &Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(data) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name() == "user" {
            continue;
        }
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// write-then-rename: anyone racing the unpack (a second first-run
/// instance) sees the old file or the new one, never a torn half
fn write_file(target: &Path, contents: &[u8]) -> std::io::Result<()> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let tmp = target.with_file_name(format!("{name}.wisp-tmp"));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, target)
}

fn write_dir(dir: &Dir, root: &Path) -> std::io::Result<()> {
    for d in dir.dirs() {
        write_dir(d, root)?;
    }
    for f in dir.files() {
        let target = root.join(f.path());
        // user/ belongs to the user: never overwrite an existing file
        if f.path().starts_with("user") && target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_file(&target, f.contents())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wisp-embed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn unpacking_writes_the_whole_tree() {
        let root = scratch("tree");
        unpack_into(&root).unwrap();
        assert!(root.join("data/core/init.lua").is_file());
        assert!(root.join("data/plugins/treeview.lua").is_file());
        assert!(root.join("data/user/init.lua").is_file());
        let font = std::fs::metadata(root.join("data/jetbrainsmono.ttf")).unwrap();
        assert!(font.len() > 1_000_000, "the font came out truncated");
    }

    #[test]
    fn a_version_change_refreshes_core_but_never_user() {
        let root = scratch("upgrade");
        unpack_into(&root).unwrap();
        std::fs::write(root.join("data/core/init.lua"), "hacked\n").unwrap();
        std::fs::write(root.join("data/user/init.lua"), "mine\n").unwrap();
        std::fs::write(root.join("version"), "0.0.0-old").unwrap();
        unpack_into(&root).unwrap();
        let core = std::fs::read_to_string(root.join("data/core/init.lua")).unwrap();
        let user = std::fs::read_to_string(root.join("data/user/init.lua")).unwrap();
        assert_ne!(core, "hacked\n", "a stale core file survived the upgrade");
        assert_eq!(user, "mine\n", "the upgrade clobbered user/");
    }

    #[test]
    fn an_upgrade_prunes_files_the_new_version_dropped() {
        // plugins autoload by directory listing: a plugin removed from
        // data/ upstream used to keep loading from the unpacked tree
        // forever. user/ must survive the prune untouched
        let root = scratch("prune");
        unpack_into(&root).unwrap();
        std::fs::write(root.join("data/plugins/stale.lua"), "gone upstream\n").unwrap();
        std::fs::write(root.join("data/user/mine.lua"), "mine\n").unwrap();
        std::fs::write(root.join("version"), "0.0.0-old").unwrap();
        unpack_into(&root).unwrap();
        assert!(
            !root.join("data/plugins/stale.lua").exists(),
            "a file dropped upstream survived the upgrade"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("data/user/mine.lua")).unwrap(),
            "mine\n",
            "the prune reached into user/"
        );
    }

    #[test]
    fn a_current_stamp_leaves_the_tree_alone() {
        let root = scratch("stamp");
        unpack_into(&root).unwrap();
        std::fs::write(root.join("data/core/init.lua"), "hacked\n").unwrap();
        unpack_into(&root).unwrap();
        let core = std::fs::read_to_string(root.join("data/core/init.lua")).unwrap();
        assert_eq!(core, "hacked\n", "a fresh tree was rewritten anyway");
    }
}
