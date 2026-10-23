//! AltGr typing (#1551). A Norwegian, German or Polish layout types `@`, `{`, `[`, `$`, `€` or `ł`
//! with AltGr, so AltGr must type at the command line and never run a Ctrl shortcut.
//!
//! Windows reports AltGr as Ctrl and Alt held together, and so do browsers on Windows. eframe's
//! web runner drops the text of a key pressed with Ctrl held, so the web shell hears those
//! characters itself and hands them to [`restore_typed`]. Browsers also name keys that type
//! nothing (`AltGraph`, `Dead`), and the web runner passes the name on as text: [`drop_key_names`]
//! removes it. The desktop needs neither: winit leaves Ctrl and Alt out of AltGr's modifiers on
//! Windows, and Linux reports AltGr as its own modifier.

use egui::{Event, Key, Modifiers, RawInput};

/// The most characters kept for one frame.
pub const MAX_TYPED: usize = 64;

/// Ctrl and Alt held together, which is how Windows reports AltGr.
pub fn is_altgr(m: Modifiers) -> bool {
    m.ctrl && m.alt
}

/// The character a browser key value (`KeyboardEvent.key`) types with Ctrl and Alt held, such as
/// `@` for AltGr+2 on a Norwegian layout. Letters and digits are left out: Ctrl+Alt+A on a layout
/// without AltGr reports `a`, and that is a shortcut, not typing.
pub fn typed_char(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let c = chars.next()?;
    let typed = chars.next().is_none() && !c.is_control() && !c.is_whitespace() && !c.is_ascii_alphanumeric();
    typed.then_some(c)
}

/// A character typed with Ctrl and Alt held, and the physical key that typed it (from
/// `KeyboardEvent.code`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Typed {
    pub physical: Option<Key>,
    pub text: String,
}

/// Put characters typed with AltGr back into the input as text, each right after its key press
/// so the typing order holds. That key press loses Ctrl and Alt, so nothing reads it as a
/// shortcut. Not on a Mac, where Option types and Ctrl+Option is not AltGr.
pub fn restore_typed(ctx: &egui::Context, raw: &mut RawInput, typed: Vec<Typed>) {
    if ctx.os() == egui::os::OperatingSystem::Mac {
        return;
    }
    let mut from = 0;
    for t in typed.into_iter().take(MAX_TYPED) {
        let press = raw.events.iter_mut().enumerate().skip(from).find_map(|(i, e)| match e {
            Event::Key { physical_key, pressed: true, modifiers, .. }
                if physical_key.is_some() && *physical_key == t.physical && is_altgr(*modifiers) =>
            {
                *modifiers = Modifiers { alt: false, ctrl: false, command: false, ..*modifiers };
                Some(i)
            }
            _ => None,
        });
        let at = press.map_or(raw.events.len(), |i| i + 1);
        raw.events.insert(at, Event::Text(t.text));
        from = at + 1;
    }
}

/// Drop text that is a browser's name for a key (`AltGraph` for AltGr, `Dead`, `Unidentified`)
/// while no text field has the keyboard. Run it before the app adds its own events.
pub fn drop_key_names(ctx: &egui::Context, raw: &mut RawInput) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    raw.events.retain(|e| !matches!(e, Event::Text(t) if is_key_name(t)));
}

/// A key name: two or more ASCII letters and digits, the first a capital. A key that types
/// something reports a single character instead.
fn is_key_name(t: &str) -> bool {
    t.len() > 1 && t.starts_with(|c: char| c.is_ascii_uppercase()) && t.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALTGR: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };

    fn press(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers }
    }

    fn windows() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_os(egui::os::OperatingSystem::Windows);
        ctx
    }

    fn text(t: &str) -> Event {
        Event::Text(t.into())
    }

    #[test]
    fn altgr_characters_are_typed_and_shortcut_letters_are_not() {
        for key in ["@", "{", "[", "]", "}", "$", "|", "€", "£", "ł", "ą", "~", "\\"] {
            assert_eq!(typed_char(key).map(String::from), Some(key.to_string()), "{key}");
        }
        for key in ["a", "A", "2", "", " ", "\u{a0}", "\t", "AltGraph", "Dead", "Unidentified", "Control", "@@"] {
            assert_eq!(typed_char(key), None, "{key:?}");
        }
    }

    #[test]
    fn key_names_are_dropped_and_typed_text_stays() {
        let ctx = egui::Context::default();
        let mut raw = RawInput {
            events: ["AltGraph", "@", "Dead", "é", "Unidentified", "ß", "A", "ab", "10", "Æ", "€"].map(text).to_vec(),
            ..Default::default()
        };
        drop_key_names(&ctx, &mut raw);
        assert_eq!(raw.events, ["@", "é", "ß", "A", "ab", "10", "Æ", "€"].map(text).to_vec());
    }

    #[test]
    fn restored_text_follows_its_key_press_and_the_press_loses_ctrl_and_alt() {
        // "1", AltGr+2 (`@`, its text dropped), "0", as eframe's web runner reports them on Windows.
        let mut raw = RawInput {
            events: vec![press(Key::Num1, Modifiers::NONE), text("1"), press(Key::Num2, ALTGR), press(Key::Num0, Modifiers::NONE), text("0")],
            ..Default::default()
        };
        restore_typed(&windows(), &mut raw, vec![Typed { physical: Some(Key::Num2), text: "@".into() }]);
        let want = vec![
            press(Key::Num1, Modifiers::NONE),
            text("1"),
            press(Key::Num2, Modifiers::NONE),
            text("@"),
            press(Key::Num0, Modifiers::NONE),
            text("0"),
        ];
        assert_eq!(raw.events, want);
    }

    #[test]
    fn repeated_altgr_presses_keep_their_order() {
        let mut raw = RawInput { events: vec![press(Key::Num7, ALTGR), press(Key::Num0, ALTGR), press(Key::Num7, ALTGR)], ..Default::default() };
        let typed =
            ["{", "}", "{"].iter().zip([Key::Num7, Key::Num0, Key::Num7]).map(|(t, k)| Typed { physical: Some(k), text: t.to_string() }).collect();
        restore_typed(&windows(), &mut raw, typed);
        let texts: String = raw.events.iter().filter_map(|e| if let Event::Text(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(texts, "{}{");
        assert_eq!(raw.events.get(1), Some(&text("{")));
        assert_eq!(raw.events.get(3), Some(&text("}")));
    }

    #[test]
    fn unmatched_text_is_added_at_the_end_and_macs_are_left_alone() {
        let mut raw = RawInput { events: vec![press(Key::A, Modifiers::NONE)], ..Default::default() };
        restore_typed(&windows(), &mut raw, vec![Typed { physical: None, text: "@".into() }]);
        assert_eq!(raw.events, vec![press(Key::A, Modifiers::NONE), text("@")]);

        let mac = egui::Context::default();
        mac.set_os(egui::os::OperatingSystem::Mac);
        let mut raw = RawInput { events: vec![press(Key::Num2, ALTGR)], ..Default::default() };
        restore_typed(&mac, &mut raw, vec![Typed { physical: Some(Key::Num2), text: "™".into() }]);
        assert_eq!(raw.events, vec![press(Key::Num2, ALTGR)]);
    }

    #[test]
    fn a_frame_keeps_at_most_max_typed_characters() {
        let mut raw = RawInput::default();
        let typed = vec![Typed { physical: None, text: "@".into() }; MAX_TYPED + 10];
        restore_typed(&windows(), &mut raw, typed);
        assert_eq!(raw.events.len(), MAX_TYPED);
    }
}
