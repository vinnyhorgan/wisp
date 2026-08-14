//! End-to-end tests: the real, unmodified editor Lua booting and running
//! on a headless wisp core with a virtual clock. No window, no display,
//! fully deterministic.

use wisp::headless::Headless;
use wisp::platform::Event;

/// A minimal, stable project directory (its listing is rendered by the
/// treeview, so it must not change between runs).
fn project_dir() -> String {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("project");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hello.txt"), "hello wisp\n").unwrap();
    dir.display().to_string()
}

fn boot() -> Headless {
    let mut editor = Headless::boot(&project_dir(), 900, 600);
    editor.run_until_frames(1, 10_000);
    editor
}

#[test]
fn editor_boots_and_draws_a_frame() {
    let editor = boot();
    let (pixels, w, h) = editor.last_frame();
    assert_eq!(pixels.len(), (w * h) as usize);

    // the frame must actually look like an editor: a dominant background
    // color plus a meaningful amount of text/UI pixels
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
    assert_eq!(editor.window_title(), "lite");
}

#[test]
fn idle_editor_stops_redrawing() {
    // lite's most beloved property, enforced by machine: once quiescent
    // (caret blinking aside, there is no caret without an open doc), the
    // editor must present no new frames.
    let mut editor = boot();
    // let it settle: run plenty of steps for threads and blinking to quiesce
    for _ in 0..2000 {
        if !editor.step() {
            panic!("editor exited while settling");
        }
    }
    let settled = editor.frame_count();
    for _ in 0..2000 {
        editor.step();
    }
    let after = editor.frame_count();
    assert_eq!(settled, after, "idle editor kept redrawing");
}

#[test]
fn typing_in_a_new_doc_appears_on_screen() {
    let mut editor = boot();
    // ctrl+n -> core:new-doc
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("n".into()));
    editor.push_event(Event::KeyReleased("n".into()));
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
    assert_eq!(editor.window_title(), "unsaved* - lite");
}

#[test]
fn quit_event_exits_cleanly() {
    let mut editor = boot();
    editor.push_event(Event::Quit);
    for _ in 0..10_000 {
        if !editor.step() {
            break;
        }
    }
    assert_eq!(
        editor.exited,
        Some(0),
        "quit with no unsaved docs must exit 0"
    );
}
