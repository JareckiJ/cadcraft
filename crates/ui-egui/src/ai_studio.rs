//! Local RoboDhan workspace. Network requests run off the editor thread.
use serde_json::{Value, json};
use std::sync::mpsc::{self, Receiver};

pub struct Studio {
    pub visible: bool,
    pub structured: bool,
    pub prompt: String,
    pub language: String,
    pub result: String,
    pub status: String,
    pending: Option<Receiver<Result<String, String>>>,
}
impl Default for Studio {
    fn default() -> Self {
        Self {
            visible: false,
            structured: false,
            prompt: String::new(),
            language: "Indonesia".into(),
            result: String::new(),
            status: String::new(),
            pending: None,
        }
    }
}
pub enum Action {
    Generate,
    Apply,
    Save,
}
impl Studio {
    pub fn draw(&mut self, ui: &mut egui::Ui, title: &str, apply: &str) -> Option<Action> {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(text) => {
                            self.result = text;
                            self.status = "Hasil siap ditinjau.".into();
                        }
                        Err(error) => self.status = error,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.status = "Proses AI terputus.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                }
            }
        }
        egui::Panel::top("robodhan-toggle").show(ui, |ui| {
            ui.horizontal(|ui| {
                #[cfg(target_os = "macos")]
                ui.add_space(100.0);
                let toggle_label = if self.visible { "Sembunyikan AI Studio" } else { "Tampilkan AI Studio" };
                ui.toggle_value(&mut self.visible, toggle_label);
                if self.pending.is_some() {
                    ui.spinner();
                }
            });
        });
        if !self.visible {
            return None;
        }
        let mut action = None;
        egui::Panel::right("robodhan-studio").resizable(true).default_size(310.0).show(ui, |ui| {
            ui.heading("AI Studio · RoboDhan");
            ui.label(title);
            ui.horizontal(|ui| {
                ui.label("Bahasa hasil:");
                ui.text_edit_singleline(&mut self.language);
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.prompt)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .char_limit(4000)
                    .hint_text("Instruksi tambahan…"),
            );
            if ui.add_enabled(self.pending.is_none(), egui::Button::new("Proses dengan AI lokal")).clicked() {
                action = Some(Action::Generate);
            }
            ui.label(&self.status);
            ui.separator();
            egui::ScrollArea::vertical().max_height(450.0).show(ui, |ui| {
                if self.structured {
                    ui.label(friendly_preview(&self.result));
                } else {
                    ui.add(egui::TextEdit::multiline(&mut self.result).desired_rows(14).desired_width(f32::INFINITY).char_limit(100_000));
                }
            });
            ui.horizontal_wrapped(|ui| {
                let ready = self.pending.is_none() && !self.result.trim().is_empty();
                if ui.add_enabled(ready, egui::Button::new("Salin hasil")).clicked() {
                    ui.ctx().copy_text(self.result.clone());
                }
                if ui.add_enabled(ready, egui::Button::new("Simpan hasil…")).clicked() {
                    action = Some(Action::Save);
                }
                if !apply.is_empty() && ui.add_enabled(ready, egui::Button::new(apply)).clicked() {
                    action = Some(Action::Apply);
                }
            });
            ui.small("API lokal 127.0.0.1:1234. Model mengikuti RoboDhan. Hasil tidak diterapkan otomatis.");
        });
        action
    }
    pub fn start(&mut self, ctx: &egui::Context, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
        if self.pending.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new().name("robodhan-studio".into()).spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).unwrap_or_else(|_| Err("Proses AI gagal; dokumen tetap aman.".into()));
            let _ = tx.send(result);
            ctx.request_repaint();
        }) {
            Ok(_) => {
                self.pending = Some(rx);
                self.status = "Menunggu RoboDhan…".into();
            }
            Err(error) => self.status = error.to_string(),
        }
    }
}

fn friendly_preview(text: &str) -> String {
    let text = text.trim();
    let source =
        if text.starts_with("```") { text.split_once('\n').map(|(_, rest)| rest.trim_end_matches('`').trim()).unwrap_or(text) } else { text };
    let Ok(value) = serde_json::from_str::<Value>(source) else {
        return if text.is_empty() { String::new() } else { "Hasil belum dapat dibaca. Coba instruksi yang lebih sederhana.".into() };
    };
    if let Some(slides) = value["slides"].as_array() {
        return slides
            .iter()
            .take(12)
            .enumerate()
            .map(|(i, slide)| format!("Slide {} · {}\n{}", i + 1, slide["title"].as_str().unwrap_or(""), slide["body"].as_str().unwrap_or("")))
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    let mut out = value["explanation"].as_str().unwrap_or("").to_owned();
    if let Some(commands) = value["commands"].as_array() {
        out.push_str(&format!("\n\n{} objek dalam rancangan:", commands.len()));
        for command in commands.iter().take(100) {
            let name = match command["command"].as_str().unwrap_or("") {
                "circle" => "Lingkaran",
                "line" => "Garis",
                "rectang" => "Persegi panjang",
                _ => "Objek tidak didukung",
            };
            out.push_str(&format!("\n• {name}: {}", command["params"]));
        }
    }
    out
}

pub fn ask(task: &str, prompt: String, language: String, source: String) -> Result<String, String> {
    if source.len() > 240_000 {
        return Err("Sumber terlalu panjang (maksimum 240 KB). Pilih bagian lebih kecil.".into());
    }
    if !matches!(task, "pdf.summary" | "word.translate") || source.chars().count() <= 10_000 {
        return ask_once(task, prompt, language, source);
    }
    // Bound each local-model context; never silently omit the rest of a document.
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    for (index, ch) in source.chars().enumerate() {
        chunk.push(ch);
        if (index + 1).is_multiple_of(10_000) {
            chunks.push(std::mem::take(&mut chunk));
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    let mut results = Vec::new();
    for (index, chunk) in chunks.into_iter().enumerate() {
        results.push(ask_once(task, format!("Bagian {}. {}", index + 1, prompt), language.clone(), chunk)?);
    }
    let joined = results.join("\n\n");
    if task == "pdf.summary" {
        if joined.len() > 240_000 {
            return Err("Ringkasan antara terlalu panjang. Ringkas dokumen yang lebih pendek.".into());
        }
        ask_once(task, format!("Gabungkan ringkasan bagian berikut tanpa menghilangkan kesimpulan dan referensi halaman. {prompt}"), language, joined)
    } else {
        Ok(joined)
    }
}

fn ask_once(task: &str, prompt: String, language: String, source: String) -> Result<String, String> {
    use std::io::Read;
    if source.len() > 240_000 {
        return Err("Sumber terlalu panjang (maksimum 240 KB). Pilih bagian lebih kecil.".into());
    }
    let agent: ureq::Agent =
        ureq::Agent::config_builder().proxy(None).max_redirects(0).timeout_global(Some(std::time::Duration::from_secs(600))).build().into();
    let body = json!({"task":task,"prompt":prompt,"language":language,"source":source});
    let mut response = agent
        .post("http://127.0.0.1:1234/v1/craft/tasks")
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| format!("RoboDhan: {e}. Aktifkan API lokal dan gunakan RoboDhan versi integrasi Craft."))?;
    let mut bytes = Vec::new();
    response.body_mut().as_reader().take(512 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 512 * 1024 {
        return Err("Respons AI terlalu besar.".into());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    value["choices"][0]["message"]["content"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_owned())
        .ok_or_else(|| "RoboDhan tidak mengembalikan hasil.".into())
}

pub fn save(text: &str, extension: &str) -> Result<Option<std::path::PathBuf>, String> {
    let Some(path) = rfd::FileDialog::new().set_file_name(format!("hasil-ai.{extension}")).add_filter("Hasil AI", &[extension]).save_file() else {
        return Ok(None);
    };
    use std::io::Write;
    let parent = path.parent().ok_or("Folder tujuan tidak valid")?;
    static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stamp = NEXT_TEMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = parent.join(format!(".craft-ai-{}-{stamp}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
    let written = (|| -> std::io::Result<()> {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, &path)
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    Ok(Some(path))
}

pub fn json_result(text: &str) -> Result<Value, String> {
    let text = text.trim();
    let text = if text.starts_with("```") { text.split_once('\n').map(|(_, rest)| rest.trim_end_matches('`').trim()).unwrap_or(text) } else { text };
    serde_json::from_str(text).map_err(|e| format!("Format hasil AI tidak valid: {e}. Perbaiki prompt lalu coba lagi."))
}

fn point(value: &Value) -> bool {
    value.as_array().is_some_and(|a| a.len() == 2 && a.iter().all(|v| v.as_f64().is_some_and(|n| n.is_finite() && n.abs() <= 1_000_000.0)))
}
fn commands(text: &str) -> Result<Vec<(String, Value)>, String> {
    let value = json_result(text)?;
    let commands = value["commands"].as_array().filter(|a| !a.is_empty() && a.len() <= 100).ok_or("Rancangan harus berisi 1–100 objek.")?;
    commands
        .iter()
        .map(|v| {
            let command = v["command"].as_str().ok_or("Perintah tidak valid.")?;
            let p = &v["params"];
            let valid = match command {
                "line" => p["points"].as_array().is_some_and(|a| a.len() >= 2 && a.len() <= 100 && a.iter().all(point)),
                "circle" => point(&p["center"]) && p["radius"].as_f64().is_some_and(|n| n.is_finite() && n > 0.0 && n <= 1_000_000.0),
                "rectang" => point(&p["p1"]) && point(&p["p2"]),
                _ => false,
            };
            if !valid {
                return Err(format!("Geometri {command} tidak valid atau tidak didukung."));
            }
            // Discard all AI-supplied keys except the simple geometry parameters.
            let params = match command {
                "line" => json!({"points":p["points"]}),
                "circle" => json!({"center":p["center"],"radius":p["radius"]}),
                _ => json!({"p1":p["p1"],"p2":p["p2"]}),
            };
            Ok((command.to_owned(), params))
        })
        .collect()
}
pub fn show(app: &mut crate::CadApp, ui: &mut egui::Ui) {
    let mut state = std::mem::take(&mut app.ai_studio);
    state.structured = true;
    match state.draw(
        ui,
        "Rancang geometri 2D: garis, lingkaran, persegi panjang. Masukkan ukuran dalam milimeter (mm). Periksa dimensi dan asumsi hasil AI sebelum membuat gambar baru.",
        "Buat gambar baru dari rancangan",
    ) {
        Some(Action::Generate) => {
            if state.prompt.trim().is_empty() {
                state.status = "Masukkan deskripsi dan dimensi rancangan.".into();
            } else {
                let (prompt, language) = (state.prompt.clone(), state.language.clone());
                state.start(ui.ctx(), move || ask("cad.plan", prompt, language, String::new()));
            }
        }
        Some(Action::Apply) => {
            state.status = match commands(&state.result).and_then(|commands| {
                let mut preview = cadcraft_engine::Session::new();
                for (command, params) in &commands {
                    preview.execute(command, params).map_err(|e| e.to_string())?;
                }
                app.session.new_drawing(true);
                app.ui.start_tab = false;
                for (command, params) in commands {
                    app.run(&command, params)?;
                }
                app.run("zoom.extents", json!({}))?;
                Ok(())
            }) {
                Ok(()) => "Gambar baru dibuat. Periksa dimensi dan simpan melalui Save As.".into(),
                Err(e) => e,
            };
        }
        Some(Action::Save) => {
            if let Err(e) = save(&state.result, "json") {
                state.status = e;
            }
        }
        None => {}
    }
    app.ai_studio = state;
}

#[cfg(test)]
mod studio_tests {
    use super::*;
    #[test]
    fn hidden_panel_keeps_prompt_and_pending_result() {
        let mut state = Studio { prompt: "keep this".into(), ..Default::default() };
        let (tx, rx) = mpsc::channel();
        state.pending = Some(rx);
        tx.send(Ok("completed".into())).unwrap();
        let mut frame = egui::Context::default().run_ui(Default::default(), |ui| {
            state.draw(ui, "test", "");
        });
        frame.textures_delta.clear();
        assert!(!state.visible);
        assert_eq!(state.prompt, "keep this");
        assert_eq!(state.result, "completed");
        assert!(state.pending.is_none());
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    #[test]
    fn validates_geometry_and_refuses_other_commands() {
        let parsed = commands(r#"{"commands":[{"command":"circle","params":{"center":[0,0],"radius":5}}]}"#).unwrap();
        let mut session = cadcraft_engine::Session::new();
        for (command, params) in parsed {
            session.execute(&command, &params).unwrap();
        }
        for invalid in [
            r#"{"commands":[{"command":"erase","params":{}}]}"#,
            r#"{"commands":[{"command":"circle","params":{"center":[0,0],"radius":-1}}]}"#,
            r#"{"commands":[{"command":"line","params":{"points":[[0,0],[1e20,0]]}}]}"#,
        ] {
            assert!(commands(invalid).is_err());
        }
    }
}
