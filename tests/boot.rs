//! end-to-end tests: the real, unmodified editor lua booting and running
//! on a headless wisp core with a virtual clock. no window, no display,
//! fully deterministic.

use wisp::headless::Headless;
use wisp::platform::Event;

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
}

#[test]
fn clipboard_round_trips_through_the_editor() {
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
