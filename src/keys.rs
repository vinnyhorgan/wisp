//! translates winit keys into wisp's key names.
//!
//! the names are wisp's spec, inherited verbatim from lite (which got them
//! from sdl_getkeyname, lowercased): the keymap in data/ matches on strings
//! like "left ctrl", "return", "keypad enter". changing any name here
//! silently breaks a keybinding, so the full vocabulary the stock keymap
//! uses is pinned by the tests below.

use winit::keyboard::{Key, KeyLocation, NamedKey};

pub fn key_name(key: &Key, location: KeyLocation) -> Option<String> {
    let side = |left: &str, right: &str| -> Option<String> {
        Some(match location {
            KeyLocation::Right => right.to_owned(),
            _ => left.to_owned(),
        })
    };

    match key {
        Key::Named(named) => {
            let name = match named {
                NamedKey::Control => return side("left ctrl", "right ctrl"),
                NamedKey::Shift => return side("left shift", "right shift"),
                NamedKey::Alt => return side("left alt", "right alt"),
                NamedKey::AltGraph => "right alt",
                NamedKey::Super | NamedKey::Meta => return side("left gui", "right gui"),
                // enter is the one named key that keeps sdl's keypad
                // prefix (the stock keymap binds "keypad enter"); numpad
                // home/end/arrows deliberately name like their standard
                // twins, so numlock-off navigation just works
                NamedKey::Enter => {
                    if location == KeyLocation::Numpad {
                        "keypad enter"
                    } else {
                        "return"
                    }
                }
                NamedKey::Tab => "tab",
                NamedKey::Space => "space",
                NamedKey::Backspace => "backspace",
                NamedKey::Delete => "delete",
                NamedKey::Insert => "insert",
                NamedKey::Escape => "escape",
                NamedKey::Home => "home",
                NamedKey::End => "end",
                NamedKey::PageUp => "pageup",
                NamedKey::PageDown => "pagedown",
                NamedKey::ArrowUp => "up",
                NamedKey::ArrowDown => "down",
                NamedKey::ArrowLeft => "left",
                NamedKey::ArrowRight => "right",
                NamedKey::CapsLock => "capslock",
                NamedKey::NumLock => "numlock",
                NamedKey::ScrollLock => "scrolllock",
                NamedKey::PrintScreen => "printscreen",
                NamedKey::Pause => "pause",
                NamedKey::ContextMenu => "application",
                NamedKey::F1 => "f1",
                NamedKey::F2 => "f2",
                NamedKey::F3 => "f3",
                NamedKey::F4 => "f4",
                NamedKey::F5 => "f5",
                NamedKey::F6 => "f6",
                NamedKey::F7 => "f7",
                NamedKey::F8 => "f8",
                NamedKey::F9 => "f9",
                NamedKey::F10 => "f10",
                NamedKey::F11 => "f11",
                NamedKey::F12 => "f12",
                // sdl named these too; a user keymap must be able to
                // bind the extended function row and the menu key
                NamedKey::F13 => "f13",
                NamedKey::F14 => "f14",
                NamedKey::F15 => "f15",
                NamedKey::F16 => "f16",
                NamedKey::F17 => "f17",
                NamedKey::F18 => "f18",
                NamedKey::F19 => "f19",
                NamedKey::F20 => "f20",
                NamedKey::F21 => "f21",
                NamedKey::F22 => "f22",
                NamedKey::F23 => "f23",
                NamedKey::F24 => "f24",
                _ => return None,
            };
            Some(name.to_owned())
        }
        Key::Character(s) => {
            let lower = s.to_lowercase();
            if location == KeyLocation::Numpad {
                Some(format!("keypad {lower}"))
            } else if lower == " " {
                Some("space".to_owned())
            } else {
                Some(lower)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn named(key: NamedKey) -> String {
        key_name(&Key::Named(key), KeyLocation::Standard).unwrap()
    }

    fn ch(s: &str) -> String {
        key_name(&Key::Character(SmolStr::new(s)), KeyLocation::Standard).unwrap()
    }

    #[test]
    fn every_name_the_stock_keymap_binds() {
        // non-modifier keys referenced by data/core/keymap.lua and plugins
        assert_eq!(named(NamedKey::Enter), "return");
        assert_eq!(named(NamedKey::Escape), "escape");
        assert_eq!(named(NamedKey::Tab), "tab");
        assert_eq!(named(NamedKey::Backspace), "backspace");
        assert_eq!(named(NamedKey::Delete), "delete");
        assert_eq!(named(NamedKey::Insert), "insert");
        assert_eq!(named(NamedKey::Home), "home");
        assert_eq!(named(NamedKey::End), "end");
        assert_eq!(named(NamedKey::PageUp), "pageup");
        assert_eq!(named(NamedKey::PageDown), "pagedown");
        assert_eq!(named(NamedKey::ArrowUp), "up");
        assert_eq!(named(NamedKey::ArrowDown), "down");
        assert_eq!(named(NamedKey::ArrowLeft), "left");
        assert_eq!(named(NamedKey::ArrowRight), "right");
        let fkeys = [
            (NamedKey::F1, "f1"),
            (NamedKey::F2, "f2"),
            (NamedKey::F3, "f3"),
            (NamedKey::F4, "f4"),
            (NamedKey::F5, "f5"),
            (NamedKey::F6, "f6"),
            (NamedKey::F7, "f7"),
            (NamedKey::F8, "f8"),
            (NamedKey::F9, "f9"),
            (NamedKey::F10, "f10"),
            (NamedKey::F11, "f11"),
            (NamedKey::F12, "f12"),
        ];
        for (key, name) in fkeys {
            assert_eq!(named(key), name);
        }
        assert_eq!(named(NamedKey::Space), "space");
        // sdl vocabulary for keys user keymaps may bind
        assert_eq!(named(NamedKey::NumLock), "numlock");
        assert_eq!(named(NamedKey::ContextMenu), "application");
        assert_eq!(named(NamedKey::CapsLock), "capslock");
        // the extended function row: unnamed keys are unbindable, so
        // these must not fall into the catch-all
        assert_eq!(named(NamedKey::F13), "f13");
        assert_eq!(named(NamedKey::F24), "f24");
        for c in "abcdefghijklmnopqrstuvwxyz0123456789[]/\\-=".chars() {
            assert_eq!(ch(&c.to_string()), c.to_string());
        }
    }

    #[test]
    fn modifier_names_match_the_keymap_modkey_map() {
        // keymap.lua maps "left ctrl"/"right ctrl" -> ctrl, "left alt" -> alt,
        // "right alt" -> altgr, "left shift"/"right shift" -> shift
        assert_eq!(
            key_name(&Key::Named(NamedKey::Control), KeyLocation::Left).unwrap(),
            "left ctrl"
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::Control), KeyLocation::Right).unwrap(),
            "right ctrl"
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::Shift), KeyLocation::Left).unwrap(),
            "left shift"
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::Shift), KeyLocation::Right).unwrap(),
            "right shift"
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::Alt), KeyLocation::Left).unwrap(),
            "left alt"
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::Alt), KeyLocation::Right).unwrap(),
            "right alt"
        );
        assert_eq!(
            key_name(&Key::Named(NamedKey::AltGraph), KeyLocation::Right).unwrap(),
            "right alt"
        );
    }

    #[test]
    fn uppercase_characters_are_lowered() {
        assert_eq!(ch("S"), "s");
        assert_eq!(ch("É"), "é");
    }

    #[test]
    fn numpad_keys_get_the_keypad_prefix() {
        assert_eq!(
            key_name(&Key::Named(NamedKey::Enter), KeyLocation::Numpad).unwrap(),
            "keypad enter"
        );
        assert_eq!(
            key_name(&Key::Character(SmolStr::new("7")), KeyLocation::Numpad).unwrap(),
            "keypad 7"
        );
    }
}
