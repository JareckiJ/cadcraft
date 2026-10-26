//! The floating command line: history, the active prompt with clickable keywords, typed input
//! (keyboard focus stays on the drawing — typing anywhere goes here) and AutoComplete.

use std::collections::HashSet;

use cadcraft_engine::Input;
use egui::{Key, Pos2, Rect, Sense, Stroke, pos2, vec2};

use crate::CadApp;
use crate::theme::Tokens;

#[derive(Default)]
pub struct CmdLine {
    pub buffer: String,
    /// The selection as of the last frame, to notice when it changes.
    seen_selection: Vec<cadcraft_doc::Handle>,
    /// Something was typed at the command line since the selection was made.
    typed_since_selection: bool,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    /// The AutoComplete row Enter takes (Up/Down/Tab move it; typing puts it back on the first).
    pub suggestion: usize,
    /// Backspace erased the completion shown after the typed name; Up, Down or Tab show it again.
    pub completion_hidden: bool,
    /// The command line's whole area last frame (screen), while it is shown.
    area: Option<Rect>,
    /// The last click went to the command line: Ctrl+V pastes text into it while the pointer
    /// stays over it.
    clicked: bool,
    /// What was typed at command prompts (not command names), oldest first: the in-command
    /// shortcut menu's Recent Input.
    pub recent_input: Vec<String>,
    /// The expanded history window is open (F2, the chevron, or a click on the history lines).
    pub expanded: bool,
    /// Dragging the history's top edge: (pointer y, line count) when the drag started.
    resize_from: Option<(f32, usize)>,
    /// The Dynamic Input options menu at the cursor (Down Arrow at a prompt with keywords).
    pub options: Option<crate::dyninput::OptionsMenu>,
    /// The text whose second Dynamic Input value Tab locked; the lock holds while it is unchanged.
    locked_second: Option<String>,
    /// Where the options menu's rows were drawn last frame (screen), for clicks on them.
    pub option_rows: Vec<(Rect, String)>,
    /// Where the Dynamic Input tooltip stays while the options menu is open.
    pub options_anchor: Option<Pos2>,
    /// The IME's composition (pre-edit) text: shown after the buffer while composing, never saved.
    pub preedit: String,
    /// The command line's editing rectangle and caret (screen) from the last frame, for the IME's
    /// composition window (see `enable_ime`).
    pub ime_rect: Option<(Rect, Rect)>,
    /// The command line's IME context generation: a new one each time it starts collecting text,
    /// as egui asks of an IME owner that (re)gains focus.
    ime_generation: u64,
    /// Whether the command line published an IME context last frame.
    ime_on: bool,
}

/// The most history lines the command line shows above its bar.
pub const MAX_LINES: usize = 12;
const LINE_H: f32 = 16.0;

/// The Dynamic Input frame for the prompt on screen, or `None` when the value boxes are not live
/// (Dynamic Input off, no point prompt or a pending selection window).
pub fn dyn_frame(app: &CadApp) -> Option<crate::dyninput::Frame> {
    let st = &app.session.settings;
    if !st.dynmode || app.session.pending_window.is_some() {
        return None;
    }
    let p = app.session.current_prompt().filter(cadcraft_engine::pointmod::accepts)?;
    Some(crate::dyninput::Frame { base: p.base, cartesian: st.dynpi_cartesian, absolute: st.dynpi_absolute })
}

/// Take Tab away from egui before it moves keyboard focus to the next widget (which would leave
/// the command line deaf to typing). While no widget has focus, a pressed Tab becomes a `"\t"`
/// text event, which keeps its place among the other keystrokes; [`keyboard`] handles it.
pub fn capture_tab(ctx: &egui::Context, raw: &mut egui::RawInput) {
    if ctx.egui_wants_keyboard_input() {
        return;
    }
    raw.events.retain_mut(|e| {
        let tab = match e {
            egui::Event::Key { key: Key::Tab, pressed, .. } => Some(*pressed),
            _ => None,
        };
        match tab {
            Some(true) => {
                *e = egui::Event::Text("\t".into());
                true
            }
            Some(false) => false,
            None => true,
        }
    });
}

/// Tab on the command line: cycle the object snaps under the cursor, type the Dynamic Input
/// separator (`,` or `<`) after a value, or move down the AutoComplete list. Free text typed at a
/// prompt is left alone.
fn tab(app: &mut CadApp) {
    // Grip dimensional input: Tab locks the typed value and moves to the next field.
    if crate::grip_dyn::tab(app) {
        return;
    }
    // Over a snap marker with nothing typed, Tab cycles the candidate snaps.
    if app.cmd.buffer.is_empty() && app.canvas.snap.is_some() {
        app.canvas.snap_tab = true;
    } else if let Some(f) = dyn_frame(app)
        && let Some(sep) = crate::dyninput::tab_separator(&app.cmd.buffer, &f)
    {
        app.cmd.buffer.push(sep);
    } else if let Some(f) = dyn_frame(app)
        && crate::dyninput::can_lock_second(&app.cmd.buffer, &f)
    {
        // Both values typed: Tab locks the second one as well.
        app.cmd.locked_second = Some(app.cmd.buffer.clone());
    } else if suggesting(app) {
        // Tab moves down the AutoComplete list as Down does.
        step_suggestion(app, 1);
    }
}

/// Moves the chosen AutoComplete row by `delta`, wrapping from the last row to the first and back,
/// and shows its completion again.
fn step_suggestion(app: &mut CadApp, delta: isize) {
    let n = suggestions_for(app, &app.cmd.buffer).len();
    if n == 0 {
        return;
    }
    let cur = app.cmd.suggestion.min(n - 1);
    app.cmd.suggestion = if delta < 0 { cur.checked_sub(1).unwrap_or(n - 1) } else { (cur + 1) % n };
    app.cmd.completion_hidden = false;
}

/// The rest of the chosen AutoComplete row's name after what was typed (`CO` + `PY`), shown
/// selected on the command line; `None` when nothing is offered or Backspace erased it.
pub fn completion(app: &CadApp) -> Option<String> {
    if app.cmd.completion_hidden || !app.cmd.preedit.is_empty() || !suggesting(app) {
        return None;
    }
    let typed = app.cmd.buffer.as_str();
    let sug = suggestions_for(app, typed);
    let s = sug.get(app.cmd.suggestion).or(sug.first())?;
    let head = s.id.get(..typed.len())?;
    let rest = s.id.get(typed.len()..)?;
    (head.eq_ignore_ascii_case(typed) && !rest.is_empty()).then(|| rest.to_string())
}

/// Ctrl+V pastes text into the command line (not objects into the drawing): it was clicked and
/// the pointer is still over it.
pub fn takes_paste(app: &CadApp, ctx: &egui::Context) -> bool {
    app.cmd.clicked && app.cmd.area.zip(ctx.pointer_hover_pos()).is_some_and(|(a, p)| a.contains(p))
}

/// The longest text a paste adds to the command line.
const MAX_PASTE: usize = 256;

/// Pastes `text` into the command line as if typed: its first line, without control characters,
/// up to [`MAX_PASTE`] characters.
pub fn paste(app: &mut CadApp, text: &str) {
    let line = text.lines().next().unwrap_or_default();
    app.cmd.preedit.clear();
    app.cmd.buffer.extend(line.chars().filter(|c| !c.is_control()).take(MAX_PASTE));
    app.cmd.suggestion = 0;
    app.cmd.completion_hidden = false;
    app.cmd.history_pos = None;
}

/// An AutoComplete suggestion for the command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// Command identifier (uppercase, e.g. "RECTANG"), used for buffer completion and execution.
    pub id: String,
    /// Display text shown in the AutoComplete list (e.g. "REC (RECTANG)" or "RECOVER").
    pub display: String,
    /// Human-readable command label (e.g. "Rectangle").
    pub label: &'static str,
}

/// The AutoComplete rows of the commands for `prefix`: its exact command or alias, the command
/// names it starts, shortest first, and the aliases it starts.
struct CommandRows {
    exact: Option<Suggestion>,
    names: Vec<Suggestion>,
    aliases: Vec<Suggestion>,
}

fn command_rows(prefix: &str) -> CommandRows {
    let p = prefix.to_ascii_lowercase();
    let mut rows = CommandRows { exact: None, names: Vec::new(), aliases: Vec::new() };
    let mut seen_ids: HashSet<&'static str> = HashSet::new();

    // 1. Exact match: alias resolution or exact command name match.
    let resolved = cadcraft_engine::cmd::resolve_alias(&p);
    if let Some(spec) = cadcraft_engine::cmd::find_command(&resolved)
        && !spec.id.contains('.')
    {
        let display =
            if spec.id == p { spec.id.to_ascii_uppercase() } else { format!("{} ({})", p.to_ascii_uppercase(), spec.id.to_ascii_uppercase()) };
        rows.exact = Some(Suggestion { id: spec.id.to_ascii_uppercase(), display, label: spec.label });
        seen_ids.insert(spec.id);
    }

    // 2. Command ID prefix matches (excluding already matched exact command).
    let mut id_matches: Vec<&'static cadcraft_engine::CommandSpec> =
        cadcraft_engine::command_specs().iter().filter(|c| !c.id.contains('.') && !seen_ids.contains(c.id) && c.id.starts_with(&p)).collect();
    id_matches.sort_by_key(|c| (c.id.len(), c.id));
    for c in id_matches {
        rows.names.push(Suggestion { id: c.id.to_ascii_uppercase(), display: c.id.to_ascii_uppercase(), label: c.label });
        seen_ids.insert(c.id);
    }

    // 3. Command alias prefix matches (for commands not yet added).
    let mut alias_matches: Vec<(&'static str, &'static cadcraft_engine::CommandSpec)> = Vec::new();
    for c in cadcraft_engine::command_specs() {
        if c.id.contains('.') || seen_ids.contains(c.id) {
            continue;
        }
        let matching_alias = c.aliases.iter().copied().filter(|a| a.starts_with(&p)).min_by_key(|a| (a.len(), *a));
        if let Some(alias) = matching_alias {
            alias_matches.push((alias, c));
        }
    }
    alias_matches.sort_by_key(|(alias, c)| (alias.len(), *alias, c.id));
    for (alias, c) in alias_matches {
        if seen_ids.insert(c.id) {
            rows.aliases.push(Suggestion {
                id: c.id.to_ascii_uppercase(),
                display: format!("{} ({})", alias.to_ascii_uppercase(), c.id.to_ascii_uppercase()),
                label: c.label,
            });
        }
    }
    rows
}

/// Matching command names/aliases for AutoComplete, with `lisp`, the LISP commands the drawing
/// defines, and the system variables: everything that can be typed at the command line.
fn suggestions(prefix: &str, lisp: &[String]) -> Vec<Suggestion> {
    if prefix.is_empty() || prefix.contains(' ') {
        return Vec::new();
    }
    let rows = command_rows(prefix);
    merge_typed(prefix, rows.exact, rows.names, rows.aliases, lisp)
}

/// Joins the commands' rows with the LISP commands and system variables, ranked alike: an exact
/// name first, then the names it starts (LISP commands among the commands), shortest first, then
/// the aliases (`aliases`, in their order), then the system variables, shortest first; 8 at most.
fn merge_typed(prefix: &str, exact: Option<Suggestion>, names: Vec<Suggestion>, aliases: Vec<Suggestion>, lisp: &[String]) -> Vec<Suggestion> {
    const LISP: &str = "LISP command";
    const SYSVAR: &str = "System variable";
    let up = prefix.to_ascii_uppercase();
    let row = |name: &str, label: &'static str| Suggestion { id: name.to_string(), display: name.to_string(), label };
    let sysvars = cadcraft_engine::sysvars::names();
    let mut out: Vec<Suggestion> = Vec::new();
    match exact {
        Some(s) => out.push(s),
        None if lisp.contains(&up) => out.push(row(&up, LISP)),
        None if sysvars.binary_search(&up.as_str()).is_ok() => out.push(row(&up, SYSVAR)),
        None => {}
    }
    let taken = |out: &[Suggestion], id: &str| out.iter().any(|o| o.id.eq_ignore_ascii_case(id));
    let mut named: Vec<Suggestion> = names;
    named.extend(lisp.iter().filter(|n| n.starts_with(&up)).map(|n| row(n, LISP)));
    named.sort_by(|a, b| (a.id.len(), &a.id).cmp(&(b.id.len(), &b.id)));
    let mut vars: Vec<&str> = sysvars.iter().copied().filter(|n| n.starts_with(&up)).collect();
    vars.sort_by_key(|n| (n.len(), *n));
    for s in named.into_iter().chain(aliases).chain(vars.into_iter().map(|n| row(n, SYSVAR))) {
        if out.len() >= 8 {
            break;
        }
        if !taken(&out, &s.id) {
            out.push(s);
        }
    }
    out
}

impl CmdLine {
    /// Track the selection: a new selection starts with nothing typed since.
    pub fn watch_selection(&mut self, sel: &[cadcraft_doc::Handle]) {
        if self.seen_selection != sel {
            self.seen_selection = sel.to_vec();
            self.typed_since_selection = false;
        }
    }

    /// Note that the command line holds typed text.
    pub fn note_typing(&mut self) {
        if !self.buffer.is_empty() {
            self.typed_since_selection = true;
        }
    }

    /// Tab locked the second Dynamic Input value and nothing was typed since.
    pub fn second_locked(&self) -> bool {
        self.locked_second.as_deref() == Some(self.buffer.as_str())
    }

    /// The options menu row under `at` (screen), if the menu is open.
    pub fn option_at(&self, at: Pos2) -> Option<String> {
        self.options.as_ref()?;
        self.option_rows.iter().find(|(r, _)| r.contains(at)).map(|(_, k)| k.clone())
    }
}

/// Keep the options menu only while its prompt is on screen with Dynamic Input and DYNPROMPT on.
fn options_still_fit(app: &mut CadApp) {
    let st = &app.session.settings;
    let on = st.dynmode && st.dynprompt && app.canvas.hot_grips.is_empty();
    let kws = app.session.current_prompt().map(|p| p.keywords).unwrap_or_default();
    if app.cmd.options.as_ref().is_some_and(|m| !on || !m.fits(&kws)) {
        app.cmd.options = None;
    }
}

/// Down Arrow at a prompt with keywords (Dynamic Input on, DYNPROMPT on, not browsing the
/// history): open the options menu at the cursor, or move its highlight. `false` when the key is
/// left for the history.
fn options_key(app: &mut CadApp, step: isize) -> bool {
    if let Some(m) = app.cmd.options.as_mut() {
        m.step(step);
        return true;
    }
    let st = &app.session.settings;
    if step < 0 || !st.dynmode || !st.dynprompt || app.cmd.history_pos.is_some() || !app.canvas.hot_grips.is_empty() {
        return false;
    }
    let kws = app.session.current_prompt().map(|p| p.keywords).unwrap_or_default();
    app.cmd.options = crate::dyninput::OptionsMenu::open(&kws);
    app.cmd.options.is_some()
}

/// Send a keyword picked from the options menu, as a click on it at the command line does.
pub fn pick_option(app: &mut CadApp, keyword: String) {
    app.cmd.options = None;
    app.cmd.buffer.clear();
    if let Err(e) = app.session.input(Input::Keyword(keyword)) {
        app.session.echo(e.to_string());
    }
}

/// Whether Backspace should erase the selection instead of editing the command line. The key
/// labelled "delete" on a Mac sends Backspace, so on macOS it erases selected objects, but only
/// when it can't be meant for the command line: a fresh, plain press (never an auto-repeat, so
/// holding it to clear typing can't run on into the drawing, and not Option/Cmd+delete, which
/// delete a word or line of text on macOS), no command running, the command line empty, and
/// nothing typed there since the selection was made.
pub fn backspace_erases(mac: bool, press: KeyPress, buffer: &str, command_running: bool, has_selection: bool, typed_since_selection: bool) -> bool {
    mac && press.plain && !press.repeat && buffer.is_empty() && !command_running && has_selection && !typed_since_selection
}

/// How a key was pressed.
#[derive(Clone, Copy, Debug)]
pub struct KeyPress {
    /// The operating system's auto-repeat while the key is held.
    pub repeat: bool,
    /// No Option, Control or Command held (Shift doesn't matter).
    pub plain: bool,
}

/// Let the operating system's IME compose into the command line while a prompt asks for text.
/// Nothing owns keyboard focus there (typing goes to the command line from anywhere), so egui
/// never enables the IME by itself: publishing an IME output does it, with the command line's
/// rectangle and caret for the composition window. Without this a Windows IME (Pinyin, Japanese,
/// Korean...) cannot even switch on (#1761).
fn enable_ime(app: &mut CadApp, ctx: &egui::Context) {
    if !app.cmd.ime_on {
        app.cmd.ime_on = true;
        app.cmd.ime_generation = app.cmd.ime_generation.wrapping_add(1);
    }
    let (rect, cursor_rect) = app.cmd.ime_rect.unwrap_or_else(|| {
        let r = ctx.content_rect();
        (r, Rect::from_min_size(pos2(r.left() + 30.0, r.bottom() - 34.0), vec2(1.0, 18.0)))
    });
    ctx.output_mut(|o| {
        o.ime = Some(egui::output::IMEOutput::new(
            egui::Id::unique("cc_cmdline_ime"),
            app.cmd.ime_generation,
            egui::IMEPurpose::Normal,
            rect,
            cursor_rect,
        ));
    });
}

/// Handle keyboard input destined for the command line (when no text field has focus).
pub fn keyboard(app: &mut CadApp, ctx: &egui::Context) {
    if !app.ui.show_command_line {
        app.cmd.area = None;
    }
    if ctx.egui_wants_keyboard_input() {
        app.cmd.preedit.clear();
        app.cmd.ime_on = false;
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    let selection = app.session.selection();
    app.cmd.watch_selection(&selection);
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    let text_prompt = app.session.current_prompt().is_some_and(|p| p.accept.text && !p.accept.point && !p.accept.number);
    if text_prompt {
        enable_ime(app, ctx);
    } else {
        // No prompt is collecting text: a composition left over from one cannot belong here.
        app.cmd.preedit.clear();
        app.cmd.ime_on = false;
    }
    options_still_fit(app);
    for ev in events {
        // The options menu takes the arrows, Enter and Escape; typing closes it.
        if app.cmd.options.is_some() {
            let space = matches!(&ev, egui::Event::Text(t) if t == " ") && !text_prompt;
            match &ev {
                _ if space => {
                    if let Some(k) = app.cmd.options.as_ref().and_then(|m| m.current()).map(str::to_owned) {
                        pick_option(app, k);
                    }
                    continue;
                }
                egui::Event::Key { key: Key::Enter, pressed: true, .. } => {
                    if let Some(k) = app.cmd.options.as_ref().and_then(|m| m.current()).map(str::to_owned) {
                        pick_option(app, k);
                    }
                    continue;
                }
                egui::Event::Key { key: Key::Escape, pressed: true, .. } => {
                    app.cmd.options = None;
                    continue;
                }
                egui::Event::Key { key: Key::ArrowUp, pressed: true, modifiers, .. } if !modifiers.any() => {
                    options_key(app, -1);
                    continue;
                }
                egui::Event::Key { key: Key::ArrowDown | Key::ArrowUp, pressed: true, .. } => {}
                // Space arrives as a key and as text; the text picks.
                egui::Event::Key { key: Key::Space, .. } => {}
                egui::Event::Text(_) | egui::Event::Key { pressed: true, .. } => app.cmd.options = None,
                _ => {}
            }
        }
        match ev {
            egui::Event::Text(t) => {
                if t == "\t" {
                    tab(app);
                } else if t == " " && !text_prompt {
                    submit(app);
                } else {
                    app.cmd.preedit.clear();
                    app.cmd.buffer.push_str(&t);
                    app.cmd.suggestion = 0;
                    app.cmd.completion_hidden = false;
                    // Editing a recalled command leaves the history: AutoComplete applies again.
                    app.cmd.history_pos = None;
                }
            }
            // IME composition: the pre-edit is shown but stays out of the buffer; the commit is
            // the only text that is inserted. Enter/Return commits arrive separately as keys, so
            // a newline commit is ignored (as egui's own text fields do) instead of submitting.
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => app.cmd.preedit = text,
            egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                app.cmd.preedit.clear();
                if text != "\n" && text != "\r" {
                    app.cmd.buffer.push_str(&text);
                    // As typed letters: the AutoComplete list starts over for the new text.
                    app.cmd.suggestion = 0;
                    app.cmd.completion_hidden = false;
                    app.cmd.history_pos = None;
                }
            }
            egui::Event::Ime(_) => {}
            egui::Event::Key { key, pressed: true, repeat, modifiers, .. } => match key {
                Key::Enter => submit(app),
                Key::Escape => {
                    app.cmd.buffer.clear();
                    app.cmd.completion_hidden = false;
                    app.cmd.preedit.clear();
                    app.session.cancel();
                }
                Key::Backspace => {
                    let press = KeyPress { repeat, plain: !(modifiers.alt || modifiers.ctrl || modifiers.command) };
                    let erase = backspace_erases(
                        mac,
                        press,
                        &app.cmd.buffer,
                        app.session.prompting(),
                        !app.session.selection().is_empty(),
                        app.cmd.typed_since_selection,
                    );
                    if erase {
                        let _ = app.run("erase.selection", serde_json::json!({}));
                    } else if completion(app).is_some() {
                        // The completion goes first; the chosen row stays chosen.
                        app.cmd.completion_hidden = true;
                    } else {
                        app.cmd.buffer.pop();
                        app.cmd.suggestion = 0;
                        app.cmd.history_pos = None;
                        app.cmd.completion_hidden = true;
                    }
                }
                Key::ArrowUp if !modifiers.any() && suggesting(app) => step_suggestion(app, -1),
                Key::ArrowDown if !modifiers.any() && suggesting(app) => step_suggestion(app, 1),
                Key::ArrowUp if !modifiers.any() => {
                    if !app.cmd.history.is_empty() {
                        let n = app.cmd.history.len();
                        let pos = app.cmd.history_pos.map(|p| p.saturating_sub(1)).unwrap_or(n - 1);
                        app.cmd.history_pos = Some(pos);
                        app.cmd.buffer = app.cmd.history.get(pos).cloned().unwrap_or_default();
                    }
                }
                Key::ArrowDown if !modifiers.any() => {
                    if !options_key(app, 1)
                        && let Some(p) = app.cmd.history_pos
                    {
                        let np = p + 1;
                        if np >= app.cmd.history.len() {
                            app.cmd.history_pos = None;
                            app.cmd.buffer.clear();
                        } else {
                            app.cmd.history_pos = Some(np);
                            app.cmd.buffer = app.cmd.history.get(np).cloned().unwrap_or_default();
                        }
                    }
                }
                Key::Tab => tab(app),
                Key::Delete if app.cmd.buffer.is_empty() && !app.session.prompting() => {
                    let _ = app.run("erase.selection", serde_json::json!({}));
                }
                _ => {}
            },
            _ => {}
        }
        app.cmd.note_typing();
    }
}

pub fn submit(app: &mut CadApp) {
    app.cmd.preedit.clear();
    // Grip dimensional input: the typed lengths and angles place the grip.
    if crate::grip_dyn::submit(app) {
        return;
    }
    // Dynamic Input: a split entry (`40,30`, `40<90`, `40,`) is completed from the cursor and
    // measured from the last point.
    if let Some(f) = dyn_frame(app) {
        let cursor = app.canvas.cursor.unwrap_or(app.session.cursor);
        let last = app.session.last_point;
        let done = crate::dyninput::parse(&app.cmd.buffer, &f).and_then(|e| crate::dyninput::complete(&e, &f, cursor, last));
        if let Some(text) = done {
            app.cmd.buffer = text;
        }
    }
    complete_command(app);
    let text = std::mem::take(&mut app.cmd.buffer);
    app.cmd.suggestion = 0;
    app.cmd.completion_hidden = false;
    app.cmd.locked_second = None;
    app.cmd.history_pos = None;
    if !text.trim().is_empty() && !app.session.prompting() {
        app.cmd.history.push(text.clone());
        if app.cmd.history.len() > 200 {
            app.cmd.history.remove(0);
        }
    }
    crate::context_menu::note_input(app, &text);
    if text.is_empty() {
        if let Err(e) = app.session.input(Input::Enter) {
            app.session.echo(e.to_string());
        }
        return;
    }
    app.cmdline(&text);
}

/// Enter (or Space) on a partly typed command name runs the AutoComplete row shown as chosen (the
/// first unless Up/Down moved it), as CAD command lines usually do: `HA` + Enter is HATCH. A
/// name that already is a command, an alias, a LISP command or a system variable runs as typed,
/// unless Up/Down chose another row.
fn complete_command(app: &mut CadApp) {
    if app.session.prompting() || app.cmd.history_pos.is_some() {
        return;
    }
    let typed = app.cmd.buffer.trim();
    // A row chosen with Up/Down wins over an exact name or alias (`pol`, Down: the next row).
    let chosen = app.cmd.suggestion > 0;
    if typed.is_empty() || (!chosen && app.session.knows_command(typed)) {
        return;
    }
    let sug = suggestions_for(app, typed);
    if let Some(s) = sug.get(app.cmd.suggestion).or(sug.first()) {
        app.cmd.buffer = s.id.clone();
    }
}

/// The AutoComplete list is showing (a command name is being typed).
fn suggesting(app: &CadApp) -> bool {
    !app.session.prompting() && app.cmd.history_pos.is_none() && !suggestions_for(app, &app.cmd.buffer).is_empty()
}

/// The AutoComplete list for what is typed, with the drawing's LISP commands.
fn suggestions_for(app: &CadApp, typed: &str) -> Vec<Suggestion> {
    suggestions(typed, &app.session.lisp_commands())
}

fn visible_lines(app: &CadApp, canvas: Rect, bar: Rect) -> usize {
    let available = ((bar.top() - canvas.top() - 8.0).max(0.0) / LINE_H) as usize;
    app.ui.history_lines.min(MAX_LINES).min(available)
}

/// Draw the floating command bar and its history over the bottom-centre of `canvas`; the bar is 48 % of the canvas width, clamped to 360–760 points and never wider than the canvas minus 8 points.
pub fn show(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect) {
    let t = Tokens::get();
    let w = (canvas.width() * 0.48).clamp(360.0, 760.0).min((canvas.width() - 8.0).max(0.0));
    let h = 24.0;
    let bar = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, canvas.bottom() - h - 10.0), vec2(w, h));
    let p = ui.painter_at(canvas);
    history(app, ui, canvas, bar);
    // History lines above the bar (the expanded window replaces them).
    let n = if app.cmd.expanded { 0 } else { visible_lines(app, canvas, bar) };
    let lines: Vec<String> = app.session.log.iter().rev().take(n).rev().cloned().collect();
    if !lines.is_empty() {
        let lh = 16.0;
        let hr = Rect::from_min_size(pos2(bar.left(), bar.top() - lh * lines.len() as f32 - 4.0), vec2(w, lh * lines.len() as f32 + 2.0));
        p.rect_filled(hr, 3.0, t.cmd_history);
        for (i, l) in lines.iter().enumerate() {
            p.text(pos2(hr.left() + 8.0, hr.top() + 1.0 + lh * i as f32), egui::Align2::LEFT_TOP, l, crate::theme::small(), t.text_dim);
        }
    }
    p.rect_filled(bar, 3.0, t.cmd_bg);
    p.rect_stroke(bar, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
    // Prompt glyph.
    p.text(pos2(bar.left() + 8.0, bar.center().y), egui::Align2::LEFT_CENTER, ">_", crate::theme::mono(), t.text_dim);
    let mut x = bar.left() + 30.0;
    let prompt = app.session.current_prompt();
    let font = crate::theme::body();
    match &prompt {
        Some(pr) => {
            // The grip modes name themselves ("** STRETCH **…").
            let name = app.session.running.as_ref().filter(|r| r.id != "grip").map(|r| r.id.to_ascii_uppercase() + " ").unwrap_or_default();
            let g = p.layout_no_wrap(name, font.clone(), t.text_faint);
            let gw = g.size().x;
            p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text_faint);
            x += gw;
            // A LISP `get…` prompt shows as the routine wrote it (no keywords, default or colon added).
            let message = if pr.raw { pr.display() } else { crate::i18n::t(&pr.message).to_string() };
            let g = p.layout_no_wrap(message, font.clone(), t.text);
            let gw = g.size().x;
            p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
            x += gw;
            if !pr.raw && !pr.keywords.is_empty() {
                let g = p.layout_no_wrap(if pr.message.is_empty() { " [".into() } else { crate::tl!(" or [").into() }, font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
                let kws = pr.keywords.clone();
                for (i, k) in kws.iter().enumerate() {
                    let g = p.layout_no_wrap(k.clone(), font.clone(), t.cmd_keyword);
                    let r = Rect::from_min_size(pos2(x, bar.top() + 3.0), vec2(g.size().x, h - 6.0));
                    let resp = ui.interact(r, ui.scope_id().with(("kw", i)), Sense::click());
                    if resp.hovered() {
                        p.rect_filled(r, 2.0, t.cmd_keyword_hover);
                    }
                    let gw = g.size().x;
                    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.cmd_keyword);
                    x += gw;
                    if resp.clicked() {
                        let _ = app.session.input(Input::Keyword(k.clone()));
                    }
                    if i + 1 < kws.len() {
                        let g = p.layout_no_wrap("/".into(), font.clone(), t.text);
                        let gw = g.size().x;
                        p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                        x += gw;
                    }
                }
                let g = p.layout_no_wrap("]".into(), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
            }
            if let Some(d) = pr.default.as_ref().filter(|_| !pr.raw) {
                let g = p.layout_no_wrap(format!(" <{d}>"), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
            }
            if !pr.raw {
                let g = p.layout_no_wrap(": ".into(), font.clone(), t.text);
                let gw = g.size().x;
                p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
                x += gw;
            } else if !pr.message.ends_with(' ') {
                x += 4.0;
            }
        }
        None => {
            if app.cmd.buffer.is_empty() {
                p.text(
                    pos2(x, bar.center().y),
                    egui::Align2::LEFT_CENTER,
                    crate::tl!("Type a command"),
                    egui::FontId::proportional(12.5),
                    t.text_faint,
                );
            }
        }
    }
    let g = p.layout_no_wrap(app.cmd.buffer.clone(), font.clone(), t.text);
    let gw = g.size().x;
    p.galley(pos2(x, bar.center().y - g.size().y / 2.0), g, t.text);
    let mut caret_x = x + gw;
    // The chosen AutoComplete row's completion follows the typed name, selected.
    if let Some(rest) = completion(app) {
        let g = p.layout_no_wrap(rest, font.clone(), t.text);
        let r = Rect::from_min_size(pos2(caret_x, bar.top() + 4.0), vec2(g.size().x, h - 8.0));
        p.rect_filled(r, 0.0, t.select_bg);
        p.galley(pos2(caret_x, bar.center().y - g.size().y / 2.0), g, t.text);
    }
    // The IME's composition text sits after the buffer (underlined) until it is committed.
    if !app.cmd.preedit.is_empty() {
        let pg = p.layout_no_wrap(app.cmd.preedit.clone(), font, t.text);
        let pw = pg.size().x;
        p.galley(pos2(caret_x, bar.center().y - pg.size().y / 2.0), pg, t.text);
        p.line_segment([pos2(caret_x, bar.bottom() - 7.0), pos2(caret_x + pw, bar.bottom() - 7.0)], Stroke::new(1.0, t.accent));
        caret_x += pw;
    }
    // Caret.
    let blink = (ui.input(|i| i.time) * 2.0).floor() as i64 % 2 == 0;
    if blink {
        p.line_segment([pos2(caret_x + 1.0, bar.top() + 5.0), pos2(caret_x + 1.0, bar.bottom() - 5.0)], Stroke::new(1.0, t.text));
    }
    // Where the IME's composition window follows: the whole command line and the caret in it.
    let caret = Rect::from_min_size(pos2(caret_x + 1.0, bar.top() + 5.0), vec2(1.0, h - 10.0));
    app.cmd.ime_rect = Some((bar, caret));
    // History chevron.
    crate::icons::paint(
        &p,
        Rect::from_center_size(pos2(bar.right() - 14.0, bar.center().y), vec2(14.0, 14.0)),
        if app.cmd.expanded { crate::icons::Icon::ChevronDown } else { crate::icons::Icon::ChevronUp },
        false,
    );
    // AutoComplete list.
    if !app.session.prompting() {
        let sug = suggestions_for(app, &app.cmd.buffer);
        if !sug.is_empty() {
            let rh = 20.0;
            let lr = Rect::from_min_size(
                pos2(
                    bar.left() + 24.0,
                    bar.top() - 6.0 - rh * sug.len() as f32 - (if lines.is_empty() { 0.0 } else { 16.0 * lines.len() as f32 + 6.0 }),
                ),
                vec2((bar.width() - 24.0).max(0.0), rh * sug.len() as f32),
            );
            p.rect_filled(lr, 3.0, t.list_bg);
            p.rect_stroke(lr, 3.0, Stroke::new(1.0, t.cmd_border), egui::StrokeKind::Inside);
            for (i, s) in sug.iter().enumerate() {
                let r = Rect::from_min_size(pos2(lr.left(), lr.top() + rh * i as f32), vec2(lr.width(), rh));
                let resp = ui.interact(r, ui.scope_id().with(("sug", i)), Sense::click());
                if resp.hovered() || i == app.cmd.suggestion.min(sug.len() - 1) {
                    p.rect_filled(r.shrink(1.0), 2.0, if resp.hovered() { t.cmd_keyword_hover } else { t.list_row });
                }
                p.text(Pos2::new(r.left() + 8.0, r.center().y), egui::Align2::LEFT_CENTER, &s.display, crate::theme::body(), t.text);
                p.text(
                    Pos2::new(r.right() - 8.0, r.center().y),
                    egui::Align2::RIGHT_CENTER,
                    crate::i18n::t(s.label),
                    crate::theme::small(),
                    t.text_faint,
                );
                if resp.clicked() {
                    app.cmd.buffer.clear();
                    app.start(&s.id.to_ascii_lowercase());
                }
            }
        }
    }
}

/// The command line's areas, laid out like [`show`] draws them.
struct Zones {
    /// The inline history lines (when there are any and the window is closed).
    lines: Option<Rect>,
    /// The expanded history window.
    window: Option<Rect>,
    chevron: Rect,
    /// The top edge that resizes the inline history.
    grip: Rect,
    /// Everything, AutoComplete list included: kept away from the drawing underneath.
    all: Rect,
}

fn zones(app: &CadApp, canvas: Rect, bar: Rect) -> Zones {
    let n = if app.cmd.expanded { 0 } else { visible_lines(app, canvas, bar).min(app.session.log.len()) };
    let lines =
        (n > 0).then(|| Rect::from_min_size(pos2(bar.left(), bar.top() - LINE_H * n as f32 - 4.0), vec2(bar.width(), LINE_H * n as f32 + 2.0)));
    let window = app.cmd.expanded.then(|| {
        // As tall as the log (at least three lines), at most half the drawing area.
        let full = LINE_H * app.session.log.len().max(3) as f32 + 4.0;
        let h = full.min((canvas.height() * 0.5).max(3.0 * LINE_H));
        Rect::from_min_size(pos2(bar.left(), bar.top() - h - 4.0), vec2(bar.width(), h + 2.0))
    });
    let top = window.or(lines).map_or(bar.top(), |r| r.top());
    let grip = Rect::from_min_max(pos2(bar.left(), top - 3.0), pos2(bar.right(), top + 3.0));
    let mut all = bar.union(grip);
    if !app.session.prompting() {
        let k = suggestions_for(app, &app.cmd.buffer).len();
        if k > 0 {
            let above = if n == 0 { 0.0 } else { LINE_H * n as f32 + 6.0 };
            all = all.union(Rect::from_min_size(pos2(bar.left(), bar.top() - 6.0 - 20.0 * k as f32 - above), vec2(bar.width(), 1.0)));
        }
    }
    Zones { lines, window, chevron: Rect::from_center_size(pos2(bar.right() - 14.0, bar.center().y), vec2(24.0, 22.0)), grip, all }
}

/// The pointer on the command line: one widget under all of it keeps clicks, drags and the wheel
/// off the drawing (the keywords and AutoComplete rows sit on top of it). Clicking the history
/// lines or the chevron opens/closes the history window (`ui.toggle.history`, also F2); dragging
/// the top edge sets how many lines show (`ui.cmdline.lines`).
fn history(app: &mut CadApp, ui: &mut egui::Ui, canvas: Rect, bar: Rect) {
    let z = zones(app, canvas, bar);
    let resp = ui.interact(z.all, ui.scope_id().with("cmdline"), Sense::click_and_drag());
    app.cmd.area = Some(z.all);
    if let Some(o) = ui.input(|i| i.pointer.primary_pressed().then(|| i.pointer.press_origin()).flatten()) {
        app.cmd.clicked = z.all.contains(o);
    }
    let at = resp.hover_pos().or(resp.interact_pointer_pos());
    let on_grip = |p: Pos2| z.grip.contains(p) && z.window.is_none();
    let on_toggle = |p: Pos2| z.chevron.contains(p) || z.lines.is_some_and(|r| r.contains(p));
    if app.cmd.resize_from.is_some() || at.is_some_and(on_grip) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    } else if at.is_some_and(on_toggle) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.drag_started()
        && let Some(o) = ui.input(|i| i.pointer.press_origin())
        && on_grip(o)
    {
        app.cmd.resize_from = Some((o.y, visible_lines(app, canvas, bar)));
    }
    if let Some((y0, n0)) = app.cmd.resize_from {
        if let (true, Some(p)) = (resp.dragged(), resp.interact_pointer_pos()) {
            let n = (n0 as f32 + (y0 - p.y) / LINE_H).round().clamp(0.0, MAX_LINES as f32) as usize;
            if n != app.ui.history_lines {
                let _ = app.run("ui.cmdline.lines", serde_json::json!({ "lines": n }));
            }
        } else {
            app.cmd.resize_from = None;
        }
    }
    if resp.clicked() && resp.interact_pointer_pos().is_some_and(on_toggle) {
        let _ = app.run("ui.toggle.history", serde_json::json!({}));
    }
    crate::shortcut_menus::attach(app, &resp, crate::shortcut_menus::command_line_entries);
    if let Some(r) = zones(app, canvas, bar).window {
        history_window(app, ui, r);
    }
}

/// The expanded history: the whole command log, scrollable, newest at the bottom.
fn history_window(app: &CadApp, ui: &mut egui::Ui, r: Rect) {
    let t = Tokens::get();
    ui.painter().rect_filled(r, 3.0, t.cmd_bg);
    ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let inner = r.shrink2(vec2(8.0, 3.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    child.spacing_mut().item_spacing.y = 0.0;
    let log = &app.session.log;
    egui::ScrollArea::vertical().id_salt("cmd_history").stick_to_bottom(true).auto_shrink([false, false]).show_rows(
        &mut child,
        LINE_H,
        log.len(),
        |ui, rows| {
            for l in log.get(rows).into_iter().flatten() {
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), LINE_H), Sense::hover());
                let p = ui.painter().with_clip_rect(row.intersect(ui.clip_rect()));
                p.text(pos2(row.left(), row.center().y), egui::Align2::LEFT_CENTER, l, crate::theme::small(), t.text_dim);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::Session;
    use egui::{Event, PointerButton};

    #[test]
    fn compact_canvas_contains_history_controls_without_changing_line_preference() {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.ui.history_lines = 12;
        for i in 0..20 {
            app.session.echo(format!("line {i}"));
        }
        let canvas = Rect::from_min_size(Pos2::ZERO, vec2(300.0, 180.0));
        let w = (canvas.width() * 0.48).clamp(360.0, 760.0).min((canvas.width() - 8.0).max(0.0));
        let bar = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, canvas.bottom() - 34.0), vec2(w, 24.0));
        let z = zones(&app, canvas, bar);
        assert!(canvas.contains_rect(bar));
        assert!(canvas.contains_rect(z.chevron));
        assert!(z.lines.is_some_and(|r| canvas.contains_rect(r)));
        assert!(canvas.contains_rect(z.grip));
        assert_eq!(app.ui.history_lines, 12);
    }

    fn frame(app: &mut CadApp, ctx: &egui::Context, events: Vec<Event>) -> egui::FullOutput {
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        out.textures_delta.clear();
        out
    }

    fn press(app: &mut CadApp, ctx: &egui::Context, at: Pos2, pressed: bool) {
        frame(app, ctx, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() }]);
    }

    #[test]
    fn tab_leaves_free_text_at_a_prompt_alone() {
        use cadcraft_engine::geom::Vec2;
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        app.run("text", serde_json::json!({"at": [0, 0], "text": "HELLO", "height": 2.5})).unwrap();
        app.start("textedit");
        app.session.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
        app.cmd.buffer = "li".into();
        tab(&mut app);
        assert_eq!(app.cmd.buffer, "li");
    }

    #[test]
    fn ctrl_v_pastes_into_the_command_line_only_after_a_click_on_it_with_the_pointer_still_there() {
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        let area = app.cmd.area.expect("the command line is shown");
        let on = area.center_bottom() - vec2(0.0, 8.0);
        let off = pos2(area.center().x, area.top() - 200.0);
        let click = |app: &mut CadApp, at: Pos2| {
            frame(app, &ctx, vec![Event::PointerMoved(at)]);
            press(app, &ctx, at, true);
            press(app, &ctx, at, false);
        };
        let objects = |app: &CadApp| app.session.doc().map(|d| d.model.iter().count()).unwrap_or(0);
        let before = objects(&app);
        // Clicked on the command line and still over it: the text is typed there, one line.
        click(&mut app, on);
        frame(&mut app, &ctx, vec![Event::Paste("cer\nline 2".into())]);
        assert_eq!(app.cmd.buffer, "cer");
        assert_eq!(objects(&app), before);
        assert_eq!(app.cmd.suggestion, 0);
        // Clicked on the command line, then the pointer left it: the paste goes to the drawing.
        app.cmd.buffer.clear();
        frame(&mut app, &ctx, vec![Event::PointerMoved(off)]);
        frame(&mut app, &ctx, vec![Event::Paste("cer".into())]);
        assert!(app.cmd.buffer.is_empty());
        // A click in the drawing, then the pointer back over the command line: the drawing again.
        click(&mut app, off);
        frame(&mut app, &ctx, vec![Event::PointerMoved(on)]);
        frame(&mut app, &ctx, vec![Event::Paste("cer".into())]);
        assert!(app.cmd.buffer.is_empty());
    }

    #[test]
    fn ime_preedit_is_shown_and_its_commit_is_inserted_once() {
        use cadcraft_engine::geom::Vec2;
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        app.run("text", serde_json::json!({"at": [0, 0], "text": "HELLO", "height": 2.5})).unwrap();
        app.start("textedit");
        app.session.input(Input::Point(Vec2::new(1.0, 1.0))).unwrap();
        assert_eq!(app.session.current_prompt().map(|p| p.message.clone()).as_deref(), Some("Enter new text"));
        let ctx = egui::Context::default();
        let out = frame(&mut app, &ctx, Vec::new());
        // The IME is enabled for the command line while it is collecting text.
        assert!(out.platform_output.ime.is_some());
        // Composition is shown at the command line but is not part of the text being edited.
        frame(&mut app, &ctx, vec![Event::Ime(egui::ImeEvent::Preedit { text: "zhong".into(), active_range_chars: None })]);
        assert_eq!(app.cmd.preedit, "zhong");
        assert!(app.cmd.buffer.is_empty());
        // The committed text replaces the pre-edit, exactly once.
        frame(&mut app, &ctx, vec![Event::Ime(egui::ImeEvent::Commit("中文".into()))]);
        assert!(app.cmd.preedit.is_empty());
        assert_eq!(app.cmd.buffer, "中文");
        let enter = |pressed| Event::Key { key: Key::Enter, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        frame(&mut app, &ctx, vec![enter(true), enter(false)]);
        let text = app.session.doc().unwrap().model.iter().next().map(|e| match &e.kind {
            cadcraft_engine::doc::EntityKind::Text(t) => t.value.clone(),
            _ => String::new(),
        });
        assert_eq!(text.as_deref(), Some("中文"));
        // Back to selecting objects: the next frame lets the IME go, so it can't swallow ASCII typing.
        let out = frame(&mut app, &ctx, Vec::new());
        assert!(out.platform_output.ime.is_none());
    }

    #[test]
    fn history_expands_by_click_chevron_f2_and_resizes_by_drag_without_touching_the_drawing() {
        let mut app = CadApp::new(Session::empty(), crate::Services::default());
        app.start("ui.sample");
        for i in 0..20 {
            app.session.echo(format!("line {i}"));
        }
        let ctx = egui::Context::default();
        frame(&mut app, &ctx, Vec::new());
        frame(&mut app, &ctx, Vec::new());
        let canvas = app.canvas.rect.unwrap_or(Rect::NOTHING);
        let w = (canvas.width() * 0.48).clamp(360.0, 760.0).min((canvas.width() - 8.0).max(0.0));
        let bar = Rect::from_min_size(pos2(canvas.center().x - w / 2.0, canvas.bottom() - 34.0), vec2(w, 24.0));
        let z = zones(&app, canvas, bar);
        let Some(lines) = z.lines else { panic!("three history lines show by default") };
        // A click on the history lines opens the history window, and doesn't start a selection.
        for (at, pressed) in [(lines.center(), true), (lines.center(), false)] {
            frame(&mut app, &ctx, vec![Event::PointerMoved(at)]);
            press(&mut app, &ctx, at, pressed);
        }
        frame(&mut app, &ctx, Vec::new());
        assert!(app.cmd.expanded);
        assert!(app.session.pending_window.is_none(), "the click stayed on the command line");
        // The chevron closes it; F2 and TEXTSCR open it.
        let chevron = z.chevron.center();
        press(&mut app, &ctx, chevron, true);
        press(&mut app, &ctx, chevron, false);
        frame(&mut app, &ctx, Vec::new());
        assert!(!app.cmd.expanded);
        let f2 = |pressed| Event::Key { key: Key::F2, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        frame(&mut app, &ctx, vec![f2(true), f2(false)]);
        assert!(app.cmd.expanded);
        assert_eq!(app.run("ui.toggle.history", serde_json::json!({"on": false})), Ok(serde_json::json!({"on": false})));
        app.cmdline("textscr");
        assert!(app.cmd.expanded);
        let _ = app.run("ui.toggle.history", serde_json::json!({"on": false}));
        frame(&mut app, &ctx, Vec::new());
        // Dragging the top edge up by two lines shows two more lines (capped).
        let top = pos2(lines.center().x, lines.top() - 1.0);
        frame(&mut app, &ctx, vec![Event::PointerMoved(top)]);
        press(&mut app, &ctx, top, true);
        for dy in [8.0, 20.0, 2.0 * LINE_H] {
            frame(&mut app, &ctx, vec![Event::PointerMoved(top - vec2(0.0, dy))]);
        }
        press(&mut app, &ctx, top - vec2(0.0, 2.0 * LINE_H), false);
        frame(&mut app, &ctx, Vec::new());
        assert_eq!(app.ui.history_lines, 5);
        assert!(!app.cmd.expanded, "a drag isn't a click");
        assert!(app.session.pending_window.is_none());
        assert_eq!(app.run("ui.cmdline.lines", serde_json::json!({"lines": 99})), Ok(serde_json::json!({"lines": MAX_LINES})));
    }
}

#[cfg(test)]
mod delete_key_tests {
    use super::*;
    use cadcraft_doc::Handle;

    const PRESS: KeyPress = KeyPress { repeat: false, plain: true };

    #[test]
    fn mac_delete_erases_a_fresh_selection() {
        assert!(backspace_erases(true, PRESS, "", false, true, false));
        // Not on Windows/Linux, where Backspace never deletes objects.
        assert!(!backspace_erases(false, PRESS, "", false, true, false));
        // Nothing selected, a command running, or text on the command line: just edit text.
        assert!(!backspace_erases(true, PRESS, "", false, false, false));
        assert!(!backspace_erases(true, PRESS, "", true, true, false));
        assert!(!backspace_erases(true, PRESS, "m", false, true, false));
        // Option/Cmd+delete are text-editing gestures on macOS.
        assert!(!backspace_erases(true, KeyPress { plain: false, ..PRESS }, "", false, true, false));
    }

    #[test]
    fn holding_delete_to_clear_typing_never_erases() {
        // Auto-repeat after the last character is gone.
        assert!(!backspace_erases(true, KeyPress { repeat: true, ..PRESS }, "", false, true, false));
        // Typed, then cleared with separate presses: still not erased.
        assert!(!backspace_erases(true, PRESS, "", false, true, true));
    }

    #[test]
    fn typing_is_forgotten_when_the_selection_changes() {
        let mut c = CmdLine::default();
        c.watch_selection(&[Handle(1)]);
        c.buffer.push('m');
        c.note_typing();
        c.buffer.clear();
        c.watch_selection(&[Handle(1)]);
        assert!(c.typed_since_selection);
        c.watch_selection(&[Handle(1), Handle(2)]);
        assert!(!c.typed_since_selection);
    }
}

#[cfg(test)]
mod autocomplete_tests {
    use super::*;

    fn suggestions_en(prefix: &str) -> Vec<Suggestion> {
        suggestions(prefix, &[])
    }

    #[test]
    fn lisp_commands_and_system_variables_are_offered_like_commands() {
        let lisp = vec!["DIMC".to_string(), "ZAP".to_string()];
        // An exact LISP name comes first: Enter runs it.
        let dimc = suggestions("dimc", &lisp);
        assert_eq!((dimc[0].id.as_str(), dimc[0].label), ("DIMC", "LISP command"));
        assert!(dimc.iter().skip(1).all(|s| s.id != "DIMC"));
        // One more letter: the other names.
        assert!(suggestions("dimco", &lisp).iter().all(|s| s.id != "DIMC"));
        // A LISP name ranks among the command names, shortest first.
        assert!(suggestions("z", &lisp).iter().any(|s| s.id == "ZAP" && s.label == "LISP command"));
        // System variables follow the commands; an exact one comes first.
        let osm = suggestions_en("osm");
        assert!(osm.iter().any(|s| s.id == "OSMODE" && s.label == "System variable"), "{osm:?}");
        assert_eq!(suggestions_en("osmode")[0].id, "OSMODE");
        assert!(suggestions_en("x").len() <= 8);
    }

    #[test]
    fn a_complete_name_with_nothing_else_has_no_completion() {
        let mut app = CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        app.cmd.buffer = "circle".to_string();
        assert_eq!(completion(&app), None);
        app.cmd.buffer = "circ".to_string();
        assert_eq!(completion(&app).as_deref(), Some("LE"));
    }

    #[test]
    fn exact_alias_is_first_suggestion_with_alias_display() {
        let rec = suggestions_en("rec");
        assert!(!rec.is_empty());
        assert_eq!(rec[0].id, "RECTANG");
        assert_eq!(rec[0].display, "REC (RECTANG)");
        assert_eq!(rec[0].label, "Rectangle");
        // RECOVER should follow RECTANG, not precede it
        assert!(rec.iter().any(|s| s.id == "RECOVER"));
        assert_eq!(rec[1].id, "RECOVER");
        assert_eq!(rec[1].display, "RECOVER");

        let l = suggestions_en("l");
        assert_eq!(l[0].id, "LINE");
        assert_eq!(l[0].display, "L (LINE)");

        let c = suggestions_en("c");
        assert_eq!(c[0].id, "CIRCLE");
        assert_eq!(c[0].display, "C (CIRCLE)");

        let co = suggestions_en("co");
        assert_eq!(co[0].id, "COPY");
        assert_eq!(co[0].display, "CO (COPY)");
    }

    #[test]
    fn prefix_without_alias_shows_command_id() {
        let rect = suggestions_en("rect");
        assert_eq!(rect[0].id, "RECTANG");
        assert_eq!(rect[0].display, "RECTANG");

        let line = suggestions_en("line");
        assert_eq!(line[0].id, "LINE");
        assert_eq!(line[0].display, "LINE");
    }

    #[test]
    fn tab_moves_down_the_autocomplete_list_and_wraps() {
        let mut app = CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        app.cmd.buffer = "rec".to_string();
        assert_eq!(completion(&app).as_deref(), Some("TANG"));
        // Tab moves down the list as Down does, back to the first row after the last.
        let n = suggestions_en("rec").len();
        assert!(n > 1);
        tab(&mut app);
        assert_eq!(app.cmd.buffer, "rec");
        assert_eq!(app.cmd.suggestion, 1);
        for _ in 1..n {
            tab(&mut app);
        }
        assert_eq!(app.cmd.suggestion, 0);
    }
}
