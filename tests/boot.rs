//! end-to-end tests: the real, unmodified editor lua booting and running
//! on a headless wisp core with a virtual clock. no window, no display,
//! fully deterministic.

use wisp::headless::Headless;
use wisp::platform::Event;

/// the editor chdirs the process into its project dir (on desktop it
/// owns the process, so that is correct), but every test in this binary
/// shares one process and one cwd: editors running concurrently race
/// each other's relative paths. every test takes this lock first
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// a minimal, stable project directory (its listing is rendered by the
/// treeview, so it must not change between runs). written exactly once:
/// tests run concurrently and editors scan this directory while others
/// boot, so rewriting it mid-run would race
fn project_dir() -> String {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("project");
    ONCE.call_once(|| {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "hello wisp\n").unwrap();
    });
    dir.display().to_string()
}

fn boot() -> Headless {
    let mut editor = Headless::boot(&project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor
}

fn press(editor: &Headless, key: &str) {
    editor.push_event(Event::KeyPressed(key.into()));
    editor.push_event(Event::KeyReleased(key.into()));
}

/// copies the repo's data/ into CARGO_TARGET_TMPDIR/<name>/data and
/// returns the root, ready for `Headless::boot_with_exedir`; tests then
/// overwrite data/user/init.lua in the copy to inject their fixture
fn copy_data_root(name: &str) -> std::path::PathBuf {
    fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let t = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_dir(&e.path(), &t);
            } else {
                std::fs::copy(e.path(), &t).unwrap();
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data"),
        &root.join("data"),
    );
    root
}

/// ctrl+n, then type some text into the fresh doc
fn open_dirty_doc(editor: &mut Headless) {
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(editor, "n");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(50);
    editor.push_event(Event::TextInput("hello".into()));
    editor.run_steps(50);
}

#[test]
fn editor_boots_and_draws_a_frame() {
    let _serial = serial();
    let editor = boot();
    let (pixels, w, h) = editor.last_frame();
    assert_eq!(pixels.len(), (w * h) as usize);

    // the frame must actually look like an editor: a dominant background
    // color plus a meaningful amount of text and ui pixels
    let mut counts = std::collections::HashMap::new();
    for &px in &pixels {
        *counts.entry(px).or_insert(0usize) += 1;
    }
    let (&bg, &bg_count) = counts.iter().max_by_key(|&(_, &n)| n).unwrap();
    let non_bg = pixels.len() - bg_count;
    assert!(
        counts.len() > 16,
        "only {} distinct colors drawn",
        counts.len()
    );
    assert!(
        non_bg > pixels.len() / 100,
        "only {non_bg} non-background pixels (bg {bg:#08x})"
    );
    assert_eq!(editor.window_title(), "wisp");
}

#[test]
fn boot_is_deterministic() {
    let _serial = serial();
    // same events, same virtual clock, same pixels -- twice. every
    // pixel-comparing test in this file stands on this property
    let a = boot();
    let b = boot();
    assert_eq!(
        a.last_frame().0,
        b.last_frame().0,
        "two boots drew different first frames"
    );
}

#[test]
fn boot_is_deterministic_at_2x_scale() {
    let _serial = serial();
    // the scale global reaches every style metric and font size; hidpi
    // must be just as reproducible as 1x, and actually different from it
    let mut a = Headless::boot(&project_dir(), 900, 600, 2.0);
    a.run_until_frames(1, 10_000);
    let mut b = Headless::boot(&project_dir(), 900, 600, 2.0);
    b.run_until_frames(1, 10_000);
    assert_eq!(
        a.last_frame().0,
        b.last_frame().0,
        "two 2x boots drew different first frames"
    );
    assert_ne!(
        a.last_frame().0,
        boot().last_frame().0,
        "2x boot must not render the 1x layout"
    );
}

#[test]
fn resizing_redraws_at_the_new_size() {
    let _serial = serial();
    let mut editor = boot();
    let before = editor.frame_count();
    editor.resize(600, 400);
    editor.run_until_frames(before + 1, 10_000);
    let (pixels, w, h) = editor.last_frame();
    assert_eq!((w, h), (600, 400));
    assert_eq!(pixels.len(), 600 * 400);
    // and the smaller frame is still a real editor, not a stale crop
    let distinct: std::collections::HashSet<u32> = pixels.iter().copied().collect();
    assert!(distinct.len() > 16, "resized frame looks blank");
}

#[test]
fn exposed_event_repaints_identical_pixels() {
    let _serial = serial();
    // an expose must invalidate the cache and present a full frame, and
    // that frame must be exactly what was on screen before
    let mut editor = boot();
    for _ in 0..2000 {
        assert!(editor.step(), "editor exited while settling");
    }
    let settled = editor.frame_count();
    let (before, _, _) = editor.last_frame();
    editor.push_event(Event::Exposed);
    editor.run_steps(200);
    assert!(
        editor.frame_count() > settled,
        "exposed must force a present"
    );
    assert_eq!(
        before,
        editor.last_frame().0,
        "exposed repaint changed the pixels"
    );
}

#[test]
fn clicking_the_treeview_opens_the_file() {
    let _serial = serial();
    // hello.txt is the only row in the treeview; probe downward until the
    // click lands on it (its exact y depends on style metrics, which this
    // test deliberately does not hardcode)
    let mut editor = boot();
    for row in 0..8 {
        let y = 8 + row * 12;
        editor.push_event(Event::MouseMoved(40, y, 0, 0));
        editor.run_steps(50);
        editor.push_event(Event::MousePressed("left", 40, y, 1));
        editor.push_event(Event::MouseReleased("left", 40, y));
        editor.run_steps(100);
        if editor.window_title().starts_with("hello.txt") {
            break;
        }
    }
    assert_eq!(editor.window_title(), "hello.txt - wisp");
}

#[test]
fn idle_editor_stops_redrawing() {
    let _serial = serial();
    // lite's most beloved property, enforced by machine: once quiescent
    // (no open doc, so no blinking caret), the editor must present no
    // new frames at all
    let mut editor = boot();
    // let it settle: plenty of steps for background threads to quiesce
    for _ in 0..2000 {
        if !editor.step() {
            panic!("editor exited while settling");
        }
    }
    let settled = editor.frame_count();
    for _ in 0..2000 {
        editor.step();
    }
    assert_eq!(settled, editor.frame_count(), "idle editor kept redrawing");
}

#[test]
fn typing_in_a_new_doc_appears_on_screen() {
    let _serial = serial();
    let mut editor = boot();
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "n");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    let before = editor.frame_count();
    editor.run_until_frames(before + 1, 10_000);
    let (frame_empty_doc, _, _) = editor.last_frame();

    editor.push_event(Event::TextInput("hello".into()));
    let before = editor.frame_count();
    editor.run_until_frames(before + 1, 10_000);
    let (frame_typed, _, _) = editor.last_frame();

    assert_ne!(
        frame_empty_doc, frame_typed,
        "typing must change the screen"
    );
    assert_eq!(editor.window_title(), "unsaved* - wisp");
}

#[test]
fn quit_event_exits_cleanly() {
    let _serial = serial();
    let mut editor = boot();
    editor.push_event(Event::Quit);
    editor.run_steps(10_000);
    assert_eq!(
        editor.exited,
        Some(0),
        "quit with no unsaved docs must exit 0"
    );
}

#[test]
fn quit_with_unsaved_changes_asks_in_the_editor() {
    let _serial = serial();
    // wisp's one deviation from lite (see DEVIATIONS.md): the unsaved
    // changes confirmation is a commandview prompt, not an os dialog
    let mut editor = boot();
    open_dirty_doc(&mut editor);

    // asking to quit must not exit -- the editor is waiting for an answer
    editor.push_event(Event::Quit);
    editor.run_steps(200);
    assert_eq!(editor.exited, None, "dirty editor must ask before quitting");

    // answering anything but yes cancels
    editor.push_event(Event::TextInput("no".into()));
    press(&editor, "return");
    editor.run_steps(200);
    assert_eq!(editor.exited, None, "answering no must cancel the quit");

    // asking again and answering yes quits
    editor.push_event(Event::Quit);
    editor.run_steps(200);
    editor.push_event(Event::TextInput("yes".into()));
    press(&editor, "return");
    editor.run_steps(2000);
    assert_eq!(editor.exited, Some(0), "answering yes must quit");
}

#[test]
fn dragging_the_divider_resizes_the_treeview() {
    let _serial = serial();
    // lite issue #113: the resize cursor appeared but dragging did
    // nothing. wisp's treeview opts into divider dragging
    let mut editor = boot();
    for _ in 0..2000 {
        assert!(editor.step(), "editor exited while settling");
    }
    // background2 (treeview) vs background (docview) at a point that is
    // docview before the drag and treeview after it
    let (pixels, w, _) = editor.last_frame();
    let probe = |pixels: &[u32], x: i32, y: i32| pixels[(y * w + x) as usize];
    let treeview_bg = probe(&pixels, 100, 300);
    let docview_bg = probe(&pixels, 250, 300);
    assert_ne!(treeview_bg, docview_bg, "test needs distinct backgrounds");

    // the divider sits at the treeview's width (200 at scale 1); press
    // within its 6px grab zone and drag right
    editor.push_event(Event::MousePressed("left", 203, 300, 1));
    editor.run_steps(20);
    editor.push_event(Event::MouseMoved(300, 300, 97, 0));
    editor.run_steps(20);
    editor.push_event(Event::MouseReleased("left", 300, 300));
    editor.run_steps(500);

    let (pixels, _, _) = editor.last_frame();
    assert_eq!(
        probe(&pixels, 250, 300),
        treeview_bg,
        "treeview must widen to cover the dragged-over area"
    );

    // dragging past the window edge must not swallow the whole window:
    // the width caps below it so the divider stays grabbable (80px at
    // scale 1, so from a 900px window the divider stops at 820)
    editor.push_event(Event::MousePressed("left", 300, 300, 1));
    editor.run_steps(20);
    editor.push_event(Event::MouseMoved(890, 300, 590, 0));
    editor.run_steps(20);
    editor.push_event(Event::MouseReleased("left", 890, 300));
    editor.run_steps(500);

    let (pixels, _, _) = editor.last_frame();
    assert_eq!(
        probe(&pixels, 800, 300),
        treeview_bg,
        "treeview must still widen toward the cap"
    );
    assert_eq!(
        probe(&pixels, 860, 300),
        docview_bg,
        "the docview strip past the cap must survive an overdrag"
    );
}

#[test]
fn treeview_scrolling_is_clamped_to_its_content() {
    let _serial = serial();
    // stock lite scrolls the treeview into the void forever
    // (View:get_scrollable_size is math.huge); wisp clamps it
    let mut editor = boot();
    for _ in 0..2000 {
        assert!(editor.step(), "editor exited while settling");
    }
    let (before, _, _) = editor.last_frame();
    editor.push_event(Event::MouseMoved(100, 300, 0, 0));
    editor.run_steps(50);
    for _ in 0..10 {
        editor.push_event(Event::MouseWheel(0.0, -50.0));
        editor.run_steps(50);
    }
    editor.run_steps(1000);
    let (after, _, _) = editor.last_frame();
    assert_eq!(editor.exited, None);
    // with the scroll clamped, wheeling down cannot move the (fully
    // visible) items, so the treeview must look exactly as before
    assert_eq!(
        before, after,
        "treeview scrolled into the void below its content"
    );
}

#[test]
fn binary_files_are_refused() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("binary");
    std::fs::create_dir_all(&dir).unwrap();
    let blob = dir.join("blob.bin");
    std::fs::write(&blob, b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x03\0").unwrap();

    let mut editor = boot();
    // ctrl+o, type the absolute path, return
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput(blob.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    assert_eq!(editor.exited, None, "refusing a binary file must not exit");
    assert_eq!(
        editor.window_title(),
        "wisp",
        "a binary file must not open as a document"
    );
}

#[test]
fn wheel_scrolls_the_document() {
    let _serial = serial();
    let mut editor = boot();
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "n");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(50);
    editor.push_event(Event::TextInput("line\n".repeat(200)));
    editor.run_steps(200);
    let (before, w, h) = editor.last_frame();

    // the wheel is routed to whichever view was under the mouse on the
    // previous step (lite coalesces mouse moves and dispatches them at
    // the end of each step), so move first and let it settle. typing
    // left the view at the bottom of the doc, so scroll up (positive y,
    // the sdl convention lite was written against)
    editor.push_event(Event::MouseMoved(w / 2, h / 2, 0, 0));
    editor.run_steps(50);
    editor.push_event(Event::MouseWheel(0.0, 20.0));
    editor.run_steps(200);
    assert_eq!(editor.exited, None, "editor died on a wheel event");
    let (after, _, _) = editor.last_frame();

    // scrolling must repaint a large part of the view; a blinking caret
    // alone only touches a sliver, so this cannot pass by accident
    let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    assert!(
        changed > before.len() / 20,
        "wheel changed only {changed} of {} pixels",
        before.len()
    );

    // a horizontal wheel at the left edge has nowhere to go; it must be
    // clamped and harmless
    editor.push_event(Event::MouseWheel(3.0, 0.0));
    editor.run_steps(200);
    assert_eq!(editor.exited, None);
}

#[test]
fn horizontal_wheel_pans_long_lines_and_clamps() {
    let _serial = serial();
    let mut editor = boot();
    // an unfocused window draws no caret, so frames compare exactly
    editor.set_focus(false);

    // a fresh doc with a single very long line; typing leaves the view
    // scrolled right, following the caret to the end of the line
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "n");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(50);
    editor.push_event(Event::TextInput(format!("start {}", "wide ".repeat(200))));
    editor.run_steps(500);
    let (right_end, w, h) = editor.last_frame();

    // wheel routing needs the mouse over the docview first
    editor.push_event(Event::MouseMoved(w / 2, h / 2, 0, 0));
    editor.run_steps(50);

    // pan all the way back to the start of the line: positive x scrolls
    // left (the winit convention, mirroring positive y scrolling up)
    for _ in 0..10 {
        editor.push_event(Event::MouseWheel(50.0, 0.0));
        editor.run_steps(50);
    }
    editor.run_steps(1000);
    let (line_start, _, _) = editor.last_frame();
    assert_ne!(
        right_end, line_start,
        "horizontal wheel did not pan the view"
    );

    // overshoot right by ~25000px, then wheel left by only ~10000px:
    // clamped to the widest line (~8000px) this lands back at 0 exactly;
    // unclamped it would strand the view in the void past the text
    for _ in 0..10 {
        editor.push_event(Event::MouseWheel(-50.0, 0.0));
        editor.run_steps(50);
    }
    for _ in 0..4 {
        editor.push_event(Event::MouseWheel(50.0, 0.0));
        editor.run_steps(50);
    }
    editor.run_steps(1000);
    let (after, _, _) = editor.last_frame();
    assert_eq!(editor.exited, None);
    assert_eq!(
        line_start, after,
        "horizontal scroll is not clamped to the content"
    );

    // shift turns a vertical wheel into a horizontal one; the doc is a
    // single line, so on a lua layer without the translation this wheel
    // would scroll vertically, which clamps to nothing and changes nothing
    editor.push_event(Event::KeyPressed("left shift".into()));
    editor.push_event(Event::MouseWheel(0.0, -50.0));
    editor.run_steps(50);
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.run_steps(1000);
    let (shifted, _, _) = editor.last_frame();
    assert_ne!(line_start, shifted, "shift+wheel did not scroll sideways");

    // a wheel that is already horizontal must stay horizontal under
    // shift (the translation used to swap the axes, sending the sideways
    // component into a vertical scroll)
    editor.push_event(Event::KeyPressed("left shift".into()));
    editor.push_event(Event::MouseWheel(50.0, 0.0));
    editor.push_event(Event::MouseWheel(50.0, 0.0));
    editor.run_steps(50);
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.run_steps(1000);
    let (back, _, _) = editor.last_frame();
    assert_eq!(
        line_start, back,
        "shift discarded the hardware horizontal wheel"
    );
}

#[test]
fn clipboard_round_trips_through_the_editor() {
    let _serial = serial();
    // ctrl+n, type, select all, copy: the platform clipboard must hold
    // exactly what was typed
    let mut editor = boot();
    open_dirty_doc(&mut editor);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "a");
    press(&editor, "c");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(200);
    let clipboard = editor.engine.borrow_mut().platform.get_clipboard();
    assert_eq!(clipboard.as_deref(), Some("hello"));
}

#[test]
fn malformed_utf8_never_hangs_the_caret() {
    let _serial = serial();
    // a legacy-encoded (latin-1) file can start with a utf-8 continuation
    // byte; translate.previous_char used to spin forever on it at 1,1.
    // the editor runs in its own thread so a regression fails this test
    // instead of hanging the whole suite
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("latin1");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("legacy.txt");
    std::fs::write(&file, b"\x92hello\x92\n").unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
        editor.run_until_frames(1, 10_000);
        // ctrl+o, type the path, return
        editor.push_event(Event::KeyPressed("left ctrl".into()));
        press(&editor, "o");
        editor.push_event(Event::KeyReleased("left ctrl".into()));
        editor.run_steps(100);
        editor.push_event(Event::TextInput(file.display().to_string()));
        editor.run_steps(100);
        press(&editor, "return");
        editor.run_steps(500);
        // backspace and left arrow at 1,1 both walk previous_char
        press(&editor, "backspace");
        editor.run_steps(100);
        press(&editor, "left");
        editor.run_steps(100);
        // and right arrow at the end of the doc walks next_char
        editor.push_event(Event::KeyPressed("left ctrl".into()));
        press(&editor, "end");
        editor.push_event(Event::KeyReleased("left ctrl".into()));
        editor.run_steps(100);
        press(&editor, "right");
        editor.run_steps(100);
        tx.send(editor.exited).unwrap();
    });
    let exited = rx
        .recv_timeout(std::time::Duration::from_secs(120))
        .expect("editor hung moving the caret over malformed utf-8");
    assert_eq!(exited, None);
}

#[test]
fn project_search_survives_an_empty_project() {
    let _serial = serial();
    // drawing the results view used to divide by zero project files and
    // feed inf into %d, which errors -- on the draw path, outside
    // core.try, killing the editor
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("emptyproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);

    // ctrl+shift+f, type a needle, return
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "f");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("needle".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(1000);
    assert_eq!(
        editor.exited, None,
        "project search crashed in an empty project"
    );
}

#[test]
fn autoreload_keeps_unsaved_changes() {
    let _serial = serial();
    // autoreload used to replace the buffer and mark it clean even when
    // it held unsaved edits
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("reload");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("note.txt");
    std::fs::write(&file, "original\n").unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    // open the file and dirty it
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput(file.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);
    editor.push_event(Event::TextInput("edit ".into()));
    editor.run_steps(100);
    assert!(
        editor.window_title().contains("note.txt*"),
        "expected a dirty doc, title was {:?}",
        editor.window_title()
    );

    // change the file on disk behind the editor's back
    std::fs::write(&file, "replaced\n").unwrap();
    let f = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
        .unwrap();
    drop(f);

    // give the autoreload thread time to notice (it polls every 5
    // virtual seconds)
    editor.run_steps(5000);
    assert_eq!(editor.exited, None);
    assert!(
        editor.window_title().contains("note.txt*"),
        "autoreload discarded unsaved changes (title: {:?})",
        editor.window_title()
    );
}

#[test]
fn renaming_a_file_moves_it_on_disk() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("renametest");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let old = dir.join("old.txt");
    std::fs::write(&old, "content\n").unwrap();
    // age the old file's mtime so the same-file guard can tell the old
    // file from the one the rename just wrote
    let f = std::fs::OpenOptions::new().write(true).open(&old).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    drop(f);

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    // open the file
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput(old.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    // run doc:rename through the command palette; the prompt comes
    // prefilled with the old path, so select-all before typing the new
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "p");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("doc:rename".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(100);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "a");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(50);
    let new = dir.join("new.txt");
    editor.push_event(Event::TextInput(new.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    assert_eq!(editor.exited, None);
    assert!(new.exists(), "renamed file missing on disk");
    assert!(!old.exists(), "old file still on disk after rename");
    assert!(
        editor.window_title().contains("new.txt"),
        "doc did not follow the rename (title: {:?})",
        editor.window_title()
    );
}

#[test]
fn tabularize_keeps_empty_fields() {
    let _serial = serial();
    // the old single-character [^d]+ split dropped empty fields, so
    // "a,,b" lost a column
    let mut editor = boot();
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "n");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(50);
    editor.push_event(Event::TextInput("a,,b\ncc,d,e".into()));
    editor.run_steps(100);

    // select all, then run tabularize from the command palette
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "a");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(50);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "p");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("tabularize".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(100);
    editor.push_event(Event::TextInput(",".into()));
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(200);

    // copy everything and inspect the clipboard
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "a");
    press(&editor, "c");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(200);
    assert_eq!(editor.exited, None);
    let clipboard = editor.engine.borrow_mut().platform.get_clipboard();
    assert_eq!(clipboard.as_deref(), Some("a , ,b\ncc,d,e"));
}

#[test]
fn quit_asks_even_while_a_prompt_is_open() {
    let _serial = serial();
    // a busy commandview used to swallow the quit confirmation entirely:
    // clicking the window button appeared to do nothing
    let mut editor = boot();
    open_dirty_doc(&mut editor);
    // open the find prompt
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "f");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(200);

    editor.push_event(Event::Quit);
    editor.run_steps(200);
    assert_eq!(editor.exited, None, "dirty editor must ask before quitting");

    editor.push_event(Event::TextInput("yes".into()));
    press(&editor, "return");
    editor.run_steps(2000);
    assert_eq!(
        editor.exited,
        Some(0),
        "the quit prompt was swallowed by the open find prompt"
    );
}

#[test]
fn opening_views_works_while_the_treeview_has_focus() {
    let _serial = serial();
    // clicking the treeview focuses a locked node, and by the time a
    // prompt submits the last active view is the (also locked) command
    // view -- lite's open_doc asserted on this instead of falling back
    // to the editing area
    let mut editor = boot();
    // focus the treeview: any click inside it counts, no row needed
    editor.push_event(Event::MouseMoved(40, 300, 0, 0));
    editor.run_steps(50);
    editor.push_event(Event::MousePressed("left", 40, 300, 1));
    editor.push_event(Event::MouseReleased("left", 40, 300));
    editor.run_steps(50);

    // open the project file from a prompt (absolute path: the process
    // cwd races other tests' editors, so the project scan is off limits)
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    let path = format!("{}/hello.txt", project_dir());
    editor.push_event(Event::TextInput(path));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(200);
    assert_eq!(
        editor.window_title(),
        "hello.txt - wisp",
        "open from the palette failed with the treeview focused"
    );

    // the same trap through core:open-log, which adds a view directly
    editor.push_event(Event::MousePressed("left", 40, 300, 1));
    editor.push_event(Event::MouseReleased("left", 40, 300));
    editor.run_steps(50);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "p");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("core:open-log".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(200);
    assert_eq!(
        editor.window_title(),
        "log - wisp",
        "open-log failed with the treeview focused"
    );
}

#[test]
fn caret_follow_keeps_the_users_scroll() {
    let _serial = serial();
    // lite recomputed the horizontal scroll from the caret position on
    // every caret move -- a snap, not a keep-visible clamp. it discarded
    // the view's scroll position whenever the caret moved, and during a
    // drag it fed back into the mouse position and galloped down the
    // line (the fix in franko's unmerged lite PR #230)
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("caretfollow");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("long.txt");
    std::fs::write(&file, format!("{}\n", "a".repeat(300))).unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    // an unfocused window draws no caret, so frames compare exactly
    editor.set_focus(false);

    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput(file.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    // the doc area: everything above the status bar (whose column
    // readout changes with the caret and must not fail the compare)
    fn doc_area(frame: &[u32], w: i32) -> Vec<u32> {
        frame[..(500 * w) as usize].to_vec()
    }

    // at the end of the line the view sits scrolled right; nudging the
    // caret one column left must leave the scroll exactly where it is
    press(&editor, "end");
    editor.run_steps(500);
    let (frame, w, _) = editor.last_frame();
    let before = doc_area(&frame, w);
    press(&editor, "left");
    editor.run_steps(500);
    let (frame, w, _) = editor.last_frame();
    assert_eq!(
        before,
        doc_area(&frame, w),
        "moving the caret one column moved the scroll"
    );

    // drag a selection rightward across the visible text: the scroll
    // must hold still so the selection tracks the mouse, instead of the
    // view sliding under the pointer and ballooning the selection
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "home");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(500);
    editor.push_event(Event::MouseMoved(300, 8, 0, 0));
    editor.run_steps(20);
    editor.push_event(Event::MousePressed("left", 300, 8, 1));
    editor.run_steps(10);
    let mut x = 300;
    while x < 860 {
        x += 10;
        editor.push_event(Event::MouseMoved(x, 8, 0, 0));
        editor.run_steps(3);
    }
    editor.push_event(Event::MouseReleased("left", 860, 8));
    editor.run_steps(100);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "c");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    let selected = editor
        .engine
        .borrow_mut()
        .platform
        .get_clipboard()
        .unwrap_or_default();
    assert!(
        selected.len() > 30,
        "drag selected almost nothing ({} chars)",
        selected.len()
    );
    assert!(
        selected.len() < 120,
        "drag selection galloped: {} chars for a ~560px drag",
        selected.len()
    );
}

#[test]
fn an_invalid_search_pattern_does_not_kill_the_editor() {
    let _serial = serial();
    // "[" is a malformed lua pattern. it used to raise inside the search
    // thread, and a crashed thread propagated straight out of the main
    // loop: the whole editor died on a typo
    let mut editor = boot();
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "p");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("project-search:find-pattern".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(100);
    editor.push_event(Event::TextInput("[".into()));
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(1000);
    assert_eq!(editor.exited, None, "invalid pattern killed the editor");
    assert_eq!(
        editor.window_title(),
        "wisp",
        "invalid pattern still opened a results view"
    );
}

#[test]
fn project_search_skips_binary_files() {
    let _serial = serial();
    // the search scanned binaries and matched garbage; the first result
    // then pointed at a file the editor itself refuses to open
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("searchbin");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("aaa.bin"), b"hello\0hello\n").unwrap();
    std::fs::write(dir.join("hello.txt"), "say hello\n").unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "f");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(50);
    editor.push_event(Event::TextInput("hello".into()));
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(1000);

    // the first (and only) result must be the text file
    press(&editor, "down");
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(500);
    assert_eq!(
        editor.window_title(),
        "hello.txt - wisp",
        "the first search result was not the text file"
    );
}

#[test]
fn refreshing_a_search_midway_does_not_duplicate_results() {
    let _serial = serial();
    // lite cancelled a superseded search thread through a weak table
    // key, i.e. whenever the gc got around to it; wisp cancels it with
    // an explicit generation check. either way, this must hold: a
    // refresh mid-search leaves exactly one search's results
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("searchrefresh");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // big enough that the search is still mid-flight when f5 arrives
    let many = "hello\n".repeat(10_000);
    std::fs::write(dir.join("aaa.txt"), &many).unwrap();
    std::fs::write(dir.join("bbb.txt"), &many).unwrap();

    // two identical editors run the same search; the second refreshes
    // mid-search. the settled frames must be pixel-identical
    let run = |refresh: bool| -> Vec<u32> {
        let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
        editor.run_until_frames(1, 10_000);
        editor.set_focus(false);
        editor.push_event(Event::KeyPressed("left ctrl".into()));
        editor.push_event(Event::KeyPressed("left shift".into()));
        press(&editor, "f");
        editor.push_event(Event::KeyReleased("left shift".into()));
        editor.push_event(Event::KeyReleased("left ctrl".into()));
        editor.run_steps(50);
        editor.push_event(Event::TextInput("hello".into()));
        editor.run_steps(50);
        press(&editor, "return");
        editor.run_steps(3);
        if refresh {
            press(&editor, "f5");
        }
        editor.run_steps(8000);
        editor.last_frame().0
    };
    let clean = run(false);
    let refreshed = run(true);
    assert_eq!(
        clean, refreshed,
        "a refresh mid-search changed the settled results"
    );
}

#[test]
fn line_commands_on_the_last_line_leave_the_doc_intact() {
    let _serial = serial();
    // every whole-line command first materialized a phantom newline at
    // the end of the doc (append_line_if_last_line) -- a real edit:
    // ctrl+l on the last line dirtied the doc, and moving the last line
    // down fed blank lines into it
    fn ctrl(editor: &Headless, key: &str) {
        editor.push_event(Event::KeyPressed("left ctrl".into()));
        press(editor, key);
        editor.push_event(Event::KeyReleased("left ctrl".into()));
    }
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("lastline");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("f.txt");
    std::fs::write(&file, "aa\nbb\ncc\n").unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    ctrl(&editor, "o");
    editor.run_steps(100);
    editor.push_event(Event::TextInput(file.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    // mid-doc select-lines still spans the trailing newline
    ctrl(&editor, "l");
    editor.run_steps(50);
    ctrl(&editor, "c");
    editor.run_steps(100);
    let clip = editor.engine.borrow_mut().platform.get_clipboard();
    assert_eq!(clip.as_deref(), Some("aa\n"));

    // on the last line it selects the text without editing the doc
    ctrl(&editor, "end");
    editor.run_steps(50);
    ctrl(&editor, "l");
    editor.run_steps(100);
    assert_eq!(
        editor.window_title(),
        "f.txt - wisp",
        "select-lines on the last line dirtied the doc"
    );
    ctrl(&editor, "c");
    editor.run_steps(100);
    let clip = editor.engine.borrow_mut().platform.get_clipboard();
    assert_eq!(clip.as_deref(), Some("cc"));

    // moving the last line down is a no-op, not a blank-line feeder
    ctrl(&editor, "down");
    editor.run_steps(100);
    assert_eq!(
        editor.window_title(),
        "f.txt - wisp",
        "move-lines-down on the last line edited the doc"
    );

    let save_and_read = |editor: &mut Headless| -> String {
        ctrl(editor, "s");
        editor.run_steps(300);
        std::fs::read_to_string(&file).unwrap()
    };

    // moving a line down onto the last line swaps them
    ctrl(&editor, "g");
    editor.run_steps(100);
    editor.push_event(Event::TextInput("2".into()));
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(100);
    ctrl(&editor, "down");
    editor.run_steps(100);
    assert_eq!(save_and_read(&mut editor), "aa\ncc\nbb\n");

    // duplicating the last line
    ctrl(&editor, "end");
    editor.run_steps(50);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "d");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    assert_eq!(save_and_read(&mut editor), "aa\ncc\nbb\nbb\n");

    // deleting the last line
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "k");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    assert_eq!(save_and_read(&mut editor), "aa\ncc\nbb\n");

    // moving the last line up
    ctrl(&editor, "end");
    editor.run_steps(50);
    ctrl(&editor, "up");
    editor.run_steps(100);
    assert_eq!(save_and_read(&mut editor), "aa\nbb\ncc\n");
}

#[test]
fn treeview_hover_follows_a_wheel_scroll() {
    let _serial = serial();
    // hover was only recomputed on mouse *move*: wheel-scrolling under a
    // stationary pointer left the highlight (and the click target!) on
    // the pre-scroll row, so clicking opened the wrong file
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("treehover");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 1..=60 {
        std::fs::write(dir.join(format!("f{i:02}.txt")), "x\n").unwrap();
    }

    // two editors: one clicks a treeview spot directly, the other
    // wheel-scrolls first and clicks the same spot
    let run = |scroll: bool| -> String {
        let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
        editor.run_until_frames(1, 10_000);
        editor.run_steps(200); // let the project scan land
        editor.push_event(Event::MouseMoved(40, 300, 0, 0));
        editor.run_steps(100);
        if scroll {
            editor.push_event(Event::MouseWheel(0.0, -3.0));
            editor.run_steps(500);
        }
        editor.push_event(Event::MousePressed("left", 40, 300, 1));
        editor.push_event(Event::MouseReleased("left", 40, 300));
        editor.run_steps(300);
        editor.window_title()
    };
    let direct = run(false);
    let scrolled = run(true);
    assert!(
        direct.contains(".txt"),
        "direct click opened nothing ({direct:?})"
    );
    assert!(
        scrolled.contains(".txt"),
        "post-scroll click opened nothing ({scrolled:?})"
    );
    assert_ne!(
        direct, scrolled,
        "the click target did not follow the scroll"
    );
}

#[test]
fn treeview_has_a_scrollbar_and_pans_sideways() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("treebar");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for i in 1..=60 {
        std::fs::write(dir.join(format!("f{i:02}.txt")), "x\n").unwrap();
    }
    std::fs::write(
        dir.join("z_a_filename_much_wider_than_the_treeview_could_ever_show.txt"),
        "x\n",
    )
    .unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.set_focus(false);
    editor.run_steps(500);

    // the scrollbar strip at the treeview's right edge must not be a
    // uniform background: 61 rows overflow the view, so a bar is drawn
    let (frame, w, _) = editor.last_frame();
    let mut strip = std::collections::HashSet::new();
    for y in 10..500 {
        for x in 196..200 {
            strip.insert(frame[(y * w + x) as usize]);
        }
    }
    assert!(strip.len() > 1, "no scrollbar drawn in the treeview");

    // shift the content sideways with a horizontal wheel and back: the
    // long filename pans into view, then the frame returns exactly
    editor.push_event(Event::MouseMoved(40, 300, 0, 0));
    editor.run_steps(100);
    let (before, _, _) = editor.last_frame();
    editor.push_event(Event::MouseWheel(-2.0, 0.0));
    editor.run_steps(500);
    let (panned, _, _) = editor.last_frame();
    assert_ne!(before, panned, "the treeview did not pan sideways");
    editor.push_event(Event::MouseWheel(2.0, 0.0));
    editor.run_steps(500);
    let (back, _, _) = editor.last_frame();
    assert_eq!(before, back, "panning back did not restore the view");

    // dragging the scrollbar scrolls the list instead of opening the
    // row under the pointer
    editor.push_event(Event::MouseMoved(198, 30, 0, 0));
    editor.run_steps(50);
    editor.push_event(Event::MousePressed("left", 198, 30, 1));
    editor.run_steps(20);
    editor.push_event(Event::MouseMoved(198, 300, 0, 0));
    editor.run_steps(200);
    editor.push_event(Event::MouseReleased("left", 198, 300));
    editor.run_steps(200);
    assert_eq!(
        editor.window_title(),
        "wisp",
        "clicking the scrollbar opened a file"
    );
    let (dragged, _, _) = editor.last_frame();
    assert_ne!(back, dragged, "dragging the scrollbar did not scroll");
}

#[test]
fn autocomplete_merges_duplicate_suggestions() {
    let _serial = serial();
    // two providers offering the same symbol: lite's dedup indexed the
    // sorted list with the wrong variable and repeated one entry down
    // the whole list once a duplicate had been collapsed. the fixture
    // needs a second provider, so the editor boots on a copy of data/
    // with a user module that registers two
    let root = copy_data_root("acroot");
    // symbol lengths differ so the fuzzy scores differ: lua 5.2's
    // randomized string hash makes pairs() order vary per boot, and
    // score ties would preserve that varying order
    std::fs::write(
        root.join("data/user/init.lua"),
        r#"
local autocomplete = require("plugins.autocomplete")
autocomplete.add({ name = "test-a", items = { alpa = "from a" } })
autocomplete.add({ name = "test-b", items = { alpa = "from b", alpbb = false, alpccc = false } })
"#,
    )
    .unwrap();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("acproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.txt"), "x\n").unwrap();

    // type "alp", walk to the nth suggestion, complete it, return the
    // completed doc text
    let complete_nth = |n: usize| -> String {
        let mut editor = Headless::boot_with_exedir(
            &root.display().to_string(),
            &dir.display().to_string(),
            900,
            600,
            1.0,
        );
        editor.run_until_frames(1, 10_000);
        editor.push_event(Event::KeyPressed("left ctrl".into()));
        press(&editor, "n");
        editor.push_event(Event::KeyReleased("left ctrl".into()));
        editor.run_steps(50);
        editor.push_event(Event::TextInput("alp".into()));
        editor.run_steps(10);
        for _ in 1..n {
            press(&editor, "down");
            editor.run_steps(5);
        }
        press(&editor, "tab");
        editor.run_steps(50);
        editor.push_event(Event::KeyPressed("left ctrl".into()));
        press(&editor, "a");
        press(&editor, "c");
        editor.push_event(Event::KeyReleased("left ctrl".into()));
        editor.run_steps(100);
        editor
            .engine
            .borrow_mut()
            .platform
            .get_clipboard()
            .unwrap_or_default()
    };
    // three distinct symbols exist across the two providers, so the
    // list must hold exactly those three, best score first, and walking
    // past the end clamps to the last one. lite's dedup repeated an
    // entry down the rest of the list once a duplicate was collapsed
    assert_eq!(complete_nth(1), "alpa");
    assert_eq!(complete_nth(2), "alpbb");
    assert_eq!(complete_nth(3), "alpccc");
    assert_eq!(
        complete_nth(4),
        "alpccc",
        "a fourth suggestion exists for three distinct symbols"
    );
}

#[test]
fn previous_find_before_any_find_reports_cleanly() {
    let _serial = serial();
    // shift+f3 with no find history popped from a nil table; the raw
    // lua error surfaced instead of a message. a user-module hook
    // mirrors core.error into a file so the test can read the message
    let root = copy_data_root("errroot");
    let errlog = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("errlog.txt");
    let _ = std::fs::remove_file(&errlog);
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
local core = require("core")
local error_fn = core.error
function core.error(...)
    local fp = io.open({:?}, "a")
    fp:write(string.format(...) .. "\n")
    fp:close()
    return error_fn(...)
end
"#,
            errlog.display()
        ),
    )
    .unwrap();

    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "n");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "f3");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.run_steps(200);

    let logged = std::fs::read_to_string(&errlog).unwrap_or_default();
    assert!(
        logged.contains("no previous finds"),
        "expected a clean message, got {logged:?}"
    );
    assert!(
        !logged.contains("table expected"),
        "the raw lua error still reaches the user: {logged:?}"
    );
}

#[test]
fn logview_scrolling_is_clamped_to_its_content() {
    let _serial = serial();
    // the base view reports an unbounded scrollable size, so the log
    // scrolled into the void forever
    let mut editor = boot();
    editor.set_focus(false);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "p");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("core:open-log".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);

    // the few boot messages fit the view, so wheeling down hard must
    // change nothing at all
    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(50);
    let (top, _, _) = editor.last_frame();
    for _ in 0..10 {
        editor.push_event(Event::MouseWheel(0.0, -50.0));
        editor.run_steps(50);
    }
    editor.run_steps(1000);
    assert_eq!(
        top,
        editor.last_frame().0,
        "the log scrolled past its content"
    );
}

#[test]
fn statusbar_column_counts_characters_not_bytes() {
    let _serial = serial();
    // two files, same shape, one multibyte character: with the caret
    // after two characters both status bars must read "col: 3" (lite
    // issue #300 -- the byte offset said 4 for the accented file)
    let run = |name: &str, content: &str| -> Vec<u32> {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.txt");
        std::fs::write(&file, content).unwrap();
        let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
        editor.run_until_frames(1, 10_000);
        editor.set_focus(false);
        // open by relative path: the status bar shows the doc name, and
        // it must be the identical string ("x.txt") in both runs
        editor.push_event(Event::KeyPressed("left ctrl".into()));
        press(&editor, "o");
        editor.push_event(Event::KeyReleased("left ctrl".into()));
        editor.run_steps(100);
        editor.push_event(Event::TextInput("x.txt".into()));
        editor.run_steps(100);
        press(&editor, "return");
        editor.run_steps(500);
        press(&editor, "right");
        editor.run_steps(50);
        press(&editor, "right");
        editor.run_steps(300);
        let (frame, w, h) = editor.last_frame();
        // the status bar strip only: the doc text differs by design
        frame[((h - 25) * w) as usize..].to_vec()
    };
    let plain = run("colplain", "hello\n");
    let accented = run("colaccent", "h\u{e9}llo\n");
    assert_eq!(
        plain, accented,
        "the status bar column differs after two characters"
    );
}

#[test]
fn saving_from_a_prompt_is_refused() {
    let _serial = serial();
    // the command view is a docview, so ctrl+s inside a prompt used to
    // run doc:save on the prompt's one-line doc and offer to write the
    // prompt text to disk
    let mut editor = boot();
    let leak = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("prompt-leak.txt");
    let _ = std::fs::remove_file(&leak);

    // open a doc, then a find prompt over it
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("hello.txt".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "f");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);

    // ctrl+s must be a no-op here; if it opened save-as, the typed path
    // would land in that prompt and return would write the file
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "s");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput(leak.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);

    assert_eq!(editor.exited, None);
    assert!(
        !leak.exists(),
        "saving from a prompt wrote the prompt text to disk"
    );
}

#[test]
fn plugin_position_and_home_expansion_api() {
    let _serial = serial();
    // two api holes plugins fall into: get_line_screen_position ignored
    // its col argument (lite issue #313), and there was no way to expand
    // "~" in paths. a user-module command probes both from inside
    let root = copy_data_root("proberoot");
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("probe-out.txt");
    let _ = std::fs::remove_file(&out);
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
local core = require("core")
local common = require("core.common")
local command = require("core.command")
command.add("core.docview", {{
    ["test:probe"] = function()
        local dv = core.active_view
        local x1 = dv:get_line_screen_position(1)
        local x5 = dv:get_line_screen_position(1, 5)
        local fp = io.open({out:?}, "w")
        fp:write(string.format("%d\n%s\n%s\n", x5 - x1, common.home_expand("~/x.txt"), os.getenv("HOME") or ""))
        fp:close()
    end,
}})
"#,
            out = out.display().to_string()
        ),
    )
    .unwrap();

    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "n");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("wide enough line".into()));
    editor.run_steps(100);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "p");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("test:probe".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);

    let probe = std::fs::read_to_string(&out).expect("the probe command never ran");
    let mut lines = probe.lines();
    let dx: i32 = lines.next().unwrap().parse().unwrap();
    let expanded = lines.next().unwrap();
    let home = lines.next().unwrap();
    assert!(dx > 0, "get_line_screen_position ignored its col argument");
    assert_eq!(
        expanded,
        format!("{home}/x.txt"),
        "home_expand did not expand the tilde"
    );
}

#[test]
fn a_non_latin_project_dir_works() {
    let _serial = serial();
    // lite issue #13: starting from a directory with a non-latin name
    // broke on lite's c core. wisp's raw-byte core must not care
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("проект-δοκιμή");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.txt"), "hi\n").unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("x.txt".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);
    assert_eq!(editor.window_title(), "x.txt - wisp");
}

#[test]
fn logview_pans_sideways() {
    let _serial = serial();
    // a long error message was unreachable: the log view never opted
    // into the §8 sideways-scroll protocol
    let mut editor = boot();
    editor.set_focus(false);
    // fail to open a long nonexistent path: the error lands in the log
    let long = format!("/nowhere/{}.txt", "a".repeat(120));
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput(long));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);

    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "p");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("core:open-log".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);

    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(100);
    let (before, _, _) = editor.last_frame();
    editor.push_event(Event::MouseWheel(-2.0, 0.0));
    editor.run_steps(500);
    let (panned, _, _) = editor.last_frame();
    assert_ne!(before, panned, "the log view did not pan sideways");
    editor.push_event(Event::MouseWheel(2.0, 0.0));
    editor.run_steps(500);
    assert_eq!(
        before,
        editor.last_frame().0,
        "panning back did not restore the log view"
    );
}

#[test]
fn search_results_pan_sideways() {
    let _serial = serial();
    // same hole as the log view: long result lines were unreachable
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("searchpan");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.txt"), format!("hello {}\n", "x".repeat(150))).unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.set_focus(false);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "f");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(50);
    editor.push_event(Event::TextInput("hello".into()));
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(1000);

    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(100);
    let (before, _, _) = editor.last_frame();
    editor.push_event(Event::MouseWheel(-3.0, 0.0));
    editor.run_steps(500);
    let (panned, _, _) = editor.last_frame();
    assert_ne!(before, panned, "the results view did not pan sideways");
    editor.push_event(Event::MouseWheel(3.0, 0.0));
    editor.run_steps(500);
    assert_eq!(
        before,
        editor.last_frame().0,
        "panning back did not restore the results view"
    );
}
