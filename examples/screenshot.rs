//! renders the readme screenshots with the same headless editor the
//! tests use: boot on wisp's own lua layer, script some input, dump the
//! framebuffer. run from the repo root:
//!
//!     cargo run --release --example screenshot
//!     magick <out>.ppm <out>.png
//!
//! frames land in .github/ as ppm; converting to png is left to
//! imagemagick so the editor stays free of image dependencies.

use wisp::headless::Headless;
use wisp::platform::Event;

fn press(editor: &Headless, key: &str) {
    editor.push_event(Event::KeyPressed(key.into()));
    editor.push_event(Event::KeyReleased(key.into()));
}

fn dump(editor: &Headless, name: &str) {
    // the editor chdirs into its project dir at boot, so output paths
    // must be anchored to the repo root
    let path = format!("{}/.github/{name}", env!("CARGO_MANIFEST_DIR"));
    let (pixels, w, h) = editor.last_frame();
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    for px in pixels {
        out.extend_from_slice(&[(px >> 16) as u8, (px >> 8) as u8, px as u8]);
    }
    std::fs::write(&path, out).unwrap();
    println!("wrote {path} ({w}x{h})");
}

fn main() {
    std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/.github")).unwrap();
    let mut editor = Headless::boot("data", 1600, 1000, 2.0);
    editor.run_until_frames(1, 10_000);
    editor.run_steps(500);

    // expand the core directory in the treeview (hover, then click)
    editor.push_event(Event::MouseMoved(100, 35, 0, 0));
    editor.run_steps(20);
    editor.push_event(Event::MousePressed("left", 100, 35, 1));
    editor.push_event(Event::MouseReleased("left", 100, 35));
    editor.run_steps(50);
    // park the mouse over the docview so no row shows a hover highlight
    editor.push_event(Event::MouseMoved(900, 500, 0, 0));
    editor.run_steps(50);

    // the command palette over the wordmark, mid-search
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    editor.push_event(Event::KeyPressed("left shift".into()));
    press(&editor, "p");
    editor.push_event(Event::KeyReleased("left shift".into()));
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(60);
    editor.push_event(Event::TextInput("find".into()));
    editor.run_steps(15);
    dump(&editor, "palette.ppm");
    press(&editor, "escape");
    editor.run_steps(60);

    // focus the doc node with a click (the treeview still holds focus
    // from the expand click, and open_doc refuses locked nodes)
    editor.push_event(Event::MousePressed("left", 900, 500, 1));
    editor.push_event(Event::MouseReleased("left", 900, 500));
    editor.run_steps(50);

    // open the theme in the theme, at the color table
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "o");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("core/style.lua".into()));
    editor.run_steps(100);
    press(&editor, "return");
    editor.run_steps(300);
    editor.push_event(Event::KeyPressed("left ctrl".into()));
    press(&editor, "g");
    editor.push_event(Event::KeyReleased("left ctrl".into()));
    editor.run_steps(100);
    editor.push_event(Event::TextInput("30".into()));
    editor.run_steps(50);
    press(&editor, "return");
    editor.run_steps(300);
    // nudge the caret so it is solid, not mid-blink, when captured
    press(&editor, "right");
    editor.run_steps(3);
    dump(&editor, "editor.ppm");
}
