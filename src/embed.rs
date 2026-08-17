//! the data/ tree, baked into the desktop binary.
//!
//! a wisp binary with no data/ beside it (a single installed file)
//! unpacks its embedded copy into the user data dir and runs from there.
//! real files, deliberately: the unpacked tree stays as live-editable as
//! a checkout, and no fs api has to know embedded files exist. core
//! files are unpacked again whenever the version changes. the user's own
//! files are not here at all: they live in the xdg config dir, which this
//! module also creates and seeds.

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

/// the editor's own config directory: $XDG_CONFIG_HOME/wisp, which is
/// ~/.config/wisp unless the variable says otherwise. this is the obvious
/// place a person looks for their settings, and the obvious place to drop
/// a plugin file
pub fn config_dir() -> std::io::Result<PathBuf> {
    Ok(base_dir("XDG_CONFIG_HOME", ".config")?.join("wisp"))
}

/// $XDG_STATE_HOME/wisp: things the editor writes for itself and the user
/// never edits -- the crash report today, a restored session tomorrow.
/// not config, and not data either
pub fn state_dir() -> std::io::Result<PathBuf> {
    Ok(base_dir("XDG_STATE_HOME", ".local/state")?.join("wisp"))
}

fn data_home() -> std::io::Result<PathBuf> {
    base_dir("XDG_DATA_HOME", ".local/share")
}

fn base_dir(var: &str, fallback: &str) -> std::io::Result<PathBuf> {
    match pick(std::env::var_os(var), std::env::var_os("HOME"), fallback) {
        Some(dir) => Ok(dir),
        None => Err(std::io::Error::other("no home directory")),
    }
}

/// the xdg lookup itself, kept free of the environment so it can be
/// tested: the variable wins when it is set to an absolute path, and the
/// spec is explicit that a relative one is invalid and must be ignored
fn pick(
    var: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    fallback: &str,
) -> Option<PathBuf> {
    if let Some(dir) = var
        && !dir.is_empty()
        && Path::new(&dir).is_absolute()
    {
        return Some(dir.into());
    }
    match home {
        Some(home) if !home.is_empty() => Some(PathBuf::from(home).join(fallback)),
        _ => None,
    }
}

/// create the config directory and seed it, once. an existing init.lua is
/// never touched. `legacy` is where the user module used to live, inside
/// the unpacked tree: an install from before the split keeps its settings
/// by having them moved, not by being ignored
pub fn prepare_config_dir(userdir: &Path, legacy: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(userdir.join("plugins"))?;
    let init = userdir.join("init.lua");
    if init.exists() {
        return Ok(());
    }
    let seed = match std::fs::read(legacy) {
        Ok(contents) => contents,
        Err(_) => match DATA.get_file("user/init.lua") {
            Some(f) => f.contents().to_vec(),
            None => return Ok(()),
        },
    };
    write_file(&init, &seed)
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
        // user/ is not shipped at all any more: it is the seed for the
        // config dir, and unpacking it here would put a second, dead copy
        // of the user module inside the editor's own tree
        if f.path().starts_with("user") {
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
    fn unpacking_writes_the_tree_but_never_the_user_module() {
        let root = scratch("tree");
        unpack_into(&root).unwrap();
        assert!(root.join("data/core/init.lua").is_file());
        assert!(root.join("data/plugins/treeview.lua").is_file());
        // the user module belongs in the config dir; a second copy inside
        // the editor's own tree would be dead the moment it was written
        assert!(!root.join("data/user/init.lua").exists());
        let font = std::fs::metadata(root.join("data/jetbrainsmono.ttf")).unwrap();
        assert!(font.len() > 1_000_000, "the font came out truncated");
    }

    #[test]
    fn a_version_change_refreshes_core_and_spares_a_legacy_user_dir() {
        let root = scratch("upgrade");
        unpack_into(&root).unwrap();
        std::fs::write(root.join("data/core/init.lua"), "hacked\n").unwrap();
        // an install from before the config split still has its settings
        // here, and prepare_config_dir moves them: the upgrade must not
        // delete them out from under it first
        std::fs::create_dir_all(root.join("data/user")).unwrap();
        std::fs::write(root.join("data/user/init.lua"), "mine\n").unwrap();
        std::fs::write(root.join("version"), "0.0.0-old").unwrap();
        unpack_into(&root).unwrap();
        let core = std::fs::read_to_string(root.join("data/core/init.lua")).unwrap();
        let user = std::fs::read_to_string(root.join("data/user/init.lua")).unwrap();
        assert_ne!(core, "hacked\n", "a stale core file survived the upgrade");
        assert_eq!(user, "mine\n", "the upgrade clobbered a legacy user/");
    }

    #[test]
    fn xdg_takes_the_variable_only_when_it_is_absolute() {
        let abs = pick(Some("/tmp/xdg".into()), Some("/home/x".into()), ".config");
        assert_eq!(abs.unwrap(), PathBuf::from("/tmp/xdg"));
        // the spec calls a relative value invalid and says to ignore it
        let rel = pick(Some("relative".into()), Some("/home/x".into()), ".config");
        assert_eq!(rel.unwrap(), PathBuf::from("/home/x/.config"));
        let empty = pick(Some("".into()), Some("/home/x".into()), ".config");
        assert_eq!(empty.unwrap(), PathBuf::from("/home/x/.config"));
        let unset = pick(None, Some("/home/x".into()), ".local/state");
        assert_eq!(unset.unwrap(), PathBuf::from("/home/x/.local/state"));
        assert!(pick(None, None, ".config").is_none());
    }

    #[test]
    fn the_config_dir_is_seeded_once_and_never_clobbered() {
        let root = scratch("config");
        let userdir = root.join("config");
        let legacy = root.join("legacy/init.lua");

        // no legacy install: the seed is the stub baked into the binary
        prepare_config_dir(&userdir, &legacy).unwrap();
        assert!(userdir.join("plugins").is_dir(), "no plugins directory");
        let seeded = std::fs::read_to_string(userdir.join("init.lua")).unwrap();
        assert!(seeded.contains("put user settings here"));

        // and a second run leaves what the user has written alone
        std::fs::write(userdir.join("init.lua"), "mine\n").unwrap();
        prepare_config_dir(&userdir, &legacy).unwrap();
        assert_eq!(
            std::fs::read_to_string(userdir.join("init.lua")).unwrap(),
            "mine\n"
        );
    }

    #[test]
    fn an_install_from_before_the_split_keeps_its_settings() {
        let root = scratch("migrate");
        let userdir = root.join("config");
        let legacy = root.join("data/user/init.lua");
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "-- years of tweaks\n").unwrap();

        prepare_config_dir(&userdir, &legacy).unwrap();
        assert_eq!(
            std::fs::read_to_string(userdir.join("init.lua")).unwrap(),
            "-- years of tweaks\n",
            "the upgrade silently dropped the old user module"
        );
    }

    #[test]
    fn an_upgrade_prunes_files_the_new_version_dropped() {
        // plugins autoload by directory listing: a plugin removed from
        // data/ upstream used to keep loading from the unpacked tree
        // forever. user/ must survive the prune untouched
        let root = scratch("prune");
        unpack_into(&root).unwrap();
        std::fs::write(root.join("data/plugins/stale.lua"), "gone upstream\n").unwrap();
        std::fs::create_dir_all(root.join("data/user")).unwrap();
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
