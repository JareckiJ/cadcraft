//! Canvas navigation consumes raw events in order, including complete drags in one frame.

use egui::{Context, Event, PointerButton, Pos2, Rect, Response};

/// Navigation includes batched press/move/release frames and the wheel's smoothing tail.
pub(crate) fn navigating(ctx: &Context) -> bool {
    ctx.input(|i| {
        i.pointer.middle_down()
            || i.smooth_scroll_delta != egui::Vec2::ZERO
            || i.zoom_delta() != 1.0
            || i.raw.events.iter().any(|e| matches!(e, Event::PointerButton { button: PointerButton::Middle, .. }))
    })
}

fn overlays(ctx: &Context, response: &Response) -> Vec<Rect> {
    ctx.interactive_rects_last_pass().into_iter().filter(|r| response.rect.contains_rect(*r) && r.area() < response.rect.area() - 1.0).collect()
}

fn allowed(ctx: &Context, response: &Response, rect: Rect, overlays: &[Rect], pos: Pos2) -> bool {
    pos.is_finite()
        && rect.contains(pos)
        && ctx.layer_id_at(pos) == Some(response.layer_id)
        && ctx.memory(|m| m.allows_interaction(response.layer_id))
        && !overlays.iter().any(|r| r.contains(pos))
}

/// Ordered middle-button navigation, including the release position.
#[derive(Default)]
pub(crate) struct Middle {
    frame: Option<u64>,
    last: Option<Pos2>,
}

impl Middle {
    pub fn active(&self) -> bool {
        self.last.is_some()
    }

    pub fn delta(&mut self, ctx: &Context, response: &Response, rect: Rect) -> egui::Vec2 {
        let frame = ctx.cumulative_frame_nr();
        if self.frame == Some(frame) {
            return egui::Vec2::ZERO;
        }
        self.frame = Some(frame);
        if !ctx.input(|i| i.focused) || !response.enabled() {
            self.last = None;
            return egui::Vec2::ZERO;
        }
        let covers = overlays(ctx, response);
        // Context lookups for overlay routing must happen outside the input lock.
        let events: Vec<_> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        Event::PointerButton { button: PointerButton::Middle, .. }
                            | Event::PointerMoved(_)
                            | Event::PointerGone
                            | Event::WindowFocused(false)
                    )
                })
                .cloned()
                .collect()
        });
        let mut delta = egui::Vec2::ZERO;
        for event in events {
            match event {
                Event::PointerButton { pos, button: PointerButton::Middle, pressed: true, .. } => {
                    self.last = allowed(ctx, response, rect, &covers, pos).then_some(pos);
                }
                Event::PointerMoved(pos) | Event::PointerButton { pos, button: PointerButton::Middle, pressed: false, .. } => {
                    if let Some(last) = self.last
                        && pos.is_finite()
                    {
                        delta += pos - last;
                        self.last = Some(pos);
                    }
                    if matches!(event, Event::PointerButton { pressed: false, .. }) || !pos.is_finite() {
                        self.last = None;
                    }
                }
                Event::PointerGone | Event::WindowFocused(false) => self.last = None,
                _ => {}
            }
        }
        if !ctx.input(|i| i.pointer.middle_down()) {
            self.last = None;
        }
        if delta.is_finite() { delta } else { egui::Vec2::ZERO }
    }
}
