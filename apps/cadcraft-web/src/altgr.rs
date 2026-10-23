//! AltGr on Windows (#1551). Browsers there report AltGr as Ctrl and Alt held, and eframe's web
//! runner drops the character typed with Ctrl held, so AltGr+2 on a Norwegian layout typed no `@`.
//! This listener hears those characters on the drawing canvas, beside eframe's own listener, and
//! the next frame puts them back ([`cadcraft_ui_egui::altgr::restore_typed`]).

use std::cell::Cell;
use std::rc::Rc;

use cadcraft_ui_egui::altgr::{MAX_TYPED, Typed, typed_char};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;

/// Characters typed with Ctrl and Alt held since the last frame.
#[derive(Clone, Default)]
pub struct AltGrKeys(Rc<Cell<Vec<Typed>>>);

impl AltGrKeys {
    /// Listen to key presses on the canvas. A text field's keys go to eframe's hidden input
    /// instead, which types AltGr characters itself.
    pub fn listen(&self, canvas: &web_sys::HtmlCanvasElement) {
        let typed = Rc::clone(&self.0);
        let on_keydown = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
            if !event.ctrl_key() || !event.alt_key() || event.meta_key() {
                return;
            }
            let Some(c) = typed_char(&event.key()) else { return };
            let mut list = typed.take();
            if list.len() < MAX_TYPED {
                list.push(Typed { physical: egui::Key::from_name(&event.code()), text: c.to_string() });
            }
            typed.set(list);
        });
        if canvas.add_event_listener_with_callback("keydown", on_keydown.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_keydown.forget();
        }
    }

    /// The characters typed since the last call.
    pub fn take(&self) -> Vec<Typed> {
        self.0.take()
    }
}
