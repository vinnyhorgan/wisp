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

/// holds left ctrl around a key press
fn ctrl(editor: &Headless, key: &str) {
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(editor, key);
    editor.push_event(Event::KeyReleased("left ctrl".into()));
}

/// holds left ctrl and left shift around a key press
fn ctrl_shift(editor: &Headless, key: &str) {
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(editor, key);
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
}

/// drives any command through the palette: ctrl+shift+p, type, return
fn palette(editor: &mut Headless, command: &str) {
    ctrl_shift(editor, "p");
    editor.run_steps(100);
    editor.push_event(Event::TextInput(command.into()));
    editor.run_steps(100);
    press(editor, "return");
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

/// clicks down the treeview rows until hello.txt opens; its exact y
/// depends on style metrics, which this deliberately does not hardcode
fn open_hello_via_treeview(editor: &mut Headless) {
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
fn boot_survives_a_fractional_clock() {
    let _serial = serial();
    // a real desktop clock is never integral, and lua's %d and %x refuse
    // non-integral floats since 5.3. the virtual clock's flat 0.0 hid
    // exactly that once: core/init.lua fed get_time() * 1000 to %08x
    // unfloored, the suite passed, and every desktop launch died at
    // require time. seed the clock before the first resume so require
    // runs at a fractional instant, the way the desktop always does
    let mut editor = Headless::boot(&project_dir(), 900, 600, 1.0);
    editor.set_clock(1723.456789);
    editor.run_until_frames(1, 10_000);
    assert!(editor.exited.is_none());
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
    let mut editor = boot();
    open_hello_via_treeview(&mut editor);
}

#[test]
fn a_triple_click_on_the_last_line_keeps_the_doc_clean() {
    let _serial = serial();
    let mut editor = boot();
    open_hello_via_treeview(&mut editor);
    // hello.txt is one line, so its first line is also its last: the
    // triple click must select it without editing the doc to do so
    editor.push_event(Event::MouseMoved(400, 60, 0, 0));
    editor.run_steps(20);
    editor.push_event(Event::MousePressed("left", 400, 60, 3));
    editor.push_event(Event::MouseReleased("left", 400, 60));
    editor.run_steps(100);
    assert_eq!(editor.window_title(), "hello.txt - wisp");
}

#[test]
fn a_bare_launch_opens_the_current_directory() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("barecwd");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("marker.txt"), "here\n").unwrap();
    // chdirs the shared test process and never restores: safe only
    // because every other boot chdirs again and `serial` is held
    std::env::set_current_dir(&dir).unwrap();
    let mut editor = Headless::boot_bare(900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    // the launch directory is the project: its file must be findable
    ctrl(&editor, "p");
    editor.run_steps(100);
    editor.push_event(Event::TextInput("marker".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);
    assert_eq!(editor.window_title(), "marker.txt - wisp");
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
    // no focus, no caret: otherwise a blink alone could satisfy the
    // pixel diff below and the assert would prove nothing about glyphs
    editor.set_focus(false);
    ctrl(&editor, "n");
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
fn a_dropped_file_opens_in_the_editor() {
    let _serial = serial();
    // the exact event the desktop platform pushes on a dnd drop; the
    // path crosses to lua as raw bytes and must open like any other doc
    let mut editor = boot();
    let file = std::path::Path::new(&project_dir()).join("hello.txt");
    editor.push_event(Event::FileDropped(file, 400, 300));
    editor.run_steps(300);
    assert_eq!(editor.window_title(), "hello.txt - wisp");
}

#[test]
fn exec_fires_and_forgets_a_shell_line() {
    let _serial = serial();
    // system.exec has no consumer in the stock lua, so drive it from an
    // injected user module and watch for its side effect on disk
    let root = copy_data_root("execroot");
    let marker = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("exec-marker");
    let _ = std::fs::remove_file(&marker);
    std::fs::write(
        root.join("data/user/init.lua"),
        format!("system.exec(\"touch '{}'\")\n", marker.display()),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    // the command runs as a real background process on real time, while
    // the editor clock is virtual: poll the disk, not the frame count
    for _ in 0..1000 {
        if marker.exists() {
            return;
        }
        editor.run_steps(10);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("system.exec never ran the command");
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
        editor.push_event(Event::MouseWheel(0.0, -50.0, None));
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
    ctrl(&editor, "o");
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
    ctrl(&editor, "n");
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
    editor.push_event(Event::MouseWheel(0.0, 20.0, None));
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
    editor.push_event(Event::MouseWheel(3.0, 0.0, None));
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
    ctrl(&editor, "n");
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
        editor.push_event(Event::MouseWheel(50.0, 0.0, None));
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
        editor.push_event(Event::MouseWheel(-50.0, 0.0, None));
        editor.run_steps(50);
    }
    for _ in 0..4 {
        editor.push_event(Event::MouseWheel(50.0, 0.0, None));
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
    editor.push_event(Event::MouseWheel(0.0, -50.0, None));
    editor.run_steps(50);
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.run_steps(1000);
    let (shifted, _, _) = editor.last_frame();
    assert_ne!(line_start, shifted, "shift+wheel did not scroll sideways");

    // a wheel that is already horizontal must stay horizontal under
    // shift (the translation used to swap the axes, sending the sideways
    // component into a vertical scroll)
    editor.push_event(Event::KeyPressed("left shift".into()));
    editor.push_event(Event::MouseWheel(50.0, 0.0, None));
    editor.push_event(Event::MouseWheel(50.0, 0.0, None));
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
        ctrl(&editor, "o");
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
        ctrl(&editor, "end");
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
    ctrl_shift(&editor, "f");
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
    ctrl(&editor, "o");
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
    // line the write up with a fresh second so the rename's own save
    // lands inside it too: both files then report the same whole-second
    // mtime and, the doc being clean, the same size -- the collision the
    // old stat-based same-file guard read as "same file", leaving the
    // old file on disk
    // (sleep a little past the boundary, not exactly onto it: waking a
    // few microseconds early would put the two writes either side of it)
    let subsec = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    std::thread::sleep(std::time::Duration::from_nanos(u64::from(
        1_050_000_000 - subsec,
    )));
    std::fs::write(&old, "content\n").unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    // open the file
    ctrl(&editor, "o");
    editor.run_steps(100);
    editor.push_event(Event::TextInput(old.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    // run doc:rename through the command palette; the prompt comes
    // prefilled with the old path, so select-all before typing the new
    palette(&mut editor, "doc:rename");
    editor.run_steps(100);
    ctrl(&editor, "a");
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
    ctrl(&editor, "n");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("a,,b\ncc,d,e".into()));
    editor.run_steps(100);

    // select all, then run tabularize from the command palette
    ctrl(&editor, "a");
    editor.run_steps(50);
    palette(&mut editor, "tabularize");
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
    ctrl(&editor, "f");
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
    ctrl(&editor, "o");
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
    palette(&mut editor, "core:open-log");
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

    ctrl(&editor, "o");
    editor.run_steps(100);
    editor.push_event(Event::TextInput(file.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    // the doc area: everything above the status bar (whose column
    // readout changes with the caret and must not fail the compare).
    // 500 hardcodes the 900x600 boot geometry with generous margin: the
    // status bar sits well below it at these style metrics, and a
    // too-small cut only makes the compare weaker, never wrong
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
    ctrl(&editor, "home");
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
    ctrl(&editor, "c");
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
    palette(&mut editor, "project-search:find-pattern");
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
    ctrl_shift(&editor, "f");
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
        ctrl_shift(&editor, "f");
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
    ctrl_shift(&editor, "d");
    editor.run_steps(100);
    assert_eq!(save_and_read(&mut editor), "aa\ncc\nbb\nbb\n");

    // deleting the last line
    ctrl_shift(&editor, "k");
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
            editor.push_event(Event::MouseWheel(0.0, -3.0, None));
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
    editor.push_event(Event::MouseWheel(-2.0, 0.0, None));
    editor.run_steps(500);
    let (panned, _, _) = editor.last_frame();
    assert_ne!(before, panned, "the treeview did not pan sideways");
    editor.push_event(Event::MouseWheel(2.0, 0.0, None));
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
        ctrl(&editor, "n");
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
    ctrl(&editor, "n");
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
    palette(&mut editor, "core:open-log");
    editor.run_steps(300);

    // the few boot messages fit the view, so wheeling down hard must
    // change nothing at all
    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(50);
    let (top, _, _) = editor.last_frame();
    for _ in 0..10 {
        editor.push_event(Event::MouseWheel(0.0, -50.0, None));
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
        ctrl(&editor, "o");
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
        // the status bar strip only: the doc text differs by design.
        // 25 rows hardcodes the bar's height at these style metrics; if
        // the bar ever grows past it the slice still lies inside the
        // bar, so the column readout stays in the compare
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
fn a_long_name_never_crowds_the_right_of_the_status_bar() {
    let _serial = serial();
    // identical content, wildly different name lengths, a narrow window.
    // the right-hand group ("N lines   lf") is the same string in both
    // runs and lands at the same x, so any difference in the right of
    // the strip means the left group painted straight through it
    let run = |dirname: &str, file: &str| -> Vec<u32> {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(dirname);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), "hello\n").unwrap();
        let mut editor = Headless::boot(&dir.display().to_string(), 500, 400, 1.0);
        editor.run_until_frames(1, 10_000);
        editor.set_focus(false);
        ctrl(&editor, "o");
        editor.run_steps(100);
        editor.push_event(Event::TextInput(file.into()));
        editor.run_steps(100);
        press(&editor, "return");
        editor.run_steps(500);
        let (frame, w, h) = editor.last_frame();
        // the rightmost 80px of the status strip: the left group is cut
        // off exactly where the right group starts, well left of this
        let mut out = Vec::new();
        for y in (h - 25)..h {
            for x in (w - 80)..w {
                out.push(frame[(y * w + x) as usize]);
            }
        }
        out
    };
    let short = run("statusshort", "a.txt");
    let long = run(
        "statuslong",
        "a-very-long-file-name-that-crowds-the-status-bar-badly.txt",
    );
    assert_eq!(
        short, long,
        "a long filename painted into the right-hand status group"
    );
}

#[test]
fn hiding_the_status_bar_never_flashes_a_stale_message() {
    let _serial = serial();
    // the message row sits one row-height below the item row and scrolls
    // up to replace it. while a toggle animates the bar's height that
    // offset must stay put -- keyed to size.y it collapses, sliding an
    // expired message into view partway down.
    //
    // two editors, same everything, except one has shown a message that
    // has since expired: the collapse must look identical in both
    let run = |with_message: bool| -> Vec<Vec<u32>> {
        let mut editor = boot();
        editor.set_focus(false);
        open_hello_via_treeview(&mut editor);
        if with_message {
            // logs "no previous finds" through core.error, which is what
            // puts a message in the bar
            palette(&mut editor, "find-replace:previous-find");
            editor.run_steps(200);
        }
        // jump well past config.message_timeout, then let the message
        // scroll back out so both editors are at rest and identical
        editor.set_clock(1000.0);
        editor.run_steps(500);

        ctrl_shift(&editor, "\\");
        let mut frames = Vec::new();
        for _ in 0..10 {
            editor.run_steps(2);
            let (frame, w, h) = editor.last_frame();
            frames.push(frame[((h - 40) * w) as usize..].to_vec());
        }
        frames
    };
    assert_eq!(
        run(false),
        run(true),
        "collapsing the bar flashed an expired message"
    );
}

#[test]
fn the_status_bar_toggles_and_comes_back() {
    let _serial = serial();
    let mut editor = boot();
    editor.set_focus(false);
    editor.run_steps(500);
    let before = editor.last_frame().0;

    // ctrl+shift+\ collapses the bar; its height animates, so let it
    // settle before looking
    ctrl_shift(&editor, "\\");
    editor.run_steps(1000);
    assert_ne!(before, editor.last_frame().0, "the status bar did not hide");

    ctrl_shift(&editor, "\\");
    editor.run_steps(1000);
    assert_eq!(
        before,
        editor.last_frame().0,
        "the status bar did not come back the way it went"
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
    ctrl(&editor, "o");
    editor.run_steps(100);
    editor.push_event(Event::TextInput("hello.txt".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);
    ctrl(&editor, "f");
    editor.run_steps(100);

    // ctrl+s must be a no-op here; if it opened save-as, the typed path
    // would land in that prompt and return would write the file
    ctrl(&editor, "s");
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
    ctrl(&editor, "n");
    editor.run_steps(100);
    editor.push_event(Event::TextInput("wide enough line".into()));
    editor.run_steps(100);
    palette(&mut editor, "test:probe");
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
    ctrl(&editor, "o");
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
    ctrl(&editor, "o");
    editor.run_steps(100);
    editor.push_event(Event::TextInput(long));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);

    palette(&mut editor, "core:open-log");
    editor.run_steps(300);

    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(100);
    let (before, _, _) = editor.last_frame();
    editor.push_event(Event::MouseWheel(-2.0, 0.0, None));
    editor.run_steps(500);
    let (panned, _, _) = editor.last_frame();
    assert_ne!(before, panned, "the log view did not pan sideways");
    editor.push_event(Event::MouseWheel(2.0, 0.0, None));
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
    ctrl_shift(&editor, "f");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("hello".into()));
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(1000);

    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(100);
    let (before, _, _) = editor.last_frame();
    editor.push_event(Event::MouseWheel(-3.0, 0.0, None));
    editor.run_steps(500);
    let (panned, _, _) = editor.last_frame();
    assert_ne!(before, panned, "the results view did not pan sideways");
    editor.push_event(Event::MouseWheel(3.0, 0.0, None));
    editor.run_steps(500);
    assert_eq!(
        before,
        editor.last_frame().0,
        "panning back did not restore the results view"
    );
}

#[test]
fn a_focus_loss_forgets_held_modifiers() {
    let _serial = serial();
    let mut editor = boot();

    // open the project file so there is a tab to close later
    ctrl(&editor, "o");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("hello.txt".into()));
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(50);
    assert_eq!(editor.window_title(), "hello.txt - wisp");

    // alt goes down and focus leaves (alt+tab); the release lands in
    // another window. wayland sends no key releases on focus loss, so
    // the editor never sees alt go up
    editor.push_event(Event::KeyPressed("left alt".into()));
    editor.run_steps(5);
    editor.set_focus(false);
    editor.run_steps(5);
    editor.set_focus(true);
    editor.run_steps(5);

    // with alt latched every chord was dead (ctrl+w arrived as
    // alt+ctrl+w); the focus round-trip must have unstuck it
    ctrl(&editor, "w");
    editor.run_steps(50);
    assert_eq!(editor.window_title(), "wisp");
}

#[test]
fn a_diagonal_wheel_snaps_to_its_dominant_axis() {
    let _serial = serial();
    let mut editor = boot();
    editor.set_focus(false);

    // one long line: caret-follow leaves the view scrolled right, and a
    // one-line doc cannot scroll vertically at all, so the only thing a
    // mostly-vertical diagonal wheel could do here is leak its x axis
    ctrl(&editor, "n");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("wide ".repeat(200)));
    editor.run_steps(500);
    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(50);

    let (before, _, _) = editor.last_frame();
    editor.push_event(Event::MouseWheel(1.0, -3.0, None));
    editor.run_steps(500);
    assert_eq!(
        before,
        editor.last_frame().0,
        "the weak x axis of a vertical glide leaked into a pan"
    );

    // the other way around, a mostly-horizontal glide must still pan
    editor.push_event(Event::MouseWheel(30.0, -1.0, None));
    editor.run_steps(500);
    assert_ne!(
        before,
        editor.last_frame().0,
        "a dominant x axis no longer pans"
    );
}

#[test]
fn a_trackpad_gesture_stays_railed_to_its_first_axis() {
    let _serial = serial();
    let mut editor = boot();
    editor.set_focus(false);

    // same fixture as the diagonal-wheel test: one long line, view
    // scrolled right by caret-follow, no vertical scrolling possible
    ctrl(&editor, "n");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("wide ".repeat(200)));
    editor.run_steps(500);
    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(50);
    let (before, _, _) = editor.last_frame();

    // a gesture that starts vertical, then drifts hard sideways: the
    // drift must stay railed to the starting axis, however dominant
    editor.push_event(Event::MouseWheel(0.0, -3.0, Some("moved")));
    editor.run_steps(50);
    editor.push_event(Event::MouseWheel(2.0, -0.5, Some("moved")));
    editor.run_steps(500);
    assert_eq!(
        before,
        editor.last_frame().0,
        "a vertical gesture's sideways drift leaked into a pan"
    );

    // fingers lift; the next gesture starts sideways and must pan
    editor.push_event(Event::MouseWheel(0.0, 0.0, Some("ended")));
    editor.run_steps(50);
    editor.push_event(Event::MouseWheel(2.0, -0.5, Some("moved")));
    editor.run_steps(500);
    assert_ne!(
        before,
        editor.last_frame().0,
        "the rail did not unlock when the gesture ended"
    );
}

#[test]
fn a_discrete_wheel_ignores_a_stale_trackpad_rail() {
    let _serial = serial();
    let mut editor = boot();
    editor.set_focus(false);

    // a doc tall enough to scroll vertically
    ctrl(&editor, "n");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("line\n".repeat(100)));
    editor.run_steps(500);
    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(50);

    // a sideways gesture rails the axis, then the fingers lift without
    // an "ended" phase ever arriving (focus lost mid-gesture, a
    // compositor that drops it). the rail must not outlive the gesture:
    // a phaseless wheel stands alone, which is what §8 promises
    editor.push_event(Event::MouseWheel(-3.0, 0.0, Some("moved")));
    editor.run_steps(200);
    let (before, _, _) = editor.last_frame();

    editor.push_event(Event::MouseWheel(0.0, -3.0, None));
    editor.run_steps(500);
    assert_ne!(
        before,
        editor.last_frame().0,
        "a latched trackpad rail swallowed a discrete wheel"
    );
}

#[test]
fn lua_spawns_a_subprocess_and_round_trips_data() {
    let _serial = serial();
    // the whole system.spawn surface, driven from a user-module command:
    // spawn cat, write to its stdin, read stdout to eof (nil, the
    // file:read convention), then report the exit code
    let root = copy_data_root("spawnroot");
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("spawn-out.txt");
    let _ = std::fs::remove_file(&out);
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
local core = require("core")
local command = require("core.command")
command.add(nil, {{
    ["test:spawn"] = function()
        local proc, err = system.spawn({{ "cat" }})
        if not proc then
            core.error("%s", err)
            return
        end
        proc:write("ping\n")
        proc:close_stdin()
        core.add_thread(function()
            local chunks = {{}}
            while true do
                local chunk = proc:read_stdout()
                if not chunk then
                    break
                end
                chunks[#chunks + 1] = chunk
                coroutine.yield()
            end
            while proc:running() do
                coroutine.yield()
            end
            local fp = io.open({out:?}, "w")
            fp:write(string.format("%s|%d", table.concat(chunks), proc:returncode()))
            fp:close()
        end)
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
    palette(&mut editor, "test:spawn");

    // the editor clock is virtual but cat runs in real time: keep
    // stepping (resuming the reader coroutine) until the report lands
    for _ in 0..1000 {
        editor.run_steps(20);
        if out.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let report = std::fs::read_to_string(&out).expect("the spawn thread never reported");
    assert_eq!(report, "ping\n|0");
}

#[test]
fn growing_a_view_reclamps_a_stale_sideways_scroll() {
    let _serial = serial();
    // a line that overflows a 900px window (so caret-follow pans right)
    // but fits an 1100px one
    let text = "wide ".repeat(19);

    let mut editor = boot();
    editor.set_focus(false);
    ctrl(&editor, "n");
    editor.run_steps(50);
    editor.push_event(Event::TextInput(text.clone()));
    editor.run_steps(500);
    editor.resize(1100, 600);
    editor.run_steps(500);
    let (grown, _, _) = editor.last_frame();

    // the reference was 1100 wide all along: the caret never left the
    // view, so it never scrolled. growing the first editor must land it
    // on this exact frame instead of leaving the stale pan in place
    let mut reference = Headless::boot(&project_dir(), 1100, 600, 1.0);
    reference.run_until_frames(1, 10_000);
    reference.set_focus(false);
    ctrl(&reference, "n");
    reference.run_steps(50);
    reference.push_event(Event::TextInput(text));
    reference.run_steps(500);
    let refframe = reference.last_frame().0;
    assert_eq!(grown.len(), refframe.len(), "frame sizes differ");
    assert_eq!(
        grown, refframe,
        "the grown view kept its stale sideways scroll"
    );
}

#[test]
fn deleting_the_widest_line_brings_the_view_home() {
    let _serial = serial();
    // pan right by typing a long second line, then delete it: between
    // caret-follow and the clamp, the short first line must never be
    // left hanging off-screen
    let mut editor = boot();
    editor.set_focus(false);
    ctrl(&editor, "n");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("bb".into()));
    press(&editor, "return");
    editor.push_event(Event::TextInput("wide ".repeat(40)));
    editor.run_steps(500);
    ctrl_shift(&editor, "k");
    editor.run_steps(500);
    let (after_delete, _, _) = editor.last_frame();

    // the reference typed the short line and nothing else; the caret
    // lands on the same spot, so the frames must be identical
    let mut reference = boot();
    reference.set_focus(false);
    ctrl(&reference, "n");
    reference.run_steps(50);
    reference.push_event(Event::TextInput("bb".into()));
    // delete-lines parks the caret at column 1; match it, or the
    // status bar's column readout differs
    press(&reference, "home");
    reference.run_steps(500);
    let refframe = reference.last_frame().0;
    assert_eq!(
        after_delete, refframe,
        "the deleted line's width still holds the view panned"
    );
}

#[test]
fn collapsing_a_wide_folder_reclamps_the_scroll() {
    let _serial = serial();
    // the content input of the clamp invariant, with no caret to come
    // to the rescue: expand a folder holding a very long name, pan the
    // treeview right, collapse the folder. the pan must come home, not
    // leave the short root row hanging off-screen
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("collapseroot");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("deep")).unwrap();
    std::fs::write(
        dir.join("deep")
            .join(format!("{}.txt", "long-name-".repeat(8))),
        "x\n",
    )
    .unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.set_focus(false);
    editor.run_steps(200);
    let (collapsed, _, _) = editor.last_frame();

    // find the folder row the way clicking_the_treeview does: probe
    // downward until a click changes the frame (the child row appears)
    let mut row_y = None;
    for row in 0..8 {
        let y = 8 + row * 12;
        editor.push_event(Event::MouseMoved(40, y, 0, 0));
        editor.run_steps(50);
        editor.push_event(Event::MousePressed("left", 40, y, 1));
        editor.push_event(Event::MouseReleased("left", 40, y));
        editor.run_steps(200);
        if editor.last_frame().0 != collapsed {
            row_y = Some(y);
            break;
        }
    }
    let y = row_y.expect("no click expanded the folder");

    // pan right (the long child name gives the view sideways room)
    editor.push_event(Event::MouseWheel(-5.0, 0.0, None));
    editor.run_steps(300);

    // collapse again, then park the mouse over the docview so no
    // hover highlight is left behind for the frame comparison
    editor.push_event(Event::MousePressed("left", 40, y, 1));
    editor.push_event(Event::MouseReleased("left", 40, y));
    editor.run_steps(100);
    editor.push_event(Event::MouseMoved(600, 300, 0, 0));
    editor.run_steps(300);
    assert_eq!(
        collapsed,
        editor.last_frame().0,
        "the collapsed folder's width still holds the treeview panned"
    );
}

// the image api's consumer, proven in-suite before the core freezes: a
// view (here the rootview, standing in for phase d's imageview) loads a
// png and draws it natural, scaled, and tinted
#[test]
fn lua_can_load_and_draw_images() {
    let _serial = serial();
    let root = copy_data_root("imageroot");
    // the 2x2 test card: red, green / blue, white, fully opaque
    let card = image::RgbaImage::from_fn(2, 2, |x, y| match (x, y) {
        (0, 0) => image::Rgba([255, 0, 0, 255]),
        (1, 0) => image::Rgba([0, 255, 0, 255]),
        (0, 1) => image::Rgba([0, 0, 255, 255]),
        _ => image::Rgba([255, 255, 255, 255]),
    });
    let fixture = root.join("card.png");
    card.save(&fixture).unwrap();
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
local RootView = require "core.rootview"
local img = renderer.image.load [[{}]]
assert(img:get_width() == 2 and img:get_height() == 2)
local draw = RootView.draw
function RootView:draw(...)
  draw(self, ...)
  renderer.draw_image(img, 100, 100, 64, 64)
  renderer.draw_image(img, 300, 100)
  renderer.draw_rect(500, 100, 64, 64, {{ 0, 0, 0, 255 }})
  renderer.draw_image(img, 500, 100, 64, 64, {{ 255, 255, 255, 128 }})
end
"#,
            fixture.display()
        ),
    )
    .unwrap();

    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    let (pixels, w, _) = editor.last_frame();
    let at = |x: i32, y: i32| pixels[(y * w + x) as usize];

    // scaled 2x2 -> 64x64: each source pixel is a clean 32x32 block
    assert_eq!(at(110, 110), 0xff0000);
    assert_eq!(at(150, 110), 0x00ff00);
    assert_eq!(at(110, 150), 0x0000ff);
    assert_eq!(at(150, 150), 0xffffff);
    // natural size: exactly 2x2, not a pixel more
    assert_eq!(at(300, 100), 0xff0000);
    assert_eq!(at(301, 100), 0x00ff00);
    assert_eq!(at(300, 101), 0x0000ff);
    assert_eq!(at(301, 101), 0xffffff);
    assert_ne!(at(302, 100), 0x00ff00);
    // half-alpha tint over the black square: exactly half red
    assert_eq!(at(510, 110), 0x800000);
}

#[test]
fn mkdir_creates_a_directory_once() {
    let _serial = serial();
    let root = copy_data_root("mkdirroot");
    let marker = root.join("mkdir-ok");
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
local made = [[{root}]] .. "/made"
assert(system.mkdir(made) == true)
local info = system.get_file_info(made)
assert(info and info.type == "dir")
-- a second creation reports the failure instead of raising
local again, err = system.mkdir(made)
assert(again == nil and type(err) == "string")
io.open([[{marker}]], "w"):close()
"#,
            root = root.display(),
            marker = marker.display()
        ),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    assert!(marker.exists(), "mkdir did not behave; see the user module");
}

#[test]
fn set_size_rescales_a_font_for_every_holder() {
    let _serial = serial();
    let root = copy_data_root("fontsizeroot");
    let marker = root.join("size-ok");
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
-- runtime zoom's core affordance: resize in place, every holder of
-- the font object sees the new metrics, no references to chase
local style = require "core.style"
local h = style.code_font:get_height()
local s = style.code_font:get_size()
style.code_font:set_size(s * 2)
assert(style.code_font:get_size() > s)
assert(style.code_font:get_height() > h)
io.open([[{}]], "w"):close()
"#,
            marker.display()
        ),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    // the frame itself is part of the proof: it drew at the new size
    editor.run_until_frames(1, 10_000);
    assert!(marker.exists(), "set_size misbehaved; see the user module");
}

#[test]
fn fs_events_reach_a_lua_coroutine() {
    let _serial = serial();
    let root = copy_data_root("watchroot");
    let marker = root.join("watch-ok");
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
-- the dirmonitor pattern: watch a tree, poll from a core thread,
-- exactly how the project scan will one day stop being a timer
local core = require "core"
local w = assert(system.watch([[{root}]]))
core.add_thread(function()
  while true do
    for _, change in ipairs(w:poll()) do
      if change[2]:find("sentinel", 1, true) then
        io.open([[{marker}]], "w"):close()
        return
      end
    end
    coroutine.yield(0.05)
  end
end)
"#,
            root = root.display(),
            marker = marker.display()
        ),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    // an outside change: the editor must notice without any rescan
    std::fs::write(root.join("sentinel.txt"), b"changed").unwrap();
    let ok = poll_editor(&mut editor, 10, |_| marker.exists());
    assert!(ok, "the fs event never reached the lua thread");
}

// --- the terminal: a real shell inside the editor ----------------------

/// boots an editor whose terminal runs the given argv (written into the
/// user module of a copied data tree)
fn boot_terminal_editor(name: &str, argv_lua: &str) -> Headless {
    let root = copy_data_root(name);
    std::fs::write(
        root.join("data/user/init.lua"),
        format!("local config = require \"core.config\"\nconfig.terminal_argv = {argv_lua}\n"),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor
}

/// real-time polling: terminal output arrives on the real clock even
/// though the editor's clock is virtual
fn poll_editor(
    editor: &mut Headless,
    secs: u64,
    mut done: impl FnMut(&mut Headless) -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        editor.run_steps(200);
        if done(editor) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

#[test]
fn a_real_shell_runs_inside_the_editor() {
    let _serial = serial();
    let mut editor = boot_terminal_editor("termshell", r#"{ "/bin/sh" }"#);
    ctrl(&editor, "`");
    editor.run_steps(200);
    assert!(
        editor.window_title().contains("terminal"),
        "terminal view did not focus: {}",
        editor.window_title()
    );
    // ask the shell to paint four cells in ansi red; the plugin maps
    // color 1 to catppuccin red #f38ba8, so the pixels are exact
    editor.push_event(Event::TextInput(r"printf '\033[41m    \033[0m'".into()));
    editor.run_steps(50);
    press(&editor, "return");
    let red = 0xf38ba8;
    let ok = poll_editor(&mut editor, 10, |e| {
        e.last_frame().0.iter().filter(|&&p| p == red).count() > 100
    });
    assert!(ok, "no ansi-red pixels appeared within the timeout");
}

#[test]
fn the_palette_still_opens_over_a_focused_terminal() {
    let _serial = serial();
    let mut editor = boot_terminal_editor("termpalette", r#"{ "/bin/sh" }"#);
    ctrl(&editor, "`");
    editor.run_steps(200);
    assert!(editor.window_title().contains("terminal"));
    // ctrl+shift+p is on the pass-through list: it must reach the
    // editor, not the shell, and the palette must work normally
    palette(&mut editor, "core: open user module");
    let ok = poll_editor(&mut editor, 5, |e| e.window_title().contains("init.lua"));
    assert!(
        ok,
        "palette did not run over the terminal: {}",
        editor.window_title()
    );
}

#[test]
fn terminal_line_answers_any_row_without_raising() {
    let _serial = serial();
    let root = copy_data_root("termline");
    let marker = root.join("line-ok");
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
-- line() runs on the draw path, where an error is fatal: any number
-- must come back as a table, never a raised error
local t = assert(system.terminal(10, 5, {{ argv = {{ "/bin/sh", "-c", "exit 0" }} }}))
for _, row in ipairs({{ -1, 0, 1, 2.5, 6, 1e9 }}) do
  assert(type(t:line(row)) == "table")
end
io.open([[{}]], "w"):close()
"#,
            marker.display()
        ),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    assert!(marker.exists(), "line() raised on a hostile row");
}

#[test]
fn a_finished_shell_closes_its_tab() {
    let _serial = serial();
    let mut editor = boot_terminal_editor("termexit", r#"{ "/bin/sh", "-c", "exit 0" }"#);
    ctrl(&editor, "`");
    editor.run_steps(100);
    // the shell exits immediately; the poller must notice and close the
    // tab, dropping the title back to the document view
    let ok = poll_editor(&mut editor, 5, |e| !e.window_title().contains("terminal"));
    assert!(ok, "terminal tab never closed: {}", editor.window_title());
}

/// everything that is not one of the two flat background colors:
/// glyphs, icons, dividers, scrollbars. a bigger font paints more of it
fn ink(frame: &[u32]) -> usize {
    frame
        .iter()
        .filter(|&&p| p != 0x1e1e2e && p != 0x181825)
        .count()
}

#[test]
fn zooming_grows_the_editor_and_a_reset_puts_every_pixel_back() {
    let _serial = serial();
    let mut editor = boot();
    open_hello_via_treeview(&mut editor);
    editor.set_focus(false);
    editor.run_steps(500);
    let before = editor.last_frame().0;

    for _ in 0..3 {
        ctrl(&editor, "=");
        editor.run_steps(300);
    }
    let bigger = editor.last_frame().0;
    assert!(
        ink(&bigger) > ink(&before),
        "ctrl+= painted no more ink ({} -> {})",
        ink(&before),
        ink(&bigger)
    );

    for _ in 0..6 {
        ctrl(&editor, "-");
        editor.run_steps(300);
    }
    let smaller = editor.last_frame().0;
    assert!(
        ink(&smaller) < ink(&before),
        "ctrl+- painted no less ink ({} -> {})",
        ink(&before),
        ink(&smaller)
    );

    // the whole reason every metric is measured from its boot value: a
    // reset is an identity. lite-xl multiplies the live numbers by a
    // ratio per step, so its rounding compounds and a reset lands near,
    // but not on, where it started
    ctrl(&editor, "0");
    editor.run_steps(500);
    assert_eq!(
        before,
        editor.last_frame().0,
        "a reset did not restore the boot scale exactly"
    );
}

#[test]
fn ctrl_wheel_zooms_instead_of_scrolling() {
    let _serial = serial();

    // the reference: one zoom step out, driven by the keyboard, on a doc
    // long enough that any stray scroll would move the text
    let long_text = "line\n".repeat(200);
    let mut reference = boot();
    ctrl(&reference, "n");
    reference.run_steps(50);
    reference.push_event(Event::TextInput(long_text.clone().into()));
    reference.set_focus(false);
    reference.run_steps(300);
    ctrl(&reference, "-");
    reference.run_steps(500);
    let expected = reference.last_frame().0;
    drop(reference);

    let mut editor = boot();
    ctrl(&editor, "n");
    editor.run_steps(50);
    editor.push_event(Event::TextInput(long_text.into()));
    editor.set_focus(false);
    editor.run_steps(300);

    // the wheel goes to the view under the mouse, so move there first
    let (_, w, h) = editor.last_frame();
    editor.push_event(Event::MouseMoved(w / 2, h / 2, 0, 0));
    editor.run_steps(50);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::MouseWheel(0.0, -1.0, None));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(500);

    assert_eq!(
        expected,
        editor.last_frame().0,
        "ctrl+wheel did not land exactly where scale:decrease does"
    );
}

#[test]
fn a_file_created_outside_the_editor_appears_without_the_timer() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("watched");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.run_steps(60);

    // only the treeview strip: it is where core.project_files is drawn,
    // and the caret blinking in the document beside it would otherwise
    // read as a change. the editor stays focused on purpose -- a focused
    // step is exactly one 60fps frame of editor time, an idle unfocused
    // one is core.run's whole 0.25s wait
    let strip = |e: &Headless| -> Vec<u32> {
        let (frame, w, h) = e.last_frame();
        (0..h)
            .flat_map(|y| {
                let row = (y * w) as usize;
                frame[row..row + 150].to_vec()
            })
            .collect()
    };
    let before = strip(&editor);

    // 60 + 6*30 = 240 steps, four seconds of editor time: the standing
    // rescan runs every config.project_scan_rate (5s), so it cannot be
    // what noticed
    std::fs::write(dir.join("b.txt"), "b\n").unwrap();
    let mut appeared = false;
    for _ in 0..6 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        editor.run_steps(30);
        if strip(&editor) != before {
            appeared = true;
            break;
        }
    }
    assert!(appeared, "the new file never reached the treeview");
}

#[test]
fn a_user_module_can_set_the_zoom_at_boot() {
    let _serial = serial();

    // the zoom a session ends on is not remembered yet, so the way to
    // start somewhere other than 100% is the user module -- it is
    // required after the plugins, so the plugin's own table is there to
    // call. this pins that path
    let root = copy_data_root("userzoom");
    std::fs::write(
        root.join("data/user/init.lua"),
        "require(\"plugins.scale\").set(1.3)\n",
    )
    .unwrap();
    let mut booted =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    booted.run_until_frames(1, 10_000);
    open_hello_via_treeview(&mut booted);
    booted.set_focus(false);
    booted.run_steps(500);
    let from_user_module = booted.last_frame().0;
    drop(booted);

    // the same zoom reached by pressing ctrl+= three times
    let mut pressed = boot();
    open_hello_via_treeview(&mut pressed);
    pressed.set_focus(false);
    pressed.run_steps(500);
    for _ in 0..3 {
        ctrl(&pressed, "=");
        pressed.run_steps(300);
    }

    assert_eq!(
        from_user_module,
        pressed.last_frame().0,
        "a zoom set from the user module does not match the same zoom typed in"
    );
}

#[test]
fn a_trackpad_glide_scrolls_further_than_the_same_wheel_delta() {
    let _serial = serial();

    // a wheel notch and a pixel of finger travel are different units;
    // the gain is what reconciles them. three notches of wheel must land
    // exactly where one notch-equivalent of glide does at gain 3
    let text = "line\n".repeat(200);
    let mut glided = boot();
    let mut notched = boot();
    for (editor, delta, phase) in [
        (&mut glided, -1.0, Some("moved")),
        (&mut notched, -3.0, None),
    ] {
        ctrl(editor, "n");
        editor.run_steps(50);
        editor.push_event(Event::TextInput(text.clone().into()));
        editor.set_focus(false);
        editor.run_steps(300);
        let (_, w, h) = editor.last_frame();
        editor.push_event(Event::MouseMoved(w / 2, h / 2, 0, 0));
        editor.run_steps(50);
        editor.push_event(Event::MouseWheel(0.0, delta, phase));
        editor.run_steps(500);
    }

    assert_eq!(
        glided.last_frame().0,
        notched.last_frame().0,
        "a glide is not gained up to match the wheel"
    );
}
