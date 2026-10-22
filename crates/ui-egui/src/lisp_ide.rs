//! AutoLISP / VBA IDE and Application Loader (`VBAIDE`, `VLIDE`, `NETLOAD`).
//!
//! Provides an interactive multi-line LISP code editor, `.lsp` file loader (`NETLOAD` / `APPLOAD`),
//! `.lsp` file saver, one-click execution of `(defun c:... ())` commands defined in the script,
//! quick expression evaluator, and output console.

use egui::{RichText, vec2};
use serde_json::Value;

use crate::CadApp;
use crate::theme::Tokens;

const DEFAULT_LISP: &str = r#";; CADCraft AutoLISP / VBAIDE Studio
;; Klik op [▶ Laad & Voer uit] om onderstaande functies te laden,
;; of open een extern .lsp bestand via [📂 Open .lsp (NETLOAD)].

(defun add-numbers (a b)
  (+ a b)
)

;; Commando C:HELLO -> typ HELLO op de commandline of klik hieronder op [▶ HELLO]
(defun c:hello ()
  (princ "\nWelkom in CADCraft AutoLISP! (+ 1 2) = ")
  (princ (+ 1 2))
  (princ)
)

;; Commando C:KADER -> tekent een rechthoek van 100x50 op (0,0)
(defun c:kader ()
  (command "_.RECTANG" '(0 0) '(100 50))
  (princ "\nKader 100x50 getekend.")
  (princ)
)
"#;

#[derive(Clone, Debug)]
pub struct LispIdeState {
    pub file_name: String,
    pub file_path: Option<String>,
    pub code: String,
    pub quick_expr: String,
    pub output: Vec<String>,
    pub commands: Vec<String>,
    pub status: Option<String>,
}

impl Default for LispIdeState {
    fn default() -> Self {
        let code = DEFAULT_LISP.to_string();
        let commands = extract_c_commands(&code);
        Self {
            file_name: "routine.lsp".into(),
            file_path: None,
            code,
            quick_expr: "(+ 1 2)".into(),
            output: vec!["Klaar. Klik op '▶ Laad & Voer uit' of open een .lsp bestand.".into()],
            commands,
            status: None,
        }
    }
}

/// Extract command names defined via `(defun c:name ...)` from LISP source text.
pub fn extract_c_commands(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let code_part = line.split(';').next().unwrap_or("");
        let lower = code_part.to_ascii_lowercase();
        let mut rest = lower.as_str();
        while let Some(pos) = rest.find("c:") {
            let after = &rest[pos + 2..];
            let name: String = after
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '(' && *c != ')')
                .collect();
            if !name.is_empty() && !out.iter().any(|x: &String| x.eq_ignore_ascii_case(&name)) {
                out.push(name.to_ascii_uppercase());
            }
            rest = after;
        }
    }
    out
}

pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    match id {
        "vbaide" | "vlide" | "vlisp" if params.is_null() => {
            if app.lisp_ide.is_none() {
                app.lisp_ide = Some(LispIdeState::default());
            }
            app.ui.dialog = Some("vbaide".into());
            Some(Ok(Value::Null))
        }
        "netload" if params.is_null() => {
            if app.lisp_ide.is_none() {
                app.lisp_ide = Some(LispIdeState::default());
            }
            app.ui.dialog = Some("vbaide".into());
            // Also open the native file picker immediately if available.
            open_file_into_ide(app);
            Some(Ok(Value::Null))
        }
        _ => None,
    }
}

pub fn open_file_into_ide(app: &mut CadApp) {
    let picked = app.services.pick_open.as_ref().and_then(|f| f());
    let Some(path) = picked else { return };
    load_path_into_ide(app, &path);
}

pub fn load_path_into_ide(app: &mut CadApp, path: &str) {
    match cadcraft_engine::lisp::builtins::read_text_file(path) {
        Some(text) => {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            let cmds = extract_c_commands(&text);
            let ide = app.lisp_ide.get_or_insert_with(LispIdeState::default);
            ide.file_name = name.clone();
            ide.file_path = Some(path.to_string());
            ide.code = text.clone();
            ide.commands = cmds.clone();
            let log_before = app.session.log.len();
            match app.session.lisp_load(&name, &text, false) {
                Ok(res) => {
                    for line in app.session.log.get(log_before..).unwrap_or(&[]) {
                        ide.output.push(line.clone());
                    }
                    if let Some(Ok(v)) = res {
                        let printed = cadcraft_engine::lisp::value::to_text(&v, true);
                        if printed != "nil" {
                            ide.output.push(format!("⇒ {printed}"));
                        }
                    } else if let Some(Err(e)) = res {
                        ide.output.push(format!("Fout: {e}"));
                    }
                    let msg = if cmds.is_empty() {
                        format!("Geladen: {name}")
                    } else {
                        format!("Geladen: {name} (Commando's: {})", cmds.join(", "))
                    };
                    ide.output.push(msg.clone());
                    ide.status = Some(msg.clone());
                    app.session.echo(msg);
                }
                Err(e) => {
                    let msg = format!("Fout bij laden {name}: {e}");
                    ide.output.push(msg.clone());
                    ide.status = Some(msg.clone());
                    app.session.echo(msg);
                }
            }
        }
        None => {
            let msg = format!("NETLOAD: kan bestand niet lezen: {path}");
            if let Some(ide) = &mut app.lisp_ide {
                ide.output.push(msg.clone());
                ide.status = Some(msg.clone());
            }
            app.session.echo(msg);
        }
    }
}

fn execute_current_code(app: &mut CadApp) {
    let Some(ide) = app.lisp_ide.as_ref() else { return };
    let name = ide.file_name.clone();
    let code = ide.code.clone();
    let cmds = extract_c_commands(&code);
    let log_before = app.session.log.len();
    let res = app.session.lisp_load(&name, &code, false);
    let new_logs: Vec<String> = app.session.log.get(log_before..).map(|s| s.to_vec()).unwrap_or_default();
    let Some(ide) = app.lisp_ide.as_mut() else { return };
    ide.commands = cmds.clone();
    for l in new_logs {
        ide.output.push(l);
    }
    match res {
        Ok(Some(Ok(v))) => {
            let txt = cadcraft_engine::lisp::value::to_text(&v, true);
            ide.output.push(format!("⇒ {txt}"));
            let msg = if cmds.is_empty() {
                format!("Uitgevoerd ({name}) ⇒ {txt}")
            } else {
                format!("Geladen ({name}) — Beschikbare commando's: {}", cmds.join(", "))
            };
            ide.status = Some(msg);
        }
        Ok(Some(Err(e))) => {
            ide.output.push(format!("Fout: {e}"));
            ide.status = Some(format!("Fout: {e}"));
        }
        Ok(None) => {
            ide.status = Some(format!("Routine gestart ({name})..."));
        }
        Err(e) => {
            ide.output.push(format!("Fout: {e}"));
            ide.status = Some(format!("Fout: {e}"));
        }
    }
}

fn eval_quick_expr(app: &mut CadApp) {
    let Some(ide) = app.lisp_ide.as_ref() else { return };
    let raw = ide.quick_expr.trim().to_string();
    if raw.is_empty() {
        return;
    }
    let expr = if cadcraft_engine::is_bare_lisp_prefix(&raw) {
        format!("({raw})")
    } else {
        raw.clone()
    };
    let log_before = app.session.log.len();
    let res = app.session.lisp_eval(&expr, true, false, Vec::new());
    let new_logs: Vec<String> = app.session.log.get(log_before..).map(|s| s.to_vec()).unwrap_or_default();
    let Some(ide) = app.lisp_ide.as_mut() else { return };
    ide.output.push(format!("> {raw}"));
    for l in new_logs {
        ide.output.push(format!("  {l}"));
    }
    if let Err(e) = res {
        ide.output.push(format!("  Fout: {e}"));
    }
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    if app.lisp_ide.is_none() {
        app.lisp_ide = Some(LispIdeState::default());
    }
    let t = Tokens::get();
    let mut do_open_file = false;
    let mut do_save_file = false;
    let mut do_run_code = false;
    let mut do_eval_quick = false;
    let mut run_cmd: Option<String> = None;

    egui::Window::new("AutoLISP / VBA IDE & NETLOAD Studio")
        .id(egui::Id::new("vbaide_window"))
        .open(open)
        .default_size(vec2(760.0, 560.0))
        .min_size(vec2(520.0, 380.0))
        .resizable(true)
        .show(ctx, |ui| {
            let Some(ide) = app.lisp_ide.as_mut() else { return };

            // Top toolbar
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(RichText::new("📂 Open .lsp (NETLOAD)").strong())
                    .on_hover_text("Open en laad een AutoLISP (.lsp) bestand vanaf schijf")
                    .clicked()
                {
                    do_open_file = true;
                }
                if ui
                    .button("💾 Opslaan .lsp")
                    .on_hover_text("Sla de huidige LISP routine op als .lsp bestand")
                    .clicked()
                {
                    do_save_file = true;
                }
                if ui
                    .button(RichText::new("▶ Laad & Voer uit").strong().color(egui::Color32::from_rgb(0x4e, 0xc9, 0x6f)))
                    .on_hover_text("Evalueer de volledige LISP code in de editor en registreer alle (defun c:...) commando's")
                    .clicked()
                {
                    do_run_code = true;
                }

                ui.separator();
                ui.label("Voorbeeld:");
                egui::ComboBox::from_id_salt("lisp_template_combo")
                    .selected_text("Kies sjabloon...")
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(false, "1. Basis Rekenen & Commando's").clicked() {
                            ide.code = DEFAULT_LISP.to_string();
                            ide.commands = extract_c_commands(&ide.code);
                        }
                        if ui.selectable_label(false, "2. Teken Kruispunt / Meetpunt (c:meetpunt)").clicked() {
                            ide.code = r#";; Tekent een cirkel met kruis op een gekozen punt
(defun c:meetpunt (/ pt)
  (setq pt (getpoint "\nWijs meetpunt aan: "))
  (if pt
    (progn
      (command "_.CIRCLE" pt 1.5)
      (command "_.LINE" (list (- (car pt) 2.0) (cadr pt)) (list (+ (car pt) 2.0) (cadr pt)) "")
      (command "_.LINE" (list (car pt) (- (cadr pt) 2.0)) (list (car pt) (+ (cadr pt) 2.0)) "")
      (princ "\nMeetpunt geplaatst!")
    )
  )
  (princ)
)
"#
                            .to_string();
                            ide.commands = extract_c_commands(&ide.code);
                        }
                        if ui.selectable_label(false, "3. Object Eigenschappen Tonen (c:objinfo)").clicked() {
                            ide.code = r#";; Toont de DXF groepen van het laatst getekende object
(defun c:objinfo (/ ent data)
  (setq ent (entlast))
  (if ent
    (progn
      (setq data (entget ent))
      (princ "\nLaatste entiteit DXF-data: ")
      (princ data)
    )
    (princ "\nGeen objecten in tekening.")
  )
  (princ)
)
"#
                            .to_string();
                            ide.commands = extract_c_commands(&ide.code);
                        }
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(&ide.file_name).monospace().color(t.text_dim));
                });
            });

            // Detected `c:` commands bar
            if !ide.commands.is_empty() {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Geladen Commando's (klik om te starten):").strong());
                    for cmd in &ide.commands {
                        if ui
                            .button(RichText::new(format!("▶ {cmd}")).monospace().strong())
                            .on_hover_text(format!("Voer commando {cmd} direct uit in de tekening"))
                            .clicked()
                        {
                            run_cmd = Some(cmd.clone());
                        }
                    }
                });
            }

            ui.separator();

            // Code editor
            ui.label(RichText::new("AutoLISP Editor (.lsp):").strong());
            egui::ScrollArea::vertical()
                .id_salt("lisp_ide_editor_scroll")
                .max_height(280.0)
                .show(ui, |ui| {
                    let te = egui::TextEdit::multiline(&mut ide.code)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(14);
                    if ui.add(te).changed() {
                        ide.commands = extract_c_commands(&ide.code);
                    }
                });

            ui.add_space(4.0);

            // Quick expression bar (`(+ 1 2)` or `+ 1 2`)
            ui.horizontal(|ui| {
                ui.label(RichText::new("Directe Expressie:").strong());
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut ide.quick_expr)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(340.0)
                        .hint_text("bijv. (+ 1 2) of + 1 2"),
                );
                if (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    || ui.button("Evalueer").clicked()
                {
                    do_eval_quick = true;
                }
                if ui.button("Wis Log").clicked() {
                    ide.output.clear();
                }
            });

            ui.separator();

            // Output console
            ui.label(RichText::new("Console / Resultaat:").strong());
            egui::Frame::NONE
                .fill(t.chrome_dark)
                .inner_margin(egui::Margin::same(6))
                .corner_radius(4.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("lisp_ide_output_scroll")
                        .stick_to_bottom(true)
                        .max_height(130.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            for line in &ide.output {
                                ui.label(RichText::new(line).monospace().size(12.0));
                            }
                        });
                });

            if let Some(st) = &ide.status {
                ui.add_space(2.0);
                ui.label(RichText::new(st).color(t.text_dim).size(11.5));
            }
        });

    if do_open_file {
        open_file_into_ide(app);
    }
    if do_save_file {
        let suggested = app
            .lisp_ide
            .as_ref()
            .map(|i| i.file_name.clone())
            .unwrap_or_else(|| "routine.lsp".into());
        if let Some(path) = app.services.pick_save.as_ref().and_then(|f| f(&suggested))
            && let Some(ide) = app.lisp_ide.as_mut()
        {
            match std::fs::write(&path, &ide.code) {
                Ok(()) => {
                    ide.file_path = Some(path.clone());
                    if let Some(name) = std::path::Path::new(&path).file_name() {
                        ide.file_name = name.to_string_lossy().to_string();
                    }
                    let msg = format!("Opgeslagen naar {path}");
                    ide.output.push(msg.clone());
                    ide.status = Some(msg);
                }
                Err(e) => {
                    let msg = format!("Kon niet opslaan naar {path}: {e}");
                    ide.output.push(msg.clone());
                    ide.status = Some(msg);
                }
            }
        }
    }
    if do_run_code {
        execute_current_code(app);
    }
    if do_eval_quick {
        eval_quick_expr(app);
    }
    if let Some(cmd) = run_cmd {
        // First ensure the current code is loaded so the command is registered, then run it.
        execute_current_code(app);
        app.cmdline(&cmd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_c_commands_from_lisp_source() {
        let cmds = extract_c_commands(DEFAULT_LISP);
        assert_eq!(cmds, vec!["HELLO".to_string(), "KADER".to_string()]);
    }
}
