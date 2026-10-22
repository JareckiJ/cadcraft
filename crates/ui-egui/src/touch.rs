//! A virtual mouse for touch screens.
//!
//! The web backend reports each touch as [`egui::Event::Touch`] and also synthesizes pointer
//! events (move, primary press/release, gone) for the first finger, so every finger that lands
//! clicks. [`TouchMouse::filter`] drops those synthesized pointer events for touch sequences that
//! start on the drawing canvas and appends its own pointer events instead, all at a crosshair it
//! keeps across frames:
//!
//! - one-finger drag moves the crosshair by the finger's motion (trackpad style), no buttons;
//! - a tap within [`NEAR`] of the crosshair clicks at the crosshair, once the double-tap window
//!   has passed; a tap elsewhere moves the crosshair to the tap, no click;
//! - a second tap of a double tap on the crosshair double-clicks;
//! - tap then touch-and-drag presses at the crosshair once the finger passes [`SLOP`], drags it,
//!   and releases on lift;
//! - two or more fingers make the sequence a gesture (pinch zoom, two-finger pan read from the
//!   touch events themselves): no buttons until every finger has lifted.
//!
//! Touch sequences that start off the canvas (menus, panels, dialogs over the canvas) and mouse
//! input pass through unchanged.

use std::collections::VecDeque;

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, TouchId, TouchPhase, Vec2};

/// A finger that moves no further than this (points) can still be a tap.
pub const SLOP: f32 = 10.0;
/// A tap lasts less than this (seconds).
pub const TAP_TIME: f64 = 0.25;
/// A tap this close to the crosshair (points) clicks it; a second touch this close to a tap
/// continues a double tap.
pub const NEAR: f32 = 40.0;
/// A second touch within this long after a tap (seconds) continues a double tap.
pub const DOUBLE_TIME: f64 = 0.3;
/// A one-finger touch moves nothing before this long (seconds), so a second finger landing a
/// little late still makes a gesture with nothing to undo.
pub const SETTLE_TIME: f64 = 0.08;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Not yet a drag: may still be a tap or become a gesture.
    Open,
    /// One finger moving the crosshair.
    Drag,
    /// One finger dragging with the primary button held (double-tap-drag).
    Press,
    /// Two or more fingers: ignored until every finger lifts.
    Gesture,
}

#[derive(Clone, Copy, Debug)]
struct Seq {
    primary: TouchId,
    start_time: f64,
    start: Pos2,
    last: Pos2,
    moved: f32,
    class: Class,
    /// Began within [`DOUBLE_TIME`] and [`NEAR`] of the previous tap.
    double: bool,
    /// This touch placed the first crosshair, so its tap only places it.
    placed: bool,
}

#[derive(Clone, Copy, Debug)]
struct Tap {
    time: f64,
    pos: Pos2,
    /// The tap clicked (rather than moved the crosshair).
    clicked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Idle,
    Canvas,
    Pass,
}

/// The virtual-mouse state machine (see the module docs). Holds no egui context.
#[derive(Debug)]
pub struct TouchMouse {
    crosshair: Option<Pos2>,
    mode: Mode,
    fingers: Vec<TouchId>,
    seq: Option<Seq>,
    last_tap: Option<Tap>,
    /// A tap's click waiting out the double-tap window: when it fires.
    pending: Option<f64>,
    out: VecDeque<Event>,
    modifiers: Modifiers,
    /// The touch event being filtered ended a canvas sequence.
    ended_on_canvas: bool,
}

impl Default for TouchMouse {
    fn default() -> Self {
        Self {
            crosshair: None,
            mode: Mode::Idle,
            fingers: Vec::new(),
            seq: None,
            last_tap: None,
            pending: None,
            out: VecDeque::new(),
            modifiers: Modifiers::default(),
            ended_on_canvas: false,
        }
    }
}

fn is_synthesized_pointer(e: &Event) -> bool {
    matches!(e, Event::PointerMoved(_) | Event::PointerGone | Event::PointerButton { button: PointerButton::Primary, .. })
}

impl TouchMouse {
    /// The crosshair the virtual mouse points at, once a canvas touch has placed it.
    pub fn crosshair(&self) -> Option<Pos2> {
        self.crosshair
    }

    /// Whether frames must keep coming without input (a click waiting out the double-tap
    /// window, or queued pointer events).
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.out.is_empty()
    }

    /// Rewrite one frame's `events` at time `now` (seconds). `on_canvas` says whether a touch
    /// starting at a point starts on the canvas; `canvas` (when known) bounds the crosshair.
    pub fn filter(&mut self, events: &mut Vec<Event>, now: f64, canvas: Option<Rect>, on_canvas: &dyn Fn(Pos2) -> bool) {
        let input = std::mem::take(events);
        let mut held: Vec<Event> = Vec::new();
        for e in input {
            if let Event::ModifiersChanged(m) = e {
                self.modifiers = m;
            }
            if is_synthesized_pointer(&e) {
                held.push(e);
                continue;
            }
            if let Event::Touch { id, phase, pos, .. } = e {
                self.touch(id, phase, pos, now, canvas, on_canvas);
                // The pointer events the backend made for this touch precede it.
                if self.mode == Mode::Canvas || self.ended_on_canvas {
                    held.clear();
                } else {
                    events.append(&mut held);
                }
                self.ended_on_canvas = false;
            }
            events.push(e);
        }
        events.append(&mut held);
        self.expire(now);
        self.drain(events);
    }

    fn touch(&mut self, id: TouchId, phase: TouchPhase, pos: Pos2, now: f64, canvas: Option<Rect>, on_canvas: &dyn Fn(Pos2) -> bool) {
        match phase {
            TouchPhase::Start => {
                if self.fingers.is_empty() {
                    self.mode = if on_canvas(pos) { Mode::Canvas } else { Mode::Pass };
                    if self.mode == Mode::Canvas {
                        self.begin(id, pos, now, canvas);
                    }
                }
                if !self.fingers.contains(&id) {
                    self.fingers.push(id);
                }
                if self.mode == Mode::Canvas && self.fingers.len() > 1 {
                    self.gesture();
                }
            }
            TouchPhase::Move => {
                if self.mode == Mode::Canvas {
                    self.moved(id, pos, now, canvas);
                }
            }
            TouchPhase::End | TouchPhase::Cancel => {
                self.fingers.retain(|f| *f != id);
                if self.mode == Mode::Canvas && self.seq.is_some_and(|s| s.primary == id) {
                    self.lift(pos, now, canvas, phase == TouchPhase::Cancel);
                }
                if self.fingers.is_empty() {
                    self.ended_on_canvas = self.mode == Mode::Canvas;
                    self.mode = Mode::Idle;
                    self.seq = None;
                }
            }
        }
    }

    fn begin(&mut self, id: TouchId, pos: Pos2, now: f64, canvas: Option<Rect>) {
        let placed = self.crosshair.is_none();
        let cross = match self.crosshair {
            Some(c) => c,
            None => clamp(pos, canvas),
        };
        self.crosshair = Some(cross);
        // Restore hover after a touch elsewhere (the backend ends those with PointerGone).
        self.out.push_back(Event::PointerMoved(cross));
        let double = self.last_tap.is_some_and(|t| now - t.time <= DOUBLE_TIME && t.pos.distance(pos) <= NEAR);
        if !double {
            self.fire_pending();
        }
        self.seq = Some(Seq { primary: id, start_time: now, start: pos, last: pos, moved: 0.0, class: Class::Open, double, placed });
    }

    fn gesture(&mut self) {
        let Some(seq) = self.seq.as_mut() else { return };
        let was = seq.class;
        seq.class = Class::Gesture;
        match was {
            // A double tap's first click still stands.
            Class::Open => self.fire_pending(),
            Class::Press => self.button(false),
            Class::Drag | Class::Gesture => {}
        }
        self.last_tap = None;
    }

    fn moved(&mut self, id: TouchId, pos: Pos2, now: f64, canvas: Option<Rect>) {
        let Some(mut seq) = self.seq else { return };
        if seq.primary != id || seq.class == Class::Gesture {
            return;
        }
        seq.moved = seq.moved.max(seq.start.distance(pos));
        let delta = match seq.class {
            Class::Open if seq.moved > SLOP && now - seq.start_time >= SETTLE_TIME => {
                if seq.double {
                    // Tap then drag: the drag replaces the first tap's click.
                    self.pending = None;
                    self.button(true);
                    seq.class = Class::Press;
                } else {
                    seq.class = Class::Drag;
                }
                pos - seq.start
            }
            Class::Open | Class::Gesture => Vec2::ZERO,
            Class::Drag | Class::Press => pos - seq.last,
        };
        if seq.class != Class::Open {
            seq.last = pos;
        }
        self.seq = Some(seq);
        if delta != Vec2::ZERO {
            self.nudge(delta, canvas);
        }
    }

    fn lift(&mut self, pos: Pos2, now: f64, canvas: Option<Rect>, cancelled: bool) {
        let Some(seq) = self.seq else { return };
        match seq.class {
            Class::Gesture => {}
            Class::Press => {
                self.button(false);
                self.last_tap = None;
            }
            Class::Drag => self.last_tap = None,
            Class::Open => {
                let tap = !cancelled && seq.moved.max(seq.start.distance(pos)) <= SLOP && now - seq.start_time < TAP_TIME;
                if !tap {
                    self.fire_pending();
                    if !cancelled && seq.start.distance(pos) > SLOP {
                        self.nudge(pos - seq.start, canvas);
                    }
                    self.last_tap = None;
                    return;
                }
                let first_clicked = seq.double && self.last_tap.is_some_and(|t| t.clicked);
                if first_clicked && self.pending.is_some() {
                    // Second tap of a double tap on the crosshair: a double click.
                    self.fire_pending();
                    self.click();
                    self.last_tap = None;
                    return;
                }
                self.fire_pending();
                let near = !seq.placed && self.crosshair.is_some_and(|c| c.distance(pos) <= NEAR);
                if near {
                    self.pending = Some(now + DOUBLE_TIME);
                } else {
                    let c = clamp(pos, canvas);
                    self.crosshair = Some(c);
                    self.out.push_back(Event::PointerMoved(c));
                }
                self.last_tap = Some(Tap { time: now, pos, clicked: near });
            }
        }
    }

    fn nudge(&mut self, delta: Vec2, canvas: Option<Rect>) {
        let Some(c) = self.crosshair else { return };
        let c = clamp(c + delta, canvas);
        self.crosshair = Some(c);
        self.out.push_back(Event::PointerMoved(c));
    }

    fn button(&mut self, pressed: bool) {
        let Some(pos) = self.crosshair else { return };
        self.out.push_back(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: self.modifiers });
    }

    fn click(&mut self) {
        self.button(true);
        self.button(false);
    }

    fn fire_pending(&mut self) {
        if self.pending.take().is_some() {
            self.click();
        }
    }

    /// Fire a waiting click once its window has passed, unless a touch that may continue the
    /// double tap is still down.
    fn expire(&mut self, now: f64) {
        let deciding = self.seq.is_some_and(|s| s.double && s.class == Class::Open);
        if !deciding && self.pending.is_some_and(|t| now >= t) {
            self.fire_pending();
        }
    }

    /// Move queued events into this frame: at most one button event per frame, so a press and
    /// its release always reach egui in separate frames.
    fn drain(&mut self, events: &mut Vec<Event>) {
        let mut buttons = 0;
        while let Some(e) = self.out.front() {
            if matches!(e, Event::PointerButton { .. }) {
                if buttons > 0 {
                    break;
                }
                buttons += 1;
            }
            if let Some(e) = self.out.pop_front() {
                if let Event::PointerButton { pos, .. } = e {
                    events.push(Event::PointerMoved(pos));
                }
                events.push(e);
            }
        }
    }
}

fn clamp(p: Pos2, canvas: Option<Rect>) -> Pos2 {
    match canvas {
        Some(r) if r.is_positive() => p.clamp(r.min, r.max),
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANVAS: Rect = Rect { min: Pos2::new(0.0, 0.0), max: Pos2::new(800.0, 600.0) };

    /// Drives a [`TouchMouse`] with the event sequences the web backend produces.
    struct Rig {
        m: TouchMouse,
        t: f64,
        out: Vec<Event>,
    }

    impl Rig {
        fn new() -> Self {
            Self { m: TouchMouse::default(), t: 10.0, out: Vec::new() }
        }
        fn frame(&mut self, mut events: Vec<Event>) {
            self.m.filter(&mut events, self.t, Some(CANVAS), &|p| CANVAS.contains(p));
            self.out.extend(events);
        }
        fn wait(&mut self, secs: f64) {
            // Frames keep coming at 60 Hz.
            let end = self.t + secs;
            while self.t < end {
                self.t += 1.0 / 60.0;
                self.frame(Vec::new());
            }
        }
        fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Event {
            Event::Touch { device_id: egui::TouchDeviceId(0), id: TouchId(id), phase, pos: Pos2::new(x, y), force: None }
        }
        fn down(&mut self, id: u64, x: f32, y: f32) {
            let pos = Pos2::new(x, y);
            let press = Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() };
            self.frame(vec![press, Self::touch(id, TouchPhase::Start, x, y)]);
        }
        fn mv(&mut self, id: u64, x: f32, y: f32) {
            self.frame(vec![Event::PointerMoved(Pos2::new(x, y)), Self::touch(id, TouchPhase::Move, x, y)]);
        }
        fn up(&mut self, id: u64, x: f32, y: f32) {
            let pos = Pos2::new(x, y);
            let release = Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() };
            self.frame(vec![release, Event::PointerGone, Self::touch(id, TouchPhase::End, x, y)]);
        }
        fn tap(&mut self, x: f32, y: f32) {
            self.down(1, x, y);
            self.t += 0.1;
            self.up(1, x, y);
        }
        /// Every button event emitted so far: (pressed, pos).
        fn buttons(&self) -> Vec<(bool, Pos2)> {
            self.out
                .iter()
                .filter_map(|e| match e {
                    Event::PointerButton { pressed, pos, .. } => Some((*pressed, *pos)),
                    _ => None,
                })
                .collect()
        }
        fn clicks(&self) -> usize {
            self.buttons().iter().filter(|(p, _)| *p).count()
        }
        fn gone(&self) -> bool {
            self.out.iter().any(|e| matches!(e, Event::PointerGone))
        }
        fn last_move(&self) -> Option<Pos2> {
            self.out.iter().rev().find_map(|e| match e {
                Event::PointerMoved(p) => Some(*p),
                _ => None,
            })
        }
        /// A rig whose crosshair sits at (100, 100).
        fn at_100() -> Self {
            let mut r = Self::new();
            r.tap(100.0, 100.0);
            r.wait(0.5);
            r.out.clear();
            r
        }
    }

    #[test]
    fn first_touch_places_crosshair_without_clicking() {
        let mut r = Rig::new();
        r.tap(100.0, 100.0);
        r.wait(0.5);
        assert_eq!(r.clicks(), 0);
        assert_eq!(r.m.crosshair(), Some(Pos2::new(100.0, 100.0)));
        assert!(!r.gone(), "the crosshair stays drawn after a lift");
    }

    #[test]
    fn tap_near_crosshair_clicks_at_crosshair_after_the_window() {
        let mut r = Rig::at_100();
        r.tap(120.0, 110.0);
        assert_eq!(r.clicks(), 0, "deferred by the double-tap window");
        assert!(r.m.busy());
        r.wait(0.4);
        let c = Pos2::new(100.0, 100.0);
        assert_eq!(r.buttons(), vec![(true, c), (false, c)]);
        assert!(!r.m.busy());
    }

    #[test]
    fn press_and_release_reach_egui_in_separate_frames() {
        let mut r = Rig::at_100();
        r.tap(100.0, 100.0);
        r.t += 0.4;
        let mut frames = Vec::new();
        for _ in 0..3 {
            let mut ev = Vec::new();
            r.m.filter(&mut ev, r.t, Some(CANVAS), &|p| CANVAS.contains(p));
            frames.push(ev.iter().filter(|e| matches!(e, Event::PointerButton { .. })).count());
        }
        assert_eq!(frames, vec![1, 1, 0]);
    }

    #[test]
    fn tap_far_moves_crosshair_without_clicking() {
        let mut r = Rig::at_100();
        r.tap(500.0, 300.0);
        r.wait(0.5);
        assert_eq!(r.clicks(), 0);
        assert_eq!(r.m.crosshair(), Some(Pos2::new(500.0, 300.0)));
        assert_eq!(r.last_move(), Some(Pos2::new(500.0, 300.0)));
    }

    #[test]
    fn one_finger_drag_moves_crosshair_relatively_without_buttons() {
        let mut r = Rig::at_100();
        r.down(1, 400.0, 400.0);
        for i in 1..=10 {
            r.t += 0.02;
            r.mv(1, 400.0 + 5.0 * i as f32, 400.0 - 3.0 * i as f32);
        }
        r.t += 0.02;
        r.up(1, 450.0, 370.0);
        r.wait(0.5);
        assert!(r.buttons().is_empty());
        assert_eq!(r.m.crosshair(), Some(Pos2::new(150.0, 70.0)));
        assert_eq!(r.last_move(), Some(Pos2::new(150.0, 70.0)));
    }

    #[test]
    fn crosshair_stays_on_the_canvas() {
        let mut r = Rig::at_100();
        r.down(1, 400.0, 400.0);
        r.t += 0.1;
        r.mv(1, 0.0, 400.0);
        r.up(1, 0.0, 400.0);
        assert_eq!(r.m.crosshair(), Some(Pos2::new(0.0, 100.0)));
    }

    #[test]
    fn pinch_with_a_late_second_finger_and_staggered_lifts_never_clicks() {
        let mut r = Rig::at_100();
        r.down(1, 300.0, 300.0);
        r.t += 0.05;
        r.mv(1, 306.0, 300.0);
        r.down(2, 500.0, 300.0);
        for i in 1..=10 {
            r.t += 0.02;
            r.mv(1, 306.0 - 10.0 * i as f32, 300.0);
            r.mv(2, 500.0 + 10.0 * i as f32, 300.0);
        }
        r.t += 0.05;
        r.up(1, 206.0, 300.0);
        r.t += 0.15;
        r.mv(2, 590.0, 300.0);
        r.up(2, 600.0, 300.0);
        r.wait(0.5);
        assert!(r.buttons().is_empty());
        assert_eq!(r.m.crosshair(), Some(Pos2::new(100.0, 100.0)), "a gesture leaves the crosshair alone");
        let touches = r.out.iter().filter(|e| matches!(e, Event::Touch { .. })).count();
        assert_eq!(touches, 26, "every touch event still reaches egui for pinch and pan");
    }

    #[test]
    fn last_finger_tapping_up_after_a_gesture_does_not_click() {
        let mut r = Rig::at_100();
        r.down(1, 100.0, 100.0);
        r.down(2, 200.0, 100.0);
        r.t += 0.05;
        r.up(2, 200.0, 100.0);
        r.t += 0.05;
        r.up(1, 100.0, 100.0);
        r.wait(0.5);
        assert!(r.buttons().is_empty());
    }

    #[test]
    fn double_tap_drag_presses_moves_and_releases() {
        let mut r = Rig::at_100();
        r.tap(100.0, 100.0);
        r.t += 0.1;
        r.down(1, 105.0, 100.0);
        for i in 1..=10 {
            r.t += 0.02;
            r.mv(1, 105.0 + 10.0 * i as f32, 100.0 + 5.0 * i as f32);
        }
        r.up(1, 205.0, 150.0);
        r.wait(0.5);
        let b = r.buttons();
        assert_eq!(b, vec![(true, Pos2::new(100.0, 100.0)), (false, Pos2::new(200.0, 150.0))], "the drag replaces the first tap's click");
        // The press comes before any drag motion.
        let press = r.out.iter().position(|e| matches!(e, Event::PointerButton { pressed: true, .. }));
        let moved = r.out.iter().position(|e| matches!(e, Event::PointerMoved(p) if p.x > 100.0));
        assert!(press < moved);
    }

    #[test]
    fn double_tap_on_crosshair_double_clicks() {
        let mut r = Rig::at_100();
        r.tap(100.0, 100.0);
        r.t += 0.1;
        r.tap(102.0, 101.0);
        r.wait(0.5);
        assert_eq!(r.clicks(), 2);
    }

    #[test]
    fn tap_click_pan_tap_click_is_exactly_two_clicks() {
        // DIMLINEAR: click, pan elsewhere with two fingers, move the crosshair, click again.
        let mut r = Rig::at_100();
        r.tap(100.0, 100.0);
        r.t += 0.15;
        r.down(1, 110.0, 110.0);
        r.down(2, 300.0, 110.0);
        for i in 1..=10 {
            r.t += 0.02;
            r.mv(1, 110.0 + 20.0 * i as f32, 110.0);
            r.mv(2, 300.0 + 20.0 * i as f32, 110.0);
        }
        r.up(1, 310.0, 110.0);
        r.up(2, 500.0, 110.0);
        r.t += 0.2;
        r.tap(500.0, 400.0);
        r.wait(0.5);
        r.tap(505.0, 400.0);
        r.wait(0.5);
        let b = r.buttons();
        assert_eq!(r.clicks(), 2, "{b:?}");
        assert_eq!(b.first().map(|x| x.1), Some(Pos2::new(100.0, 100.0)));
        assert_eq!(b.last().map(|x| x.1), Some(Pos2::new(500.0, 400.0)));
    }

    #[test]
    fn touches_off_the_canvas_and_mouse_input_pass_through() {
        let mut m = TouchMouse::default();
        let on = |p: Pos2| p.y < 500.0;
        let pos = Pos2::new(50.0, 550.0);
        let press = Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() };
        let mut ev = vec![press.clone(), Rig::touch(1, TouchPhase::Start, pos.x, pos.y)];
        m.filter(&mut ev, 1.0, Some(CANVAS), &on);
        assert!(ev.contains(&press));
        let mut mouse = vec![Event::PointerMoved(Pos2::new(10.0, 10.0)), press.clone(), Event::PointerGone];
        let before = mouse.clone();
        let mut m = TouchMouse::default();
        m.filter(&mut mouse, 1.0, Some(CANVAS), &on);
        assert_eq!(mouse, before);
    }
}
