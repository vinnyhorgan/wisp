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

/// the doc still refuses a binary file -- DEVIATIONS §7 -- even though
/// the hex view now claims every one of them before the doc is asked.
/// the refusal is the last resort, not the answer, and it has to survive
/// as the answer for anything that opens a doc directly
#[test]
fn binary_files_are_refused_by_the_doc() {
    let _serial = serial();
    let root = copy_data_root("binaryroot");
    let blob = root.join("blob.bin");
    std::fs::write(&blob, b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x03\0").unwrap();
    let marker = root.join("refused");
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
local core = require("core")
local ok, err = pcall(core.open_doc, [[{blob}]])
assert(not ok, "the doc opened a binary file")
assert(err:find("binary"), err)
io.open([[{marker}]], "w"):close()
"#,
            blob = blob.display(),
            marker = marker.display()
        ),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    assert!(
        marker.exists(),
        "the doc did not refuse; see the user module"
    );
}

/// a binary file opens in the hex view instead of being turned away: the
/// hex view claims it through `core.file_openers`, which is the door
/// DEVIATIONS §7 left open when it made the refusal the default
#[test]
fn a_binary_file_opens_in_the_hex_view() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("binary");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let blob = dir.join("blob.bin");
    std::fs::write(&blob, b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x03\0").unwrap();

    let mut editor = boot();
    ctrl(&editor, "o");
    editor.run_steps(100);
    editor.push_event(Event::TextInput(blob.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    assert_eq!(editor.exited, None, "opening a binary file must not exit");
    assert_eq!(
        editor.window_title(),
        "blob.bin - wisp",
        "a binary file did not open in the hex view"
    );
}

#[test]
fn wheel_scrolls_the_document() {
    let _serial = serial();
    let mut editor = boot();
    ctrl(&editor, "n");
    editor.run_steps(50);
    // dense lines that differ from each other at every column: a notch
    // is a whole number of lines now, so scrolling lands exactly on a
    // line boundary and a document of repeated text would come back
    // looking almost identical
    let text: String = (0..200)
        .map(|i| {
            let line: String = (0..60)
                .map(|c| (b'a' + ((i + c) % 26) as u8) as char)
                .collect();
            format!("{line}\n")
        })
        .collect();
    editor.push_event(Event::TextInput(text.into()));
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

    // wheel to the end first -- the boot log outgrew a screenful when
    // the language files landed, so the test must not assume it fits --
    // and then keep wheeling: past the last line nothing may move
    editor.push_event(Event::MouseMoved(450, 300, 0, 0));
    editor.run_steps(50);
    for _ in 0..60 {
        editor.push_event(Event::MouseWheel(0.0, -50.0, None));
        editor.run_steps(50);
    }
    editor.run_steps(1000);
    let (bottom, _, _) = editor.last_frame();
    for _ in 0..10 {
        editor.push_event(Event::MouseWheel(0.0, -50.0, None));
        editor.run_steps(50);
    }
    editor.run_steps(1000);
    assert_eq!(
        bottom,
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
    // must match config.trackpad_scroll_gain
    const GAIN: f64 = 1.75;
    let text = "line\n".repeat(200);
    let mut glided = boot();
    let mut notched = boot();
    for (editor, delta, phase) in [
        (&mut glided, -1.0, Some("moved")),
        (&mut notched, -GAIN, None),
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

#[test]
fn a_terminating_signal_rescues_unsaved_work() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("terminated");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "hello wisp\n").unwrap();

    let mut editor = Headless::boot(&dir.display().to_string(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    open_hello_via_treeview(&mut editor);
    editor.push_event(Event::TextInput("dirty".into()));
    editor.run_steps(100);
    assert_eq!(editor.window_title(), "hello.txt* - wisp");

    // a logout or a kill: there is nobody to answer the prompt an
    // ordinary quit would raise, so the editor must not raise one -- and
    // must not take the unsaved edit with it
    editor.push_event(Event::Terminate);
    editor.run_steps(500);
    assert!(editor.exited.is_some(), "the editor ignored a terminate");
    let rescued = std::fs::read_to_string(dir.join("hello.txt~")).expect("no rescue file");
    assert!(
        rescued.starts_with("dirty"),
        "the rescue file lost the edit: {rescued:?}"
    );
    // and the original is untouched: the rescue is a copy, not a save
    assert_eq!(
        std::fs::read_to_string(dir.join("hello.txt")).unwrap(),
        "hello wisp\n"
    );
}

#[test]
fn a_plugin_in_the_user_directory_loads_and_shadows_the_bundled_one() {
    let _serial = serial();
    let root = copy_data_root("userplugin");
    let marker = root.join("loaded.txt");
    std::fs::create_dir_all(root.join("data/user/plugins")).unwrap();

    // same module name as a bundled plugin, and it claims the same
    // command. if both copies loaded, command.add would assert on the
    // duplicate, core.load_plugins would report the error and the editor
    // would open its log; the marker proves it is this copy that ran
    std::fs::write(
        root.join("data/user/plugins/trimwhitespace.lua"),
        format!(
            r#"
local command = require("core.command")
io.open([[{}]], "w"):close()
command.add(nil, {{ ["trim-whitespace:trim-trailing-whitespace"] = function() end }})
"#,
            marker.display()
        ),
    )
    .unwrap();

    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.run_steps(200);
    assert!(marker.exists(), "the user's plugin never loaded");
    assert_ne!(
        editor.window_title(),
        "log - wisp",
        "the bundled plugin loaded too and the command collided"
    );
}

/// `wisp newfile.txt` used to be silently dropped: the argument was
/// neither an existing file nor an existing directory, so the loop in
/// core.init ignored it and you got the project with no buffer at all
#[test]
fn a_path_that_does_not_exist_opens_as_a_file_waiting_to_be_written() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("newfile");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("new.txt");

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);

    // the buffer is there under the name asked for, and it is not dirty:
    // an empty file you have not typed in yet is nothing to be prompted
    // about on quit
    assert_eq!(editor.window_title(), "new.txt - wisp");
    assert!(
        !file.exists(),
        "the file was created before anything was saved"
    );

    // and the first save brings it into being
    editor.push_event(Event::TextInput("written".into()));
    editor.run_steps(100);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "written\n");
    assert_eq!(editor.window_title(), "new.txt - wisp");
}

/// core.open_doc keys its cache on the canonical path, which does not
/// exist for a file that does not exist: every such doc answered nil,
/// and nil == nil made the second new file reuse the first one's doc
#[test]
fn two_files_that_do_not_exist_yet_are_two_separate_docs() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("twonew");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[
            &dir.display().to_string(),
            &dir.join("a.txt").display().to_string(),
            &dir.join("b.txt").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);

    // the last one opened is the one in front. sharing a doc would show
    // a.txt here, in a view that claims to be b.txt's
    assert_eq!(editor.window_title(), "b.txt - wisp");
}

/// a buffer nothing could ever save is worse than a refusal, so a path
/// whose directory is missing is reported instead of opened
#[test]
fn a_path_in_a_directory_that_does_not_exist_is_refused() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("nodir");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[
            &dir.display().to_string(),
            &dir.join("nope").join("x.txt").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);

    // no doc opened, and the log is up front carrying the reason -- the
    // success case for this same argument shows "x.txt - wisp"
    assert_eq!(editor.window_title(), "log - wisp");
    assert!(
        !dir.join("nope").exists(),
        "the missing directory was created"
    );
}

/// helix mode, chapter 1 of `hx --tutor`: hjkl moves a block cursor, i
/// enters insert mode where letters reach the document again, escape
/// leaves it, and d deletes the selection under the block
#[test]
fn helix_mode_moves_a_block_cursor_and_edits_modally() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("helix");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("hx.txt");
    std::fs::write(&file, "abcdef\nghijkl\n").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    palette(&mut editor, "helix: toggle");
    editor.run_steps(100);

    // normal mode swallows typed letters: they are commands now
    editor.push_event(Event::TextInput("zzz".into()));
    editor.run_steps(50);
    assert_eq!(
        editor.window_title(),
        "hx.txt - wisp",
        "typing in normal mode reached the document"
    );

    // l l moves the block right twice, then i opens insert mode there
    press(&editor, "l");
    press(&editor, "l");
    editor.run_steps(50);
    press(&editor, "i");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("XY".into()));
    editor.run_steps(50);
    press(&editor, "escape");
    editor.run_steps(50);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "abXYcdef\nghijkl\n",
        "insert did not land at the block"
    );

    // back in normal mode, d deletes the character under the block
    press(&editor, "d");
    editor.run_steps(50);
    ctrl(&editor, "s");
    editor.run_steps(200);
    let after = std::fs::read_to_string(&file).unwrap();
    assert_eq!(
        after.lines().count(),
        2,
        "d changed the line count: {after:?}"
    );
    assert_eq!(
        after.lines().next().unwrap().len(),
        7,
        "d deleted the wrong amount"
    );
}

/// helix mode, chapter 3 of the tutor: w/e/b select by word, W/E/B by
/// WORD (the tutor's own case is that `one-of-a-kind` takes seven w and
/// one W), counts repeat a motion, c changes, x takes whole lines
#[test]
fn helix_word_motions_counts_and_line_selection() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("helixwords");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("w.txt");
    std::fs::write(&file, "one two three\nalpha-beta gamma\nlast line\n").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    palette(&mut editor, "helix: toggle");
    editor.run_steps(100);

    let save_and_read = |editor: &mut Headless| {
        ctrl(editor, "s");
        editor.run_steps(200);
        std::fs::read_to_string(&file).unwrap()
    };

    // w selects "one " (the word and the gap after it), d deletes it
    press(&editor, "w");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save_and_read(&mut editor).lines().next().unwrap(),
        "two three",
        "w then d did not take one word and its gap"
    );

    // x takes the whole line including its newline, so d removes it
    press(&editor, "x");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save_and_read(&mut editor),
        "alpha-beta gamma\nlast line\n",
        "x then d did not remove the line"
    );

    // W takes the whole hyphenated WORD where w would stop at every dash
    press(&editor, "shift+w");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save_and_read(&mut editor),
        "gamma\nlast line\n",
        "W did not treat one-of-a-kind style text as a single WORD"
    );

    // a count folds the repeats into one selection: 2w crosses two words
    // and the newline between them
    press(&editor, "2");
    press(&editor, "w");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save_and_read(&mut editor),
        "line\n",
        "W did not treat one-of-a-kind style text as a single WORD"
    );
}

/// helix mode, chapter 4 and the `:` line: u undoes, y/p yank and paste
/// through helix's own register, and `:w` saves through the host's
/// command rather than a keybinding
#[test]
fn helix_undo_yank_paste_and_the_ex_line() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("helixex");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("e.txt");
    std::fs::write(&file, "alpha beta\n").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    palette(&mut editor, "helix: toggle");
    editor.run_steps(100);

    // `:` opens a prompt speaking helix's vocabulary, not wisp's palette
    let write = |editor: &mut Headless| {
        press(editor, "shift+;");
        editor.run_steps(100);
        editor.push_event(Event::TextInput("w".into()));
        editor.run_steps(100);
        press(editor, "return");
        editor.run_steps(200);
        std::fs::read_to_string(&file).unwrap()
    };

    press(&editor, "w");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(write(&mut editor), "beta\n", ":w did not save");

    // u puts it back, and the block invariant survives the undo
    press(&editor, "u");
    editor.run_steps(50);
    assert_eq!(write(&mut editor), "alpha beta\n", "u did not undo");

    // x y takes the whole line, p drops it back on a line of its own
    press(&editor, "x");
    press(&editor, "y");
    press(&editor, "p");
    editor.run_steps(50);
    assert_eq!(
        write(&mut editor),
        "alpha beta\nalpha beta\n",
        "a linewise yank did not paste onto its own line"
    );
}

/// helix mode drew two cursors: lite's own thin caret sits at the head,
/// which in helix is one character *past* the block, so a block on a
/// line's newline put a second caret at the start of the line below
#[test]
fn helix_normal_mode_draws_exactly_one_cursor() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("helixcaret");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("c.txt");
    std::fs::write(&file, "ab\ncd\n").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    palette(&mut editor, "helix: toggle");
    editor.run_steps(100);

    // walk the block onto the newline at the end of the first line, the
    // position whose head lands on the line below
    press(&editor, "l");
    press(&editor, "l");
    editor.run_steps(20);

    // catppuccin mocha green, the caret colour. scanned in the document
    // area only: the status bar renders the mode name in the same accent
    let caret = 0x00a6e3a1u32;
    let (pixels, w, _) = editor.last_frame();
    let mut rows: Vec<usize> = Vec::new();
    for y in 0..300usize {
        for x in 200..800usize {
            if pixels[y * w as usize + x] & 0x00ffffff == caret {
                rows.push(y);
                break;
            }
        }
    }
    assert!(!rows.is_empty(), "no cursor drawn at all");
    let span = rows[rows.len() - 1] - rows[0] + 1;
    assert!(
        span <= 21,
        "the cursor covers {span} rows ({:?}..{:?}) -- more than one line, so more than one cursor",
        rows[0],
        rows[rows.len() - 1]
    );
}

/// helix mode ships loaded but inert: wisp is not a modal editor unless
/// someone asks it to be, so the plugin must change nothing at all until
/// `config.helix_mode` or the toggle command turns it on
#[test]
fn helix_mode_is_off_until_it_is_asked_for() {
    let _serial = serial();
    let mut editor = boot();
    editor.push_event(Event::TextInput("hjkl".into()));
    editor.run_steps(100);
    // a modal editor would have swallowed those as commands
    assert_eq!(
        editor.window_title(),
        "wisp",
        "an untouched editor had a document open"
    );
    ctrl(&editor, "n");
    editor.run_steps(100);
    editor.push_event(Event::TextInput("hjkl".into()));
    editor.run_steps(100);
    assert_eq!(
        editor.window_title(),
        "unsaved* - wisp",
        "typed text did not reach the document, so helix mode was on"
    );
}

/// the space prefix used to die on the keystroke that started it: the
/// same call that set it also cleared it, so the key after space always
/// fell through to plain normal mode
#[test]
fn the_helix_space_prefix_survives_to_the_next_key() {
    let _serial = serial();
    // a document has to be in front: with the empty view active there is
    // no buffer, so helix mode is not engaged at all
    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&project_dir(), &format!("{}/hello.txt", project_dir())],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    palette(&mut editor, "helix: toggle");
    editor.run_steps(100);

    // space k reaches the command palette, which then runs a command
    // whose effect is visible in the title
    press(&editor, "space");
    editor.run_steps(50);
    press(&editor, "k");
    editor.run_steps(100);
    editor.push_event(Event::TextInput("core: new doc".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(200);
    assert_eq!(
        editor.window_title(),
        "unsaved - wisp",
        "space k did not reach the command palette"
    );
}

/// binding a command inside a keymap mode used to claim its *displayed*
/// binding, so the empty view advertised `helix-space:f` to everybody --
/// a key that does nothing unless you are in helix mode
#[test]
fn a_mode_binding_does_not_claim_the_key_shown_to_everyone() {
    let _serial = serial();
    let root = copy_data_root("bindingroot");
    std::fs::write(
        root.join("data/user/init.lua"),
        r#"
local keymap = require "core.keymap"
local shown = keymap.get_binding("core:find-file")
assert(shown == "ctrl+p", "find-file advertises " .. tostring(shown))
assert(keymap.get_binding("core:find-command") == "ctrl+shift+p")
io.open(EXEDIR .. "/binding-ok", "wb"):close()
"#,
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    assert!(
        root.join("binding-ok").exists(),
        "a mode binding took over the displayed key; see the user module"
    );
}

/// `w` used to stick on punctuation: it walked to the end of the token
/// under the cursor, and for a one-character token like `(` that walk
/// ended where it began with no gap to cross, so the cursor never got past
#[test]
fn helix_w_walks_over_punctuation_instead_of_sticking() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("helixpunct");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("p.txt");
    std::fs::write(&file, "foo(bar) baz\n").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    palette(&mut editor, "helix: toggle");
    editor.run_steps(100);

    // first w takes "foo", second must land on the "(" rather than
    // re-selecting the tail of the word it is already sitting in
    press(&editor, "w");
    press(&editor, "w");
    press(&editor, "d");
    editor.run_steps(50);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "foobar) baz\n",
        "the second w did not get past the punctuation"
    );

    // and it keeps walking: one w takes "bar", the next takes ") " as its
    // own token plus the gap, and only that last selection is deleted
    press(&editor, "w");
    press(&editor, "w");
    press(&editor, "d");
    editor.run_steps(50);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "foobarbaz\n",
        "w did not keep walking through the punctuation"
    );
}

/// holds left shift around a key press
fn shift(editor: &Headless, key: &str) {
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(editor, key);
    editor.push_event(Event::KeyReleased("left shift".into()));
}

/// holds left alt around a key press
fn alt(editor: &Headless, key: &str) {
    editor.push_event(Event::KeyPressed("left alt".into()));
    press(editor, key);
    editor.push_event(Event::KeyReleased("left alt".into()));
}

/// a printable keystroke exactly as the platform delivers one: the key,
/// and then the text it produced. helix's `f`, `r` and match mode read
/// their character argument off that text event, and core drops the text
/// of a keystroke the keymap claimed -- so both halves have to be sent
fn typed(editor: &Headless, text: &str) {
    editor.push_event(Event::KeyPressed(text.into()));
    editor.push_event(Event::TextInput(text.into()));
    editor.push_event(Event::KeyReleased(text.into()));
}

/// the same, with shift held: `shift+f` is the key `f` typing an `F`
fn shift_typed(editor: &Headless, key: &str, text: &str) {
    editor.push_event(Event::KeyPressed("left shift".into()));
    editor.push_event(Event::KeyPressed(key.into()));
    editor.push_event(Event::TextInput(text.into()));
    editor.push_event(Event::KeyReleased(key.into()));
    editor.push_event(Event::KeyReleased("left shift".into()));
}

/// boots an editor over a one-file project with helix mode turned on
fn helix_editor(name: &str, file: &str, text: &str) -> (Headless, std::path::PathBuf) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(file);
    std::fs::write(&path, text).unwrap();
    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &path.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    palette(&mut editor, "helix: toggle");
    editor.run_steps(100);
    (editor, path)
}

/// helix mode, chapter 4 of the tutor: the `g` prefix goes places --
/// `gg` to the top (or to a counted line), `gh` and `gl` to the ends of
/// the line, `gs` to its first non-blank character
#[test]
fn helix_goto_mode_walks_the_document() {
    let _serial = serial();
    let (mut editor, file) =
        helix_editor("helixgoto", "g.txt", "alpha beta\n  indented line\nlast\n");
    let save = |editor: &mut Headless| {
        ctrl(editor, "s");
        editor.run_steps(200);
        std::fs::read_to_string(&file).unwrap()
    };

    // gs: the first character that is not whitespace, not the first column
    press(&editor, "j");
    press(&editor, "g");
    press(&editor, "s");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor).lines().nth(1).unwrap(),
        "  ndented line",
        "gs did not land on the first non-blank character"
    );

    // gl: the last character of the line, gh: the first
    press(&editor, "g");
    press(&editor, "l");
    press(&editor, "d");
    press(&editor, "g");
    press(&editor, "h");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor).lines().nth(1).unwrap(),
        " ndented lin",
        "gl and gh did not land on the ends of the line"
    );

    // gg: the top of the document, and with a count, that line
    press(&editor, "g");
    press(&editor, "g");
    press(&editor, "d");
    press(&editor, "3");
    press(&editor, "g");
    press(&editor, "g");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "lpha beta\n ndented lin\nast\n",
        "gg did not go to the top of the document, or 3gg to line three"
    );
}

/// helix mode, chapter 6 of the tutor: f and t find a character on the
/// line, F and T do it backwards, r replaces what is selected, `.`
/// repeats the last insertion and alt-. the last find
#[test]
fn helix_finds_characters_and_repeats_itself() {
    let _serial = serial();
    let (mut editor, file) = helix_editor("helixfind", "f.txt", "hello world wide\n");
    let save = |editor: &mut Headless| {
        ctrl(editor, "s");
        editor.run_steps(200);
        std::fs::read_to_string(&file).unwrap()
    };

    // f selects up to and including the character it found
    typed(&editor, "f");
    typed(&editor, "w");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "orld wide\n",
        "f did not select up to the w"
    );

    // an insertion is remembered, and `.` plays it back
    typed(&editor, "i");
    editor.push_event(Event::TextInput("ab".into()));
    editor.run_steps(50);
    press(&editor, "escape");
    editor.run_steps(50);
    press(&editor, ".");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "ababorld wide\n",
        "`.` did not repeat the insertion"
    );

    // r overwrites every character of the selection, newlines aside
    typed(&editor, "r");
    typed(&editor, "Z");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "abZZorld wide\n",
        "r did not replace the selection"
    );

    // alt-. runs the last f again, from where the cursor is now
    alt(&editor, ".");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "abZide\n",
        "alt-. did not repeat the last find"
    );

    // F looks backwards, and takes everything between
    shift_typed(&editor, "f", "F");
    typed(&editor, "a");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(save(&mut editor), "de\n", "F did not select back to the a");
}

/// helix mode, chapter 12 of the tutor: match mode. mm walks to the other
/// end of a pair, mi and ma select what it holds, and ms / mr / md add,
/// swap and remove the pair itself
#[test]
fn helix_match_mode_selects_and_surrounds() {
    let _serial = serial();
    let (mut editor, file) = helix_editor("helixmatch", "m.txt", "foo(bar, baz) end\n");
    let save = |editor: &mut Headless| {
        ctrl(editor, "s");
        editor.run_steps(200);
        std::fs::read_to_string(&file).unwrap()
    };

    // mi( takes what the parentheses hold, and not the parentheses
    for _ in 0..4 {
        press(&editor, "l");
    }
    press(&editor, "m");
    typed(&editor, "i");
    typed(&editor, "(");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "foo() end\n",
        "mi( did not select inside the pair"
    );

    // mm walks from one half of the pair to the other, ma( takes both
    press(&editor, "m");
    press(&editor, "m");
    press(&editor, "m");
    typed(&editor, "a");
    typed(&editor, "(");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "foo end\n",
        "mm then ma( did not take the whole pair"
    );

    // miw takes the word, ms[ wraps it
    press(&editor, "h");
    press(&editor, "m");
    typed(&editor, "i");
    typed(&editor, "w");
    press(&editor, "m");
    typed(&editor, "s");
    typed(&editor, "[");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "[foo] end\n",
        "ms did not surround the selection"
    );

    // mr swaps one pair for another, md takes it away again
    press(&editor, "m");
    typed(&editor, "r");
    typed(&editor, "[");
    typed(&editor, "(");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "(foo) end\n",
        "mr did not replace the surrounding pair"
    );

    press(&editor, "g");
    press(&editor, "h");
    press(&editor, "m");
    typed(&editor, "d");
    typed(&editor, "(");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "foo end\n",
        "md did not remove the surrounding pair"
    );
}

/// helix mode, chapters 7, 10 and 11 of the tutor: the keys that are the
/// host's own edits wearing helix's letters -- J joins, > and < indent,
/// ~ and ` change case, ctrl-a and ctrl-x move a number, ctrl-c comments
#[test]
fn helix_wires_the_hosts_edits_onto_its_own_keys() {
    let _serial = serial();
    let (mut editor, file) = helix_editor("helixedits", "e.lua", "abc def\nghi\nvalue 41\n");
    let save = |editor: &mut Headless| {
        ctrl(editor, "s");
        editor.run_steps(200);
        std::fs::read_to_string(&file).unwrap()
    };

    // ~ swaps the case of everything selected
    press(&editor, "w");
    shift(&editor, "`");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor).lines().next().unwrap(),
        "ABC def",
        "~ did not switch the case of the selection"
    );

    // J pulls the next line up onto this one
    shift(&editor, "j");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "ABC def ghi\nvalue 41\n",
        "J did not join the lines"
    );

    // > indents and < puts it back exactly
    shift(&editor, ".");
    editor.run_steps(50);
    let indented = save(&mut editor);
    assert!(
        indented.starts_with(' ') || indented.starts_with('\t'),
        "> did not indent the line: {indented:?}"
    );
    shift(&editor, ",");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "ABC def ghi\nvalue 41\n",
        "< did not undo what > did"
    );

    // ctrl-a and ctrl-x move the number on the line, and take a count
    press(&editor, "2");
    press(&editor, "g");
    press(&editor, "g");
    ctrl(&editor, "a");
    press(&editor, "5");
    ctrl(&editor, "x");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor).lines().nth(1).unwrap(),
        "value 37",
        "ctrl-a and ctrl-x did not move the number by the count"
    );

    // ctrl-c comments the line in the document's own language
    ctrl(&editor, "c");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor).lines().nth(1).unwrap(),
        "-- value 37",
        "ctrl-c did not comment the line"
    );

    // % takes the whole file. the document's last newline has no
    // position past it, so it survives -- exactly as it does under
    // lite's own select-all and delete
    shift(&editor, "5");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "\n",
        "% did not select the whole document"
    );
}

/// helix mode, chapter 9 of the tutor: `*` puts the selection in the
/// search register, `n` walks the matches and wraps at the end, and the
/// jumplist remembers where you were before a jump
#[test]
fn helix_searches_from_the_selection_and_walks_the_jumplist() {
    let _serial = serial();
    let (mut editor, file) = helix_editor("helixsearch", "s.txt", "alpha\nbeta\nalpha\ngamma\n");
    let save = |editor: &mut Headless| {
        ctrl(editor, "s");
        editor.run_steps(200);
        std::fs::read_to_string(&file).unwrap()
    };

    // miw takes the word, * makes it the search, n finds the next one
    press(&editor, "m");
    typed(&editor, "i");
    typed(&editor, "w");
    shift(&editor, "8");
    press(&editor, "n");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "alpha\nbeta\n\ngamma\n",
        "* then n did not find the second alpha"
    );

    // and n wraps round the end of the document back to the first
    press(&editor, "n");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "\nbeta\n\ngamma\n",
        "n did not wrap back to the first match"
    );

    // ctrl+shift+s parks this spot, 4gg jumps away, ctrl+o comes back
    ctrl_shift(&editor, "s");
    press(&editor, "4");
    press(&editor, "g");
    press(&editor, "g");
    ctrl(&editor, "o");
    press(&editor, "d");
    editor.run_steps(50);
    assert_eq!(
        save(&mut editor),
        "beta\n\ngamma\n",
        "ctrl+o did not return to the parked selection"
    );
}

/// helix mode's view keys: `zt` puts the cursor's line at the top of the
/// view and `zz` centres it, which cannot be the same picture on a
/// document taller than the window
#[test]
fn helix_view_mode_moves_the_view_under_the_cursor() {
    let _serial = serial();
    let text = (1..=200).map(|n| format!("line {n}\n")).collect::<String>();
    let (mut editor, _file) = helix_editor("helixview", "v.txt", &text);
    // an unfocused editor draws no caret, so the frames stop depending
    // on the blink phase
    editor.set_focus(false);

    // the end of the document, put at the top of the view
    press(&editor, "g");
    press(&editor, "e");
    press(&editor, "z");
    press(&editor, "t");
    editor.run_steps(200);
    let top = editor.last_frame();

    // and then centred, which has to move the text
    press(&editor, "z");
    press(&editor, "z");
    editor.run_steps(200);
    assert_ne!(top, editor.last_frame(), "zt and zz drew the same view");
}

/// boots an editor with a binary file open, which the hex view claims
fn hex_editor(name: &str, file: &str, bytes: &[u8]) -> (Headless, std::path::PathBuf) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(file);
    std::fs::write(&path, bytes).unwrap();
    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &path.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    editor.run_steps(100);
    (editor, path)
}

/// the hex view's two panes over one cursor: two digits write a byte on
/// the left, tab moves to the right where a keystroke writes the byte it
/// is, and the whole run undoes as the one burst of typing it was
#[test]
fn the_hex_view_types_bytes_in_both_panes() {
    let _serial = serial();
    let original: Vec<u8> = (0u8..32).collect();
    let (mut editor, file) = hex_editor("hextype", "t.bin", &original);

    // the hex pane takes two digits a byte, then moves on by itself
    for digit in ["f", "f", "4", "1"] {
        typed(&editor, digit);
        editor.run_steps(20);
    }
    // tab hands the keyboard to the text pane, which writes the byte you
    // typed rather than a digit of it
    press(&editor, "tab");
    editor.run_steps(20);
    typed(&editor, "Z");
    editor.run_steps(50);

    ctrl(&editor, "s");
    editor.run_steps(200);
    let mut expected = original.clone();
    expected[0] = 0xff;
    expected[1] = 0x41;
    expected[2] = b'Z';
    assert_eq!(
        std::fs::read(&file).unwrap(),
        expected,
        "typing did not overwrite the bytes under the cursor"
    );

    // and it all comes back: a run of typing is one undo, so a handful
    // of presses is more than enough to reach the file as it was
    for _ in 0..8 {
        ctrl(&editor, "z");
        editor.run_steps(20);
    }
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        original,
        "undo did not put the original bytes back"
    );
}

/// finding, deleting and inserting: the three things that make a hex
/// dump an editor rather than a viewer
#[test]
fn the_hex_view_finds_deletes_and_inserts_bytes() {
    let _serial = serial();
    let (mut editor, file) = hex_editor("hexedit", "e.bin", b"\0hello world\0");

    // a quoted needle is bytes as typed; the match is left selected
    ctrl(&editor, "f");
    editor.run_steps(100);
    editor.push_event(Event::TextInput("\"world\"".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(100);
    press(&editor, "delete");
    editor.run_steps(50);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"\0hello \0",
        "find then delete did not remove the match"
    );

    // ctrl+return makes room for a byte at the cursor, and typing fills it
    ctrl(&editor, "home");
    editor.run_steps(20);
    ctrl(&editor, "return");
    editor.run_steps(20);
    typed(&editor, "5");
    editor.run_steps(20);
    typed(&editor, "8");
    editor.run_steps(20);
    // and ctrl+shift+return adds one to the end
    ctrl_shift(&editor, "return");
    editor.run_steps(50);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"X\0hello \0\0",
        "inserting and appending did not change the file's size"
    );
}

/// the byte buffer across its chunk boundaries, which is where an
/// off-by-one lives if one lives anywhere: an 8k chunk means a file has
/// to be bigger than that before the seams are exercised at all
#[test]
fn the_hex_buffer_reads_and_splices_across_chunks() {
    let _serial = serial();
    let root = copy_data_root("hexbufroot");
    let marker = root.join("buffer-ok");
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
local Buffer = require("plugins.hexview.buffer")
-- 20800 bytes in unique 16-byte blocks, so a needle spanning the seam at
-- 8192 has exactly one match
local parts = {{}}
for i = 1, 1300 do
    parts[i] = string.format("%016d", i)
end
local data = table.concat(parts)
local b = Buffer(data)
assert(b.size == #data)
assert(b:tostring() == data)

-- reads that cross a chunk
assert(b:sub(8188, 8) == data:sub(8189, 8196), "sub across the seam")
assert(b:byte(8191) == data:byte(8192))
assert(b:byte(8192) == data:byte(8193))
assert(b:byte(#data - 1) == data:byte(#data))
assert(b:byte(#data) == nil, "a byte past the end is nil")
assert(b:sub(#data - 2, 10) == data:sub(#data - 1), "a read is clipped, not extended")

-- an overwrite that crosses one, and its undo
b:splice(8189, 4, "WXYZ")
local want = data:sub(1, 8189) .. "WXYZ" .. data:sub(8194)
assert(b.size == #data, "a same-length splice changed the size")
assert(b:tostring() == want, "overwrite across the seam")
assert(b:undo() ~= nil)
assert(b:tostring() == data, "undo of an overwrite across the seam")

-- growing and shrinking, which rebuild
b:splice(10, 0, "!!!")
assert(b.size == #data + 3)
assert(b:tostring() == data:sub(1, 10) .. "!!!" .. data:sub(11))
assert(b:byte(8191) == data:byte(8189), "the bytes after an insert did not move")
b:splice(10, 3, "")
assert(b:tostring() == data, "delete did not undo the insert")

-- searching across the seam, forwards and back
local needle = data:sub(8190, 8200)
assert(b:find(needle, 0) == 8189, "find across the seam")
assert(b:find(needle, 8190) == 8189, "find did not wrap to the only match")
assert(b:rfind(needle, #data) == 8189, "rfind across the seam")
assert(b:rfind(needle, 0) == 8189, "rfind did not wrap")
assert(b:find("no such bytes anywhere", 0) == nil)

-- dirtiness walks back down, so undoing to the saved state is clean
local c = Buffer("abcd")
assert(not c:is_dirty())
c:splice(0, 1, "Z")
assert(c:is_dirty())
c:undo()
assert(not c:is_dirty(), "undoing to the saved bytes left the buffer dirty")

io.open([[{marker}]], "w"):close()
"#,
            marker = marker.display()
        ),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    assert!(
        marker.exists(),
        "the byte buffer is wrong; see the user module"
    );
}

/// the hex view's grid: a pixel resolves to the byte and the pane that is
/// drawn there, mid-row gap and all. this is what a mouse click is
#[test]
fn the_hex_view_resolves_a_pixel_to_a_byte() {
    let _serial = serial();
    let root = copy_data_root("hexgridroot");
    let marker = root.join("grid-ok");
    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r#"
local style = require("core.style")
local HexView = require("plugins.hexview")
local Buffer = require("plugins.hexview.buffer")

local v = HexView(nil)
v.buffer = Buffer(("x"):rep(200))
v.position.x, v.position.y = 0, 0
v.size.x, v.size.y = 2000, 800
local cw, lh = v:get_char_width(), v:get_line_height()
local x0 = style.padding.x

-- the layout this pins: 8 offset columns, a 2-column gap, three columns
-- a byte with one more after the eighth, then the text pane
local function at(col, row)
    return v:resolve_position(x0 + col * cw + 1, row * lh + 1)
end

local off, pane = at(10 + 3 * 3, 2)
assert(off == 2 * 16 + 3 and pane == "hex", "hex pane, row two, byte three")
-- past the wider gap halfway along
off, pane = at(10 + 11 * 3 + 1, 0)
assert(off == 11 and pane == "hex", "hex pane after the mid-row gap")
-- either digit of a byte is that byte
assert((at(10 + 5 * 3 + 1, 0)) == 5, "the low digit resolved to another byte")
off, pane = at(60 + 5, 1)
assert(off == 16 + 5 and pane == "text", "text pane, row one, byte five")
-- clicks off the end of the file land on its last byte
assert((at(60 + 15, 99)) == 199, "a click past the end is not clamped")

io.open([[{marker}]], "w"):close()
"#,
            marker = marker.display()
        ),
    )
    .unwrap();
    let mut editor =
        Headless::boot_with_exedir(&root.display().to_string(), &project_dir(), 900, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    assert!(
        marker.exists(),
        "the hex grid is wrong; see the user module"
    );
}

/// the hex view holds a whole file in memory, so it refuses one that is
/// too large instead of stopping the editor dead with no way back
#[test]
fn an_enormous_binary_is_refused_rather_than_loaded() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("hexhuge");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let blob = dir.join("huge.img");
    // sparse: the bytes are nulls, so it reads as binary and costs no disk
    std::fs::File::create(&blob)
        .unwrap()
        .set_len(200 * 1024 * 1024)
        .unwrap();

    let mut editor = boot();
    ctrl(&editor, "o");
    editor.run_steps(100);
    editor.push_event(Event::TextInput(blob.display().to_string()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(500);

    assert_eq!(editor.exited, None, "refusing a huge file must not exit");
    assert_eq!(
        editor.window_title(),
        "wisp",
        "a 200mb file was loaded into memory instead of being refused"
    );
}

/// nothing watches the file behind a hex view, so saving asks before it
/// overwrites one that has changed underneath: the only moment a stale
/// buffer can actually cost anything
#[test]
fn saving_a_hex_view_refuses_to_clobber_a_changed_file() {
    let _serial = serial();
    let (mut editor, file) = hex_editor("hexstale", "s.bin", b"\0abcdefgh");

    // edit a byte, then let the file change underneath
    typed(&editor, "f");
    editor.run_steps(20);
    typed(&editor, "f");
    editor.run_steps(20);
    std::fs::write(&file, b"\0ZZZZZZZZ").unwrap();
    let f = std::fs::File::options().write(true).open(&file).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
        .unwrap();
    drop(f);

    // the save asks instead of writing
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"\0ZZZZZZZZ",
        "the save clobbered a file that had changed on disk"
    );

    // and answering yes goes through
    editor.push_event(Event::TextInput("yes".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        b"\xffabcdefgh",
        "confirming the overwrite did not save the buffer"
    );
}

/// counts pixels of one exact colour in the last frame
fn count_color(editor: &Headless, rgb: u32) -> usize {
    editor
        .last_frame()
        .0
        .iter()
        .filter(|px| **px & 0xff_ff_ff == rgb)
        .count()
}

/// a png opens in the image view, not the hex view: it is a binary file
/// too, so the specific claim has to be asked before the universal one.
/// the pixels are counted out of the framebuffer, so this pins the whole
/// path -- claim, decode, fit, draw -- and then the bare `=` zooms it
#[test]
fn a_png_opens_in_the_image_view_and_zooms() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("imgview");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("card.png");
    // 80x80 in four flat quadrants; none of the colours occurs in the theme
    image::RgbaImage::from_fn(80, 80, |x, y| match (x < 40, y < 40) {
        (true, true) => image::Rgba([255, 0, 0, 255]),
        (false, true) => image::Rgba([0, 255, 0, 255]),
        (true, false) => image::Rgba([0, 0, 255, 255]),
        _ => image::Rgba([255, 0, 255, 255]),
    })
    .save(&file)
    .unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    editor.run_steps(300);

    assert_eq!(editor.window_title(), "card.png - wisp");
    // it fits without enlarging, so the quadrant is its own 40x40
    assert_eq!(
        count_color(&editor, 0xff_00_00),
        40 * 40,
        "the png did not draw at one source pixel per ui pixel"
    );

    // `=` is a bare key and only means anything in the image view's mode
    press(&editor, "=");
    editor.run_steps(300);
    assert_eq!(
        count_color(&editor, 0xff_00_00),
        80 * 80,
        "= did not zoom the image one step"
    );

    // and `0` puts it back to fitting the window
    press(&editor, "0");
    editor.run_steps(300);
    assert_eq!(
        count_color(&editor, 0xff_00_00),
        40 * 40,
        "0 did not fit the image back into the window"
    );
}

/// a picture that will not decode is best looked at as the bytes it
/// actually is, so the image view declines it and the hex view behind it
/// takes it -- proven by typing, which only a hex view accepts
#[test]
fn a_broken_png_falls_through_to_the_hex_view() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("imgbroken");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bad.png");
    // the png signature, and then nothing that follows a png's rules
    std::fs::write(&file, b"\x89PNG\r\n\x1a\n\0\0\0\0not a chunk at all").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    editor.run_steps(200);

    typed(&editor, "f");
    editor.run_steps(20);
    typed(&editor, "f");
    editor.run_steps(50);
    assert_eq!(
        editor.window_title(),
        "bad.png* - wisp",
        "a png that would not decode did not fall through to the hex view"
    );
}

/// the image view's bare keys live in a keymap mode of their own, and
/// this is why: an unbound stroke that gains a global binding starts
/// claiming the text event behind it, so a document would stop being able
/// to type the character at all
#[test]
fn a_documents_bare_keys_survive_the_image_views_bindings() {
    let _serial = serial();
    let mut editor = boot();
    ctrl(&editor, "n");
    editor.run_steps(100);
    typed(&editor, "=");
    editor.run_steps(100);
    assert_eq!(
        editor.window_title(),
        "unsaved* - wisp",
        "typing = into a document did nothing; a mode leaked into the plain keymap"
    );
}

/// an image view follows the file the way a document does: iterating on
/// an asset in another program and flipping back to wisp has to show the
/// asset, not the one that was there when it was opened
#[test]
fn an_image_view_reloads_when_the_file_changes() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("imgreload");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("asset.png");
    // red in the top-left quadrant only
    image::RgbaImage::from_fn(80, 80, |x, y| {
        if x < 40 && y < 40 {
            image::Rgba([255, 0, 0, 255])
        } else {
            image::Rgba([0, 0, 255, 255])
        }
    })
    .save(&file)
    .unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[&dir.display().to_string(), &file.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    editor.run_steps(300);
    assert_eq!(count_color(&editor, 0xff_00_00), 40 * 40);

    // the same shape, all red now
    image::RgbaImage::from_pixel(80, 80, image::Rgba([255, 0, 0, 255]))
        .save(&file)
        .unwrap();
    let f = std::fs::OpenOptions::new().write(true).open(&file).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
        .unwrap();
    drop(f);

    // the scan runs every 5 virtual seconds, like autoreload's
    editor.run_steps(5000);
    assert_eq!(
        count_color(&editor, 0xff_00_00),
        80 * 80,
        "the image view kept showing the picture that was there when it opened"
    );
}

/// indentation is a property of the file, not of the editor: one editor,
/// one config, two documents, two answers. this is the affordance a
/// detector writes into -- the user module here stands in for one
#[test]
fn indent_style_is_per_document() {
    let _serial = serial();
    let root = copy_data_root("indentroot");
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("indentproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("wide.txt"), "").unwrap();
    std::fs::write(dir.join("hard.txt"), "").unwrap();
    std::fs::write(dir.join("soft.txt"), "").unwrap();

    std::fs::write(
        root.join("data/user/init.lua"),
        r#"
local Doc = require("core.doc")
local load = Doc.load
function Doc:load(...)
    load(self, ...)
    if self.filename and self.filename:find("hard") then
        self.indent_info = { type = "hard", size = 4, confirmed = true }
    elseif self.filename and self.filename:find("wide") then
        self.indent_info = { type = "soft", size = 2, confirmed = true }
    end
end
"#,
    )
    .unwrap();

    // all three open as tabs, in order, the last one active
    let mut editor = Headless::boot_args(
        &root.display().to_string(),
        &[
            &dir.display().to_string(),
            &dir.join("wide.txt").display().to_string(),
            &dir.join("hard.txt").display().to_string(),
            &dir.join("soft.txt").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    assert_eq!(editor.window_title(), "soft.txt - wisp");

    // the document nobody measured falls back to the config: four spaces
    press(&editor, "tab");
    editor.run_steps(200);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read_to_string(dir.join("soft.txt")).unwrap(),
        "    \n"
    );

    // and the one the detector spoke for gets a tab, in the same editor
    ctrl_shift(&editor, "tab");
    editor.run_steps(200);
    assert_eq!(editor.window_title(), "hard.txt - wisp");
    press(&editor, "tab");
    editor.run_steps(200);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read_to_string(dir.join("hard.txt")).unwrap(),
        "\t\n"
    );

    // size is per-document too, and it is what backspace unindents by: a
    // soft-2 document loses both spaces, not the config's four
    ctrl_shift(&editor, "tab");
    editor.run_steps(200);
    assert_eq!(editor.window_title(), "wide.txt - wisp");
    press(&editor, "tab");
    editor.run_steps(200);
    press(&editor, "backspace");
    editor.run_steps(200);
    ctrl(&editor, "s");
    editor.run_steps(200);
    assert_eq!(
        std::fs::read_to_string(dir.join("wide.txt")).unwrap(),
        "\n",
        "backspace unindented by the config's size instead of the document's"
    );
}

/// autocomplete's symbol scanner walked `while i < #doc.lines`, so the
/// last line of every document was never read and a symbol that lived
/// only there was never suggested. lite-xl fixed the same off-by-one
#[test]
fn autocomplete_sees_symbols_on_the_last_line() {
    let _serial = serial();
    let root = copy_data_root("aclastroot");
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("aclastproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // the symbol under test exists only on the final line
    std::fs::write(
        dir.join("syms.txt"),
        "firstlinesymbol = 1\nxylophonecase = 2\n",
    )
    .unwrap();

    let mut editor = Headless::boot_args(
        &root.display().to_string(),
        &[
            &dir.display().to_string(),
            &dir.join("syms.txt").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    // let the symbol thread walk the open docs at least once
    editor.run_steps(2000);

    // type the prefix in a scratch doc, then collect what each of the
    // offered suggestions completes to. the scratch doc contributes the
    // partial itself as a symbol, so membership is the honest assertion
    ctrl(&editor, "n");
    editor.run_steps(50);
    editor.push_event(Event::TextInput("xylo".into()));
    editor.run_steps(50);

    let mut completions = Vec::new();
    for n in 1..=3 {
        for _ in 1..n {
            press(&editor, "down");
            editor.run_steps(5);
        }
        press(&editor, "tab");
        editor.run_steps(50);
        ctrl(&editor, "a");
        ctrl(&editor, "c");
        editor.run_steps(100);
        completions.push(
            editor
                .engine
                .borrow_mut()
                .platform
                .get_clipboard()
                .unwrap_or_default(),
        );
        // back to the same starting point for the next walk
        ctrl(&editor, "a");
        editor.run_steps(20);
        editor.push_event(Event::TextInput("xylo".into()));
        editor.run_steps(50);
    }

    assert!(
        completions.iter().any(|c| c == "xylophonecase"),
        "the last line's symbol was never suggested: {completions:?}"
    );
}

/// the house rules: whatever comes in, what wisp writes out is utf-8,
/// lf, ends in exactly one newline, and has no trailing whitespace --
/// except in markdown, where two trailing spaces are a hard line break
#[test]
fn saving_normalizes_line_endings_encoding_and_the_end_of_file() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("normproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // crlf endings, a latin-1 byte, trailing spaces, three blank lines
    std::fs::write(
        dir.join("messy.txt"),
        b"one   \r\ntwo \xe9 end\r\n\r\n\r\n".as_slice(),
    )
    .unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[
            &dir.display().to_string(),
            &dir.join("messy.txt").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    assert_eq!(editor.window_title(), "messy.txt - wisp");

    // an edit, so there is something to save
    editor.push_event(Event::TextInput("x".into()));
    editor.run_steps(100);
    ctrl(&editor, "s");
    editor.run_steps(300);

    let out = std::fs::read(dir.join("messy.txt")).unwrap();
    let text = String::from_utf8(out).expect("wisp wrote bytes that are not utf-8");
    assert!(!text.contains('\r'), "crlf survived the save: {text:?}");
    assert!(
        text.ends_with("end\n"),
        "the trailing blank lines were not collapsed to one newline: {text:?}"
    );
    assert!(
        !text.contains("one   \n") && text.contains("xone\n"),
        "trailing whitespace survived the save: {text:?}"
    );
    assert!(
        text.contains('\u{FFFD}'),
        "the invalid byte was not replaced: {text:?}"
    );
}

/// two trailing spaces are a hard line break in markdown, so it is the
/// one format the trim-on-save rule leaves alone
#[test]
fn markdown_keeps_its_hard_line_breaks_on_save() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("mdproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.md"), "a line  \nnext line\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "a line  \nnext line\n").unwrap();

    let open_edit_and_save = |name: &str| -> String {
        let mut editor = Headless::boot_args(
            env!("CARGO_MANIFEST_DIR"),
            &[
                &dir.display().to_string(),
                &dir.join(name).display().to_string(),
            ],
            900,
            600,
            1.0,
        );
        editor.run_until_frames(1, 10_000);
        editor.push_event(Event::TextInput("x".into()));
        editor.run_steps(100);
        ctrl(&editor, "s");
        editor.run_steps(300);
        std::fs::read_to_string(dir.join(name)).unwrap()
    };

    assert_eq!(
        open_edit_and_save("notes.md"),
        "xa line  \nnext line\n",
        "markdown lost a hard line break"
    );
    // and everywhere else the rule still applies
    assert_eq!(open_edit_and_save("notes.txt"), "xa line\nnext line\n");
}

/// every bundled language file, run through the real tokenizer on the
/// real syntax table: `syntax.get` picks the file, `tokenizer.tokenize`
/// produces the tokens, and the editor's own module writes them out.
/// this catches the two ways a ported language file fails silently --
/// a rule that never fires (a pattern wisp's tokenizer cannot express,
/// like a leading `^`) and a token type the theme has no color for,
/// which renders white rather than erroring
#[test]
fn every_language_file_highlights_its_own_extension() {
    let _serial = serial();
    let root = copy_data_root("syntaxroot");
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("syntaxproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "hello wisp\n").unwrap();
    let out = dir.join("tokens.txt");

    std::fs::write(
        root.join("data/user/init.lua"),
        format!(
            r##"
local syntax = require("core.syntax")
local tokenizer = require("core.tokenizer")

local samples = {{
    {{ "a.c", "struct point {{ int x; }};\n" }},
    {{ "a.cpp", "class A {{ public: virtual int f(); }};\n" }},
    {{ "a.h", "namespace n {{ }}\n" }},
    {{ "a.cs", "public class A {{ }}\n" }},
    {{ "a.go", "func main() {{ s := \"x\" }}\n" }},
    {{ "a.java", "public class A {{ }}\n" }},
    {{ "a.rs", "fn main() {{ let c = 'a'; }}\nlet s = r#\"raw\"#;\nfoo::<'static>(1_000u32);\nprintln!(\"hi\");\n" }},
    {{ ".bashrc", "if [ -n \"$HOME\" ]; then echo hi; fi\n" }},
    {{ "Makefile", "all:\n\tcc -o x\n" }},
    {{ "CMakeLists.txt", "add_executable(x ${{SRC}})\n" }},
    {{ "a.html", "<div class=\"x\">hi</div>\n" }},
    {{ "a.xml", "<node attr=\"x\"/>\n" }},
    {{ "a.toml", "[package]\nname = \"wisp\"\nedition = 2024\n" }},
    {{ ".editorconfig", "[*.lua]\nindent_size = 4\n" }},
    {{ "a.json", "{{\"key\": \"value\", \"n\": 1, \"b\": true}}\n" }},
    {{ "a.yml", "key: \"value\"\nlist:\n  - one\n" }},
    {{ ".gitignore", "# ignore\n/target\n!keep\n" }},
    {{ "COMMIT_EDITMSG", "subject line\n\n# comment\n" }},
    {{ "git-rebase-todo", "pick abc1234 message\n" }},
    {{ "a.diff", "--- a\n+++ b\n@@ -1 +1 @@\n-old\n+new\n context\n" }},
    {{ "a.js", "const x = 1;\n" }},
    {{ "a.lua", "local x = 1\n" }},
    {{ "a.py", "def f(): pass\n" }},
    {{ "a.css", "a {{ color: red; }}\n" }},
    {{ "a.md", "# heading\n" }},
}}

local out = {{}}
for _, s in ipairs(samples) do
    local name, text = s[1], s[2]
    local syn = syntax.get(name, text)
    local parts, state = {{ name }}, nil
    for line in text:gmatch("[^\n]*\n") do
        local res
        res, state = tokenizer.tokenize(syn, line, state)
        for _, type, tok in tokenizer.each_token(res) do
            tok = tok:gsub("^%s+", ""):gsub("%s+$", "")
            if tok ~= "" then
                table.insert(parts, type .. ":" .. tok)
            end
        end
    end
    table.insert(out, table.concat(parts, "\t"))
end

local fp = assert(io.open("{out}", "wb"))
fp:write(table.concat(out, "\n"), "\n")
fp:close()
"##,
            out = out.display().to_string().replace('\\', "\\\\"),
        ),
    )
    .unwrap();

    let mut editor = Headless::boot_args(
        &root.display().to_string(),
        &[&dir.display().to_string()],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);

    let dump = std::fs::read_to_string(&out).expect("the user module never wrote its tokens");
    let mut by_file = std::collections::HashMap::new();
    for line in dump.lines() {
        let (name, rest) = line.split_once('\t').unwrap_or((line, ""));
        by_file.insert(name.to_string(), rest.to_string());
    }

    let expect = |name: &str, want: &str| {
        let got = by_file
            .get(name)
            .unwrap_or_else(|| panic!("no tokens for {name}"));
        assert!(
            got.split('\t').any(|t| t == want),
            "{name}: expected token {want:?}, got {got}"
        );
    };

    // the seven lite shipped, still claiming what they always did
    expect("a.c", "keyword:struct");
    expect("a.js", "keyword:const");
    expect("a.lua", "keyword:local");
    expect("a.py", "keyword:def");
    expect("a.css", "keyword:color");
    expect("a.md", "keyword:# heading");
    expect("a.xml", "function:node");

    // c++ claims the headers and wins the overlap with c, which only
    // holds because it requires language_c before registering itself
    expect("a.cpp", "keyword:class");
    expect("a.h", "keyword:namespace");

    expect("a.cs", "keyword:public");
    expect("a.go", "keyword:func");
    expect("a.java", "keyword:class");

    // rust's own literals, none of which rxi's copy-of-go handled: a
    // raw string, a lifetime that is not an unterminated char, an
    // underscored suffixed number, and a macro that keeps its `!`
    expect("a.rs", "keyword:fn");
    expect("a.rs", "string:r#\"raw\"#");
    expect("a.rs", "keyword2:'static");
    expect("a.rs", "number:1_000u32");
    expect("a.rs", "function:println!");
    expect("a.rs", "string:'a'");

    // shell by name as well as by shebang
    expect(".bashrc", "keyword:if");

    expect("Makefile", "function:all:");
    expect("CMakeLists.txt", "function:add_executable");
    expect("a.html", "function:div");

    // the toml table header only fires because the `^` was taken out of
    // it: wisp's tokenizer anchors patterns itself
    expect("a.toml", "keyword:[package]");
    expect("a.toml", "function:name");
    expect("a.toml", "number:2024");
    expect(".editorconfig", "keyword:[*.lua]");

    // json is its own file now, so a key is not just another string
    expect("a.json", "function:\"key\"");
    expect("a.json", "string:\"value\"");
    expect("a.json", "literal:true");

    expect("a.yml", "function:key");
    expect("a.yml", "string:\"value\"");

    expect(".gitignore", "comment:# ignore");
    expect(".gitignore", "keyword:!");

    // a commit message is prose; only the part git strips is colored
    expect("COMMIT_EDITMSG", "comment:# comment");
    expect("COMMIT_EDITMSG", "normal:subject line");
    expect("git-rebase-todo", "keyword:pick");
    expect("git-rebase-todo", "number:abc1234");

    // a diff is decided by the first character of the line, and the
    // catch-all is what keeps a `+` in prose from claiming one
    expect("a.diff", "function:--- a");
    expect("a.diff", "keyword:@@ -1 +1 @@");
    expect("a.diff", "number:-old");
    expect("a.diff", "string:+new");
    expect("a.diff", "normal:context");

    // and nothing anywhere emitted a type the theme has no color for
    let known = [
        "normal", "symbol", "comment", "keyword", "keyword2", "number", "literal", "string",
        "operator", "function",
    ];
    for line in dump.lines() {
        for tok in line.split('\t').skip(1) {
            let kind = tok.split_once(':').unwrap().0;
            assert!(
                known.contains(&kind),
                "unknown token type {kind:?} in {line}"
            );
        }
    }
}

/// detectindent measures the file instead of asking the config: three
/// documents with three different habits, one editor, three answers.
/// rxi's version swapped the global config around every command, which
/// is exactly what DEVIATIONS §21 exists to make unnecessary
#[test]
fn detectindent_measures_the_file_rather_than_asking_the_config() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("detectproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let body = |pad: &str| {
        format!(
            "local a = {{\n{p}b = 1,\n{p}c = {{\n{p}{p}d = 2,\n{p}}},\n}}\n",
            p = pad
        )
    };
    std::fs::write(dir.join("two.lua"), body("  ")).unwrap();
    std::fs::write(dir.join("tabs.lua"), body("\t")).unwrap();
    std::fs::write(dir.join("eight.lua"), body("        ")).unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[
            &dir.display().to_string(),
            &dir.join("two.lua").display().to_string(),
            &dir.join("tabs.lua").display().to_string(),
            &dir.join("eight.lua").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);

    // the active tab is the last one opened; walk back through them,
    // indenting line one of each and saving
    for name in ["eight.lua", "tabs.lua", "two.lua"] {
        assert_eq!(editor.window_title(), format!("{name} - wisp"));
        press(&editor, "tab");
        editor.run_steps(200);
        ctrl(&editor, "s");
        editor.run_steps(200);
        ctrl_shift(&editor, "tab");
        editor.run_steps(200);
    }

    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    assert!(read("two.lua").starts_with("  local"), "two-space file");
    assert!(read("tabs.lua").starts_with("\tlocal"), "tab file");
    assert!(
        read("eight.lua").starts_with("        local"),
        "eight-space file"
    );
}

/// the drawing plugins, each proved by the pixels it adds. the counts
/// are before-and-after within one editor rather than absolute: these
/// colors are shared with the chrome (guides with the scrollbar, the
/// bracket underline with the operator syntax), so only the difference
/// is evidence
#[test]
fn the_drawing_plugins_paint_in_their_own_colors() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("drawproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("code.lua"), "local t = {\naaa = 1,\naaa = 2,\n}\n").unwrap();

    const GUIDE: u32 = 0x45475a; // surface1
    const HIGHLIGHT: u32 = 0x7f849c; // overlay1
    const BRACKET: u32 = 0x89dceb; // sky

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[
            &dir.display().to_string(),
            &dir.join("code.lua").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    editor.set_focus(false);
    editor.run_steps(200);

    let count = |editor: &Headless, rgb: u32| {
        editor
            .last_frame()
            .0
            .iter()
            .filter(|&&p| p & 0xffffff == rgb)
            .count()
    };

    // lineguide's rule is already there; indenting a line must add to it
    let before = count(&editor, GUIDE);
    assert!(before > 0, "lineguide drew no rule");
    press(&editor, "down");
    editor.run_steps(50);
    press(&editor, "tab");
    editor.run_steps(300);
    let after = count(&editor, GUIDE);
    assert!(
        after > before,
        "indentguide drew nothing for an indented line ({before} -> {after})"
    );

    // select one `aaa` and the other one gets boxed
    let before = count(&editor, HIGHLIGHT);
    ctrl(&editor, "d");
    editor.run_steps(300);
    let after = count(&editor, HIGHLIGHT);
    assert!(
        after > before,
        "selectionhighlight boxed nothing ({before} -> {after})"
    );

    // put the caret past the opening brace and its partner is underlined
    ctrl(&editor, "home");
    editor.run_steps(200);
    let before = count(&editor, BRACKET);
    press(&editor, "end");
    editor.run_steps(300);
    let after = count(&editor, BRACKET);
    assert!(
        after > before,
        "bracketmatch underlined nothing ({before} -> {after})"
    );
}

/// copy and cut with nothing selected take the whole line, and pasting
/// one puts it on a line of its own. the flag upstream keeps for this
/// goes stale the moment something else writes the clipboard, so the
/// last assertion is the one that matters
#[test]
fn a_line_copied_with_no_selection_pastes_as_a_line() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("lineclipproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.txt"), "one\ntwo\nthree\n").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[
            &dir.display().to_string(),
            &dir.join("x.txt").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);

    // caret on line one, nothing selected: copy takes the line
    ctrl(&editor, "c");
    editor.run_steps(100);
    press(&editor, "down");
    editor.run_steps(100);
    ctrl(&editor, "v");
    editor.run_steps(200);
    ctrl(&editor, "s");
    editor.run_steps(300);
    assert_eq!(
        std::fs::read_to_string(dir.join("x.txt")).unwrap(),
        "one\none\ntwo\nthree\n",
        "a copied line did not paste as a line"
    );

    // something else writes the clipboard: the next paste is ordinary,
    // landing at the caret instead of opening a line for it
    editor.engine.borrow_mut().platform.set_clipboard("ZZ");
    ctrl(&editor, "home");
    editor.run_steps(100);
    ctrl(&editor, "v");
    editor.run_steps(200);
    ctrl(&editor, "s");
    editor.run_steps(300);
    assert_eq!(
        std::fs::read_to_string(dir.join("x.txt")).unwrap(),
        "ZZone\none\ntwo\nthree\n",
        "a stale line flag hijacked an ordinary paste"
    );
}

/// markers survive the edits above them, which is the whole reason the
/// plugin hooks raw_insert and raw_remove rather than just keeping a
/// set of line numbers
#[test]
fn a_marker_follows_its_line_and_f2_walks_to_it() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("markerproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.txt"), "one\ntwo\nthree\nfour\n").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[
            &dir.display().to_string(),
            &dir.join("x.txt").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);

    // mark "three"
    press(&editor, "down");
    press(&editor, "down");
    editor.run_steps(100);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "f2");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(200);

    // push it down by opening a line at the top, then jump to it
    ctrl(&editor, "home");
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(200);
    ctrl(&editor, "home");
    editor.run_steps(100);
    press(&editor, "f2");
    editor.run_steps(200);
    editor.push_event(Event::TextInput("!".into()));
    editor.run_steps(200);
    ctrl(&editor, "s");
    editor.run_steps(300);

    assert_eq!(
        std::fs::read_to_string(dir.join("x.txt")).unwrap(),
        "\none\ntwo\n!three\nfour\n",
        "the marker did not follow its line"
    );
}

/// drawwhitespace marks two things and no more: the end of the file,
/// always, and whitespace inside a selection, where you asked
#[test]
fn whitespace_is_marked_in_a_selection_and_at_the_end_of_the_file() {
    let _serial = serial();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("wsproj");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("x.txt"), "a b c d e f\nlast line\n").unwrap();

    let mut editor = Headless::boot_args(
        env!("CARGO_MANIFEST_DIR"),
        &[
            &dir.display().to_string(),
            &dir.join("x.txt").display().to_string(),
        ],
        900,
        600,
        1.0,
    );
    editor.run_until_frames(1, 10_000);
    editor.set_focus(false);
    editor.run_steps(200);

    const WHITESPACE: u32 = 0x585b70; // surface2
    let count = |editor: &Headless| {
        editor
            .last_frame()
            .0
            .iter()
            .filter(|&&p| p & 0xffffff == WHITESPACE)
            .count()
    };

    let before = count(&editor);
    assert!(before > 0, "the end of the file was not marked");

    // select the whole first line: its five spaces get dots
    ctrl(&editor, "l");
    editor.run_steps(300);
    let after = count(&editor);
    assert!(
        after > before,
        "a selection's whitespace went unmarked ({before} -> {after})"
    );
}

/// centering is a mode, not a setting: the command moves the text and
/// the same command puts it back
#[test]
fn centerdoc_toggles_and_untoggles() {
    let _serial = serial();
    // wide enough that eighty columns and a treeview leave room to
    // center: in a narrow window centerdoc correctly does nothing
    let mut editor = Headless::boot(&project_dir(), 1600, 600, 1.0);
    editor.run_until_frames(1, 10_000);
    editor.set_focus(false);
    open_hello_via_treeview(&mut editor);
    editor.run_steps(300);

    let plain = editor.last_frame().0;
    palette(&mut editor, "center-doc:toggle");
    editor.run_steps(500);
    let centered = editor.last_frame().0;
    assert_ne!(plain, centered, "centering moved nothing");

    palette(&mut editor, "center-doc:toggle");
    editor.run_steps(500);
    assert_eq!(plain, editor.last_frame().0, "centering did not come back");
}
