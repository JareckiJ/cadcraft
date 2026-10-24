//! 3D projected ViewCube geometry, compass ring, and hit-testing (faces, edges, corners).
//!
//! Renders an AutoCAD-style interactive 3D ViewCube that rotates live with [`cadcraft_render::Orbit3d`]
//! (`yaw` and `pitch`), tilts the compass ring (`N`, `E`, `S`, `W`) in 3D space, highlights hovered
//! sub-regions (6 faces, 12 edges, 8 corners), and computes target `(yaw_deg, pitch_deg)` camera
//! orientations when any face, edge, or corner is clicked.

use cadcraft_geom::Vec3;
use cadcraft_render::Orbit3d;
use egui::{Color32, Pos2, Shape, Stroke, epaint::PathShape, pos2, vec2};

/// A clickable sub-region on the 3D ViewCube surface (6 faces, 12 edges, 8 corners), identified by
/// its outward normal sign triple `(nx, ny, nz)` where each component is in `{-1, 0, 1}` (not all zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CubeZone {
    pub nx: i8,
    pub ny: i8,
    pub nz: i8,
}

impl CubeZone {
    /// Construct a zone from signs `(-1..=1)`.
    pub const fn new(nx: i8, ny: i8, nz: i8) -> Self {
        Self { nx, ny, nz }
    }

    /// Number of non-zero components: `1` = face, `2` = edge, `3` = corner.
    pub const fn rank(self) -> u8 {
        (self.nx != 0) as u8 + (self.ny != 0) as u8 + (self.nz != 0) as u8
    }

    /// Outward direction vector in world space.
    pub fn direction(self) -> Vec3 {
        Vec3::new(f64::from(self.nx), f64::from(self.ny), f64::from(self.nz)).normalized()
    }

    /// Target `(yaw_deg, pitch_deg)` for `3dorbit` that aligns the camera to look from `self.direction()`
    /// toward the origin.
    pub fn target_angles_deg(self, current_yaw_deg: f64) -> (f64, f64) {
        let dir = self.direction();
        // Camera looks along:
        // dir.x = -sin(yaw) * sin(pitch)
        // dir.y = -cos(yaw) * sin(pitch)
        // dir.z =  cos(pitch)
        let pitch_rad = dir.z.clamp(-1.0, 1.0).acos();
        let pitch_deg = pitch_rad.to_degrees();
        let xy_len = dir.x.hypot(dir.y);
        let yaw_deg = if xy_len < 1e-6 {
            // Looking straight at Top (0,0,1) or Bottom (0,0,-1): snap current yaw to nearest 90° step
            // or 0° if already in Top view.
            if self.nz > 0 { 0.0 } else { (current_yaw_deg / 90.0).round() * 90.0 }
        } else {
            (-dir.x).atan2(-dir.y).to_degrees()
        };
        (yaw_deg, pitch_deg)
    }

    /// Human-readable tooltip describing the hovered face, edge, or corner.
    pub fn tooltip(self) -> String {
        match (self.nx, self.ny, self.nz) {
            (0, 0, 1) => "Top view (click to align, drag to orbit)".to_string(),
            (0, 0, -1) => "Bottom view (click to align, drag to orbit)".to_string(),
            (0, -1, 0) => "Front view (click to align, drag to orbit)".to_string(),
            (0, 1, 0) => "Back view (click to align, drag to orbit)".to_string(),
            (-1, 0, 0) => "Left view (click to align, drag to orbit)".to_string(),
            (1, 0, 0) => "Right view (click to align, drag to orbit)".to_string(),
            _ => {
                let mut parts = Vec::with_capacity(3);
                if self.nz > 0 {
                    parts.push("Top");
                } else if self.nz < 0 {
                    parts.push("Bottom");
                }
                if self.ny < 0 {
                    parts.push("Front");
                } else if self.ny > 0 {
                    parts.push("Back");
                }
                if self.nx < 0 {
                    parts.push("Left");
                } else if self.nx > 0 {
                    parts.push("Right");
                }
                format!("{} view (click to align, drag to orbit)", parts.join("-"))
            }
        }
    }
}

/// Definition of one of the 6 primary cube faces in local cube coordinates `[-1, 1]^3`.
#[derive(Clone, Copy)]
struct FaceDef {
    normal: (i8, i8, i8),
    /// Local `+u` tangent axis (rightward when looking at the face).
    u_axis: Vec3,
    /// Local `+v` tangent axis (upward when looking at the face).
    v_axis: Vec3,
    /// Unlocalized face key (`"TOP"`, `"BOTTOM"`, `"FRONT"`, `"BACK"`, `"LEFT"`, `"RIGHT"`).
    key: &'static str,
}

const FACES: [FaceDef; 6] = [
    FaceDef { normal: (0, 0, 1), u_axis: Vec3::new(1.0, 0.0, 0.0), v_axis: Vec3::new(0.0, 1.0, 0.0), key: "TOP" },
    FaceDef { normal: (0, 0, -1), u_axis: Vec3::new(1.0, 0.0, 0.0), v_axis: Vec3::new(0.0, -1.0, 0.0), key: "BOTTOM" },
    FaceDef { normal: (0, -1, 0), u_axis: Vec3::new(1.0, 0.0, 0.0), v_axis: Vec3::new(0.0, 0.0, 1.0), key: "FRONT" },
    FaceDef { normal: (0, 1, 0), u_axis: Vec3::new(-1.0, 0.0, 0.0), v_axis: Vec3::new(0.0, 0.0, 1.0), key: "BACK" },
    FaceDef { normal: (-1, 0, 0), u_axis: Vec3::new(0.0, -1.0, 0.0), v_axis: Vec3::new(0.0, 0.0, 1.0), key: "LEFT" },
    FaceDef { normal: (1, 0, 0), u_axis: Vec3::new(0.0, 1.0, 0.0), v_axis: Vec3::new(0.0, 0.0, 1.0), key: "RIGHT" },
];

impl FaceDef {
    fn n_vec(self) -> Vec3 {
        Vec3::new(f64::from(self.normal.0), f64::from(self.normal.1), f64::from(self.normal.2))
    }

    /// Map surface parameters `(u, v)` in `[-1, 1]^2` to a 3D point on the unit cube `[-1, 1]^3`.
    fn point(self, u: f64, v: f64) -> Vec3 {
        self.n_vec() + self.u_axis * u + self.v_axis * v
    }

    /// Compute the [`CubeZone`] for a point `(u, v)` in `[-1, 1]^2` on this face.
    /// The outer margin (`|u| > C` or `|v| > C`) selects the adjacent edge or corner.
    fn zone_at(self, u: f64, v: f64) -> CubeZone {
        const BAND: f64 = 0.56;
        let du = if u < -BAND {
            -1.0
        } else if u > BAND {
            1.0
        } else {
            0.0
        };
        let dv = if v < -BAND {
            -1.0
        } else if v > BAND {
            1.0
        } else {
            0.0
        };
        let raw = self.n_vec() + self.u_axis * du + self.v_axis * dv;
        CubeZone::new(raw.x.round() as i8, raw.y.round() as i8, raw.z.round() as i8)
    }
}

/// Camera projection helper for the ViewCube widget.
#[derive(Clone, Copy)]
pub struct CubeProjector {
    pub center: Pos2,
    pub half_size: f32,
    pub orbit: Orbit3d,
}

impl CubeProjector {
    /// Create a projector centered at `center` with cube half-extent `half_size` in screen pixels.
    pub fn new(center: Pos2, half_size: f32, orbit: Orbit3d) -> Self {
        Self { center, half_size, orbit }
    }

    /// Outward camera direction vector in world space (pointing from target toward the viewer).
    pub fn view_dir(&self) -> Vec3 {
        let (sy, cy) = self.orbit.yaw.sin_cos();
        let (sp, cp) = self.orbit.pitch.sin_cos();
        Vec3::new(-sy * sp, -cy * sp, cp)
    }

    /// Project a 3D point in unit-cube coordinates (`[-1, 1]^3`) to screen coordinates (`Pos2`)
    /// and depth (`z_cam`, positive toward viewer).
    pub fn project_unit(&self, p: Vec3) -> (Pos2, f64) {
        let v = self.orbit.project_vec(p);
        let depth = p.dot(self.view_dir());
        let s = f64::from(self.half_size);
        let screen = pos2(self.center.x + (v.x * s) as f32, self.center.y - (v.y * s) as f32);
        (screen, depth)
    }

    /// Project a 3D point on the horizontal `z = 0` plane at pixel radius `r` around `self.center`.
    pub fn project_ring_point(&self, world_xy: Vec3, radius: f32) -> Pos2 {
        // When close to edge-on (`pitch ≈ ±90°`), keep a minimum vertical opening on the compass
        // ellipse so the compass ring remains legible and interactive, just like AutoCAD's ViewCube.
        let (sy, cy) = self.orbit.yaw.sin_cos();
        let cp = self.orbit.pitch.cos();
        let eff_cp = if cp.abs() < 0.28 { 0.28 * if cp >= 0.0 { 1.0 } else { -1.0 } } else { cp };
        let x1 = world_xy.x * cy - world_xy.y * sy;
        let y1 = (world_xy.x * sy + world_xy.y * cy) * eff_cp;
        let r = f64::from(radius);
        pos2(self.center.x + (x1 * r) as f32, self.center.y - (y1 * r) as f32)
    }

    /// Determine which [`CubeZone`] (face, edge, or corner) is under screen position `pos`.
    pub fn zone_at(&self, pos: Pos2) -> Option<CubeZone> {
        let view_dir = self.view_dir();
        let mut best: Option<(f64, CubeZone)> = None;
        for face in FACES {
            let ndot = face.n_vec().dot(view_dir);
            if ndot <= 1e-3 {
                continue;
            }
            let (p00, _) = self.project_unit(face.point(-1.0, -1.0));
            let (p10, _) = self.project_unit(face.point(1.0, -1.0));
            let (p11, _) = self.project_unit(face.point(1.0, 1.0));
            let (p01, _) = self.project_unit(face.point(-1.0, 1.0));
            if let Some((u, v)) = inverse_parallelogram(pos, p00, p10, p01)
                && (-1.0..=1.0).contains(&u)
                && (-1.0..=1.0).contains(&v)
            {
                let p3d = face.point(u, v);
                let depth = p3d.dot(view_dir);
                if best.is_none_or(|(best_d, _)| depth > best_d) {
                    let _ = p11;
                    best = Some((depth, face.zone_at(u, v)));
                }
            }
        }
        best.map(|(_, z)| z)
    }

    /// Check whether `pos` lies on the tilted 3D compass ring at `ring_radius`.
    pub fn on_compass_ring(&self, pos: Pos2, ring_radius: f32) -> bool {
        const STEPS: usize = 48;
        for i in 0..STEPS {
            let a0 = (i as f64) * std::f64::consts::TAU / (STEPS as f64);
            let a1 = ((i + 1) as f64) * std::f64::consts::TAU / (STEPS as f64);
            let p0 = self.project_ring_point(Vec3::new(a0.cos(), a0.sin(), 0.0), ring_radius);
            let p1 = self.project_ring_point(Vec3::new(a1.cos(), a1.sin(), 0.0), ring_radius);
            if dist_to_segment(pos, p0, p1) <= 9.0 {
                return true;
            }
        }
        false
    }

    /// Paint the 3D tilted compass ring (`N`, `E`, `S`, `W`), visible cube faces, dashed sub-zone
    /// boundaries, hovered face/edge/corner highlight, and face labels.
    pub fn paint(&self, painter: &egui::Painter, ring_radius: f32, hovered_ring: bool, hovered_zone: Option<CubeZone>) {
        self.paint_compass_ring(painter, ring_radius, hovered_ring);
        self.paint_cube(painter, hovered_zone);
    }

    fn paint_compass_ring(&self, painter: &egui::Painter, ring_radius: f32, hovered_ring: bool) {
        const STEPS: usize = 64;
        let ring_col = if hovered_ring { Color32::from_rgb(0x5c, 0x67, 0x7c) } else { Color32::from_rgb(0x48, 0x50, 0x5c) };
        let rim_col = Color32::from_rgb(0x5c, 0x65, 0x72);
        let mut pts_mid = Vec::with_capacity(STEPS);
        let mut pts_outer = Vec::with_capacity(STEPS);
        for i in 0..STEPS {
            let a = (i as f64) * std::f64::consts::TAU / (STEPS as f64);
            let dir = Vec3::new(a.cos(), a.sin(), 0.0);
            pts_mid.push(self.project_ring_point(dir, ring_radius));
            pts_outer.push(self.project_ring_point(dir, ring_radius + 4.5));
        }
        painter.add(Shape::Path(PathShape::closed_line(pts_mid, Stroke::new(9.0, ring_col))));
        painter.add(Shape::Path(PathShape::closed_line(pts_outer, Stroke::new(1.0, rim_col))));

        let font = egui::FontId::proportional(15.5);
        let lc = Color32::from_rgb(0xc8, 0xcc, 0xd2);
        for (dir, label) in [
            (Vec3::new(0.0, 1.0, 0.0), crate::tl!("N")),
            (Vec3::new(0.0, -1.0, 0.0), crate::tl!("S")),
            (Vec3::new(1.0, 0.0, 0.0), crate::tl!("E")),
            (Vec3::new(-1.0, 0.0, 0.0), crate::tl!("W")),
        ] {
            let pos = self.project_ring_point(dir, ring_radius);
            painter.text(pos, egui::Align2::CENTER_CENTER, label, font.clone(), lc);
        }
    }

    fn paint_cube(&self, painter: &egui::Painter, hovered_zone: Option<CubeZone>) {
        let view_dir = self.view_dir();
        let light_dir = (view_dir + Vec3::new(-0.25, -0.15, 0.45)).normalized();

        let mut visible: Vec<(f64, FaceDef)> = FACES
            .into_iter()
            .filter_map(|f| {
                let ndot = f.n_vec().dot(view_dir);
                (ndot > 1e-3).then_some((ndot, f))
            })
            .collect();
        // Painter's algorithm: back-to-front among the up to 3 visible faces.
        visible.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        for (ndot, face) in visible {
            let p00 = self.project_unit(face.point(-1.0, -1.0)).0;
            let p10 = self.project_unit(face.point(1.0, -1.0)).0;
            let p11 = self.project_unit(face.point(1.0, 1.0)).0;
            let p01 = self.project_unit(face.point(-1.0, 1.0)).0;

            let shade = (0.52 + 0.36 * face.n_vec().dot(light_dir).clamp(0.0, 1.0)) as f32;
            let base_r = (182.0 * shade).round() as u8;
            let base_g = (187.0 * shade).round() as u8;
            let base_b = (195.0 * shade).round() as u8;
            let face_zone = CubeZone::new(face.normal.0, face.normal.1, face.normal.2);
            let fill = if hovered_zone == Some(face_zone) {
                Color32::from_rgb(base_r.saturating_add(28), base_g.saturating_add(30), base_b.saturating_add(34))
            } else {
                Color32::from_rgb(base_r, base_g, base_b)
            };

            painter.add(Shape::convex_polygon(
                vec![p00, p10, p11, p01],
                fill,
                Stroke::new(1.1, Color32::from_rgb(0x4b, 0x50, 0x58)),
            ));

            // Draw subtle inner subdivision lines (AutoCAD-style edge/corner bands) when hovered.
            if hovered_zone.is_some() {
                self.paint_face_subdivisions(painter, face);
            }

            // Highlight hovered edge or corner sub-patch on this face.
            if let Some(hz) = hovered_zone
                && hz.rank() >= 2
            {
                self.paint_zone_highlight_on_face(painter, face, hz);
            }

            // Render the localized face label as a 3D surface-mapped mesh ("sticker") on the cube face
            // so it foreshortens, skews, and rotates upside-down with the face in 3D space.
            if ndot > 0.16 {
                self.paint_face_label(painter, face);
            }
        }
    }

    fn paint_face_label(&self, painter: &egui::Painter, face: FaceDef) {
        let label = match face.key {
            "TOP" => crate::tl!("TOP"),
            "BOTTOM" => crate::tl!("BOTTOM"),
            "FRONT" => crate::tl!("FRONT"),
            "BACK" => crate::tl!("BACK"),
            "LEFT" => crate::tl!("LEFT"),
            "RIGHT" => crate::tl!("RIGHT"),
            other => other,
        };
        let color = Color32::from_rgb(0x1f, 0x23, 0x28);
        // Layout the text at high resolution so its glyph quads are crisp, then map the galley's
        // centered 2D coordinates directly onto the face's `(u, v)` surface coordinates `[-1, 1]^2`.
        let galley = painter.layout_no_wrap(label.to_string(), egui::FontId::proportional(18.0), color);
        let size = galley.size();
        if size.x <= 1e-3 || size.y <= 1e-3 {
            return;
        }
        let cx = size.x * 0.5;
        let cy = size.y * 0.5;
        // Fit the label comfortably inside the cube face (`max_u = 1.48` out of `2.0` total face width),
        // while capping the vertical scale so short labels like "TOP" stay the same height as "BOTTOM".
        let scale_u = (1.48 / f64::from(size.x)).min(0.62 / f64::from(size.y));
        let scale_v = (0.64 / f64::from(size.y)).min(scale_u * 1.15);

        // Emit a faux-bold pass (two slight horizontal offsets in face `u`-space) so the label looks
        // bold and clear like AutoCAD's ViewCube face textures.
        let mut mesh = egui::Mesh::with_texture(egui::TextureId::default());
        for du_bold in [-0.018_f64, 0.018_f64, 0.0_f64] {
            for row in &galley.rows {
                let row_y = row.rect().min.y;
                for vtx in &row.visuals.mesh.vertices {
                    let gx = f64::from(vtx.pos.x - cx);
                    let gy = f64::from(row_y + vtx.pos.y - cy);
                    let u = gx * scale_u + du_bold;
                    // Galley +Y is downward; face +v is upward.
                    let v = -gy * scale_v;
                    let screen_pos = self.project_unit(face.point(u, v)).0;
                    mesh.vertices.push(egui::epaint::Vertex { pos: screen_pos, uv: vtx.uv, color });
                }
                let base = (mesh.vertices.len() - row.visuals.mesh.vertices.len()) as u32;
                for &idx in &row.visuals.mesh.indices {
                    mesh.indices.push(base + idx);
                }
            }
        }
        painter.add(Shape::mesh(mesh));
    }

    fn paint_face_subdivisions(&self, painter: &egui::Painter, face: FaceDef) {
        const B: f64 = 0.56;
        let st = Stroke::new(0.7, Color32::from_rgba_unmultiplied(0x52, 0x57, 0x60, 110));
        for c in [-B, B] {
            let a = self.project_unit(face.point(c, -1.0)).0;
            let b = self.project_unit(face.point(c, 1.0)).0;
            painter.line_segment([a, b], st);
            let c0 = self.project_unit(face.point(-1.0, c)).0;
            let c1 = self.project_unit(face.point(1.0, c)).0;
            painter.line_segment([c0, c1], st);
        }
    }

    fn paint_zone_highlight_on_face(&self, painter: &egui::Painter, face: FaceDef, zone: CubeZone) {
        const B: f64 = 0.56;
        let zv = Vec3::new(f64::from(zone.nx), f64::from(zone.ny), f64::from(zone.nz));
        // Only highlight if this face is one of the faces participating in `zone`.
        if (face.n_vec().dot(zv) - 1.0).abs() > 1e-4 {
            return;
        }
        let du = face.u_axis.dot(zv).round() as i8;
        let dv = face.v_axis.dot(zv).round() as i8;
        let (u0, u1) = match du {
            -1 => (-1.0, -B),
            0 => (-B, B),
            1 => (B, 1.0),
            _ => return,
        };
        let (v0, v1) = match dv {
            -1 => (-1.0, -B),
            0 => (-B, B),
            1 => (B, 1.0),
            _ => return,
        };
        let q00 = self.project_unit(face.point(u0, v0)).0;
        let q10 = self.project_unit(face.point(u1, v0)).0;
        let q11 = self.project_unit(face.point(u1, v1)).0;
        let q01 = self.project_unit(face.point(u0, v1)).0;
        painter.add(Shape::convex_polygon(
            vec![q00, q10, q11, q01],
            Color32::from_rgba_unmultiplied(0x6e, 0xa8, 0xfe, 155),
            Stroke::new(1.0, Color32::from_rgb(0x3b, 0x82, 0xf6)),
        ));
    }
}

/// Solve for `(u, v)` in `[-1, 1]^2` given a 2D point `p` inside the projected parallelogram
/// with corners `p00 = (-1, -1)`, `p10 = (1, -1)`, and `p01 = (-1, 1)`.
fn inverse_parallelogram(p: Pos2, p00: Pos2, p10: Pos2, p01: Pos2) -> Option<(f64, f64)> {
    let eu = p10 - p00;
    let ev = p01 - p00;
    let det = f64::from(eu.x) * f64::from(ev.y) - f64::from(eu.y) * f64::from(ev.x);
    if det.abs() < 1e-4 {
        return None;
    }
    let rel = p - p00;
    let s = (f64::from(rel.x) * f64::from(ev.y) - f64::from(rel.y) * f64::from(ev.x)) / det;
    let t = (f64::from(eu.x) * f64::from(rel.y) - f64::from(eu.y) * f64::from(rel.x)) / det;
    Some((s * 2.0 - 1.0, t * 2.0 - 1.0))
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= 1e-6 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + vec2(ab.x * t, ab.y * t))
}
