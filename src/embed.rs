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
        && !dir.is_empty()
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
    if std::fs::read_to_string(&stamp).is_ok_and(|v| v == env!("CARGO_PKG_VERSION")) {
        return Ok(());
    }
    write_dir(&DATA, &root.join("data"))?;
    std::fs::write(&stamp, env!("CARGO_PKG_VERSION"))
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
        std::fs::write(&target, f.contents())?;
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
    fn a_current_stamp_leaves_the_tree_alone() {
        let root = scratch("stamp");
        unpack_into(&root).unwrap();
        std::fs::write(root.join("data/core/init.lua"), "hacked\n").unwrap();
        unpack_into(&root).unwrap();
        let core = std::fs::read_to_string(root.join("data/core/init.lua")).unwrap();
        assert_eq!(core, "hacked\n", "a fresh tree was rewritten anyway");
    }
}
