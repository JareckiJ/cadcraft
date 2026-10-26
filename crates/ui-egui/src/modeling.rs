//! Local parametric modeling workbench. Every model edit uses engine commands.
use cadcraft_engine::cmd::modeling::bodies;
use egui::{Sense, Shape, Stroke, pos2, vec2};
use serde_json::json;

use crate::{CadApp, theme::Tokens};

fn run(app: &mut CadApp, ctx: &egui::Context, command: &str, params: serde_json::Value) {
    let error_id = egui::Id::unique("model_error");
    match app.run(command, params) {
        Ok(_) => ctx.data_mut(|d| {
            d.remove::<String>(error_id);
        }),
        Err(e) => ctx.data_mut(|d| {
            d.insert_temp(error_id, e);
        }),
    }
}

pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    egui::Window::new("Parametric 3D · prototype").open(open).default_size(vec2(800.0, 600.0)).show(ctx, |ui| {
        let t = Tokens::get();
        ui.label("XY sketch → rectangular extrusion");
        ui.horizontal(|ui| {
            let can_create = app.session.doc().is_ok_and(|d| d.parametric.extrusions.is_empty());
            if ui.add_enabled(can_create, egui::Button::new("Create 40 × 20 × 10 part")).clicked() {
                run(app, ctx, "model.block", json!({"width":40,"height":20,"depth":10}));
            }
            if ui.button("Undo").clicked() {
                let _ = app.session.undo();
            }
            if ui.button("Redo").clicked() {
                let _ = app.session.redo();
            }
        });
        let rows = app.session.doc().ok().map(cadcraft_engine::cmd::modeling::parameters).unwrap_or_default();
        egui::Grid::new("model_parameters").num_columns(3).show(ui, |ui| {
            ui.strong("Parameter");
            ui.strong("Expression (Enter to apply)");
            ui.strong("Value");
            ui.end_row();
            for row in rows.iter().filter(|r| r.kind == "user") {
                let label = ui.label(&row.name);
                if let Some(expr) = crate::parametric::expr_field(ui, egui::Id::unique(("model_expr", &row.name)), &row.expr, Some(label.id)) {
                    run(app, ctx, "parameters", json!({"name":row.name,"expr":expr}));
                }
                ui.label(row.value.map(|v| format!("{v:.3}")).unwrap_or_else(|| "Error".into()));
                ui.end_row();
            }
        });
        if let Some(error) = ctx.data_mut(|d| d.get_temp::<String>(egui::Id::unique("model_error"))) {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let result = app.session.doc().map_err(|e| e.to_string()).and_then(|d| bodies(d).map_err(|e| e.to_string()));
        match result {
            Ok(parts) if !parts.is_empty() => {
                if let Some(body) = parts.first() {
                    ui.label(format!("Features: XY sketch {} → Extrude    |    Volume: {:.3} drawing units³", body.sketch, body.volume));
                    let id = egui::Id::unique("model_camera");
                    let (mut yaw, mut pitch) = ui.ctx().data_mut(|d| d.get_temp::<(f64, f64)>(id).unwrap_or((0.65, 0.55)));
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), 350.0), Sense::drag());
                    if response.dragged() {
                        let delta = ui.input(|i| i.pointer.delta());
                        yaw += f64::from(delta.x) * 0.01;
                        pitch = (pitch + f64::from(delta.y) * 0.01).clamp(-1.5, 1.5);
                    }
                    ui.ctx().data_mut(|d| d.insert_temp(id, (yaw, pitch)));
                    let painter = ui.painter_at(rect);
                    painter.rect_filled(rect, 4.0, t.canvas);
                    let origin = body.vertices.first().copied().unwrap_or([0.0; 3]);
                    let center = [origin[0] + body.width / 2.0, origin[1] + body.height / 2.0, origin[2] + body.depth / 2.0];
                    let diagonal = body.width.hypot(body.height).hypot(body.depth);
                    let scale = f64::from(rect.width().min(rect.height())) * 0.72 / diagonal;
                    let projected: Vec<_> = body
                        .vertices
                        .iter()
                        .map(|p| {
                            let (x, y, z) = (p[0] - center[0], p[1] - center[1], p[2] - center[2]);
                            let rx = x * yaw.cos() - y * yaw.sin();
                            let ry = x * yaw.sin() + y * yaw.cos();
                            let sy = ry * pitch.sin() - z * pitch.cos();
                            let depth = ry * pitch.cos() + z * pitch.sin();
                            (pos2(rect.center().x + (rx * scale) as f32, rect.center().y + (sy * scale) as f32), depth)
                        })
                        .collect();
                    let mut faces: Vec<_> = body
                        .faces
                        .iter()
                        .enumerate()
                        .map(|(i, f)| {
                            let depth = f.iter().filter_map(|n| projected.get(*n)).map(|p| p.1).sum::<f64>();
                            (i, f, depth)
                        })
                        .collect();
                    faces.sort_by(|a, b| a.2.total_cmp(&b.2));
                    for (i, face, _) in faces {
                        let points: Vec<_> = face.iter().filter_map(|n| projected.get(*n).map(|p| p.0)).collect();
                        // The outward faces visible from this camera wind clockwise
                        // in screen coordinates, as egui's convex fill requires.
                        let area: f32 = points.iter().zip(points.iter().cycle().skip(1)).take(points.len()).map(|(a, b)| a.x * b.y - a.y * b.x).sum();
                        if area <= 0.0 {
                            continue;
                        }
                        let shade = 0.45 + (i as f32) * 0.09;
                        let fill = egui::Color32::from_rgb(
                            (f32::from(t.accent.r()) * shade) as u8,
                            (f32::from(t.accent.g()) * shade) as u8,
                            (f32::from(t.accent.b()) * shade) as u8,
                        );
                        painter.add(Shape::convex_polygon(points, fill, Stroke::new(1.5, t.text)));
                    }
                    ui.label("Drag to orbit. Dimensions use the drawing's units. Save as DXF to retain editable features.");
                }
            }
            Ok(_) => {
                ui.label("Create a part to begin. The prototype currently supports one rectangular sketch extrusion.");
            }
            Err(e) => {
                ui.colored_label(t.text, e);
            }
        }
    });
}
