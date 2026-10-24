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

            // Draw AutoCAD-style dashed subdivision lines along the edges/corners of visible faces.
            self.paint_face_subdivisions(painter, face);

            // Highlight hovered edge or corner sub-patch on this face.
            if let Some(hz) = hovered_zone
                && hz.rank() >= 2
            {
                self.paint_zone_highlight_on_face(painter, face, hz);
            }

            // Render the localized face label using 3D surface-projected vector strokes so it is
            // always centered, bold, skewed with the face parallelogram, and upside-down from behind.
            if ndot > 0.16 {
                self.paint_face_label(painter, face, ndot);
            }
        }
    }

    fn paint_face_label(&self, painter: &egui::Painter, face: FaceDef, ndot: f64) {
        let raw_label = match face.key {
            "TOP" => crate::tl!("TOP"),
            "BOTTOM" => crate::tl!("BOTTOM"),
            "FRONT" => crate::tl!("FRONT"),
            "BACK" => crate::tl!("BACK"),
            "LEFT" => crate::tl!("LEFT"),
            "RIGHT" => crate::tl!("RIGHT"),
            other => other,
        };
        let label = raw_label.to_uppercase();
        let chars: Vec<char> = label.chars().filter(|c| !c.is_whitespace()).collect();
        if chars.is_empty() {
            return;
        }

        // Character metrics in normalized glyph coordinates (`x in [0, 1]`, `y in [0, 1]`).
        let char_w = 1.0_f64;
        let gap = 0.34_f64;
        let total_w = (chars.len() as f64) * char_w + ((chars.len().saturating_sub(1)) as f64) * gap;
        // Fit within `[-0.64, 0.64]` in face `u`-space (inside the `[-0.56, 0.56]` inner band when possible)
        // and cap vertical height to `0.42` in `v`-space so short words ("TOP") match longer words ("RIGHT").
        let scale_u = (1.16 / total_w).min(0.28);
        let scale_v = (scale_u * 1.45).min(0.42);
        let stroke_px = (1.85 * (ndot as f32).powf(0.25)).clamp(1.35, 2.0);
        let stroke = Stroke::new(stroke_px, Color32::from_rgb(0x22, 0x26, 0x2c));

        let start_x = -0.5 * total_w;
        for (i, ch) in chars.into_iter().enumerate() {
            let x0 = start_x + (i as f64) * (char_w + gap);
            for poly in glyph_polylines(ch) {
                let pts: Vec<Pos2> = poly
                    .iter()
                    .map(|&(gx, gy)| {
                        let u = (x0 + f64::from(gx) * char_w) * scale_u;
                        let v = (f64::from(gy) - 0.5) * scale_v;
                        self.project_unit(face.point(u, v)).0
                    })
                    .collect();
                if pts.len() == 2 {
                    painter.line_segment([pts[0], pts[1]], stroke);
                } else if pts.len() > 2 {
                    painter.add(Shape::line(pts, stroke));
                }
            }
        }
    }

    fn paint_face_subdivisions(&self, painter: &egui::Painter, face: FaceDef) {
        const B: f64 = 0.56;
        const SEGMENTS: usize = 7;
        let st = Stroke::new(0.85, Color32::from_rgba_unmultiplied(0x38, 0x3d, 0x45, 135));
        for c in [-B, B] {
            // Dashed line along v from -1 to 1 at u = c, and along u from -1 to 1 at v = c.
            for k in (0..SEGMENTS).step_by(2) {
                let t0 = -1.0 + 2.0 * (k as f64) / (SEGMENTS as f64);
                let t1 = -1.0 + 2.0 * ((k + 1) as f64) / (SEGMENTS as f64);
                let a = self.project_unit(face.point(c, t0)).0;
                let b = self.project_unit(face.point(c, t1)).0;
                painter.line_segment([a, b], st);
                let c0 = self.project_unit(face.point(t0, c)).0;
                let c1 = self.project_unit(face.point(t1, c)).0;
                painter.line_segment([c0, c1], st);
            }
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

/// Normalized vector-stroke polylines (`x in [0, 1]`, `y in [0, 1]` from bottom to top) for uppercase
/// characters on the ViewCube faces.
fn glyph_polylines(ch: char) -> &'static [&'static [(f32, f32)]] {
    match ch {
        'A' | 'Ä' | 'Á' | 'À' | 'Â' => &[&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)], &[(0.18, 0.36), (0.82, 0.36)]],
        'B' => &[
            &[(0.0, 0.0), (0.0, 1.0), (0.72, 1.0), (0.95, 0.85), (0.95, 0.62), (0.72, 0.5), (0.0, 0.5)],
            &[(0.72, 0.5), (1.0, 0.38), (1.0, 0.15), (0.75, 0.0), (0.0, 0.0)],
        ],
        'C' | 'Ç' | 'Č' => &[&[(0.96, 0.82), (0.72, 1.0), (0.28, 1.0), (0.0, 0.75), (0.0, 0.25), (0.28, 0.0), (0.72, 0.0), (0.96, 0.18)]],
        'D' => &[&[(0.0, 0.0), (0.0, 1.0), (0.65, 1.0), (1.0, 0.72), (1.0, 0.28), (0.65, 0.0), (0.0, 0.0)]],
        'E' | 'É' | 'È' | 'Ê' | 'Ë' => &[&[(0.95, 1.0), (0.0, 1.0), (0.0, 0.0), (0.95, 0.0)], &[(0.0, 0.5), (0.78, 0.5)]],
        'F' => &[&[(0.0, 0.0), (0.0, 1.0), (0.95, 1.0)], &[(0.0, 0.5), (0.78, 0.5)]],
        'G' => &[&[
            (0.96, 0.82),
            (0.72, 1.0),
            (0.28, 1.0),
            (0.0, 0.75),
            (0.0, 0.25),
            (0.28, 0.0),
            (0.78, 0.0),
            (1.0, 0.22),
            (1.0, 0.46),
            (0.52, 0.46),
        ]],
        'H' => &[&[(0.0, 0.0), (0.0, 1.0)], &[(1.0, 0.0), (1.0, 1.0)], &[(0.0, 0.5), (1.0, 0.5)]],
        'I' | 'Í' | 'Ì' | 'Î' => &[&[(0.18, 1.0), (0.82, 1.0)], &[(0.5, 0.0), (0.5, 1.0)], &[(0.18, 0.0), (0.82, 0.0)]],
        'J' => &[&[(0.2, 1.0), (0.9, 1.0)], &[(0.65, 1.0), (0.65, 0.22), (0.42, 0.0), (0.15, 0.0), (0.0, 0.2)]],
        'K' => &[&[(0.0, 0.0), (0.0, 1.0)], &[(0.96, 1.0), (0.0, 0.45)], &[(0.28, 0.6), (1.0, 0.0)]],
        'L' => &[&[(0.0, 1.0), (0.0, 0.0), (0.95, 0.0)]],
        'M' => &[&[(0.0, 0.0), (0.0, 1.0), (0.5, 0.35), (1.0, 1.0), (1.0, 0.0)]],
        'N' | 'Ñ' => &[&[(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)]],
        'O' | 'Ö' | 'Ó' | 'Ò' | 'Ô' => &[&[
            (0.28, 1.0),
            (0.72, 1.0),
            (1.0, 0.74),
            (1.0, 0.26),
            (0.72, 0.0),
            (0.28, 0.0),
            (0.0, 0.26),
            (0.0, 0.74),
            (0.28, 1.0),
        ]],
        'P' => &[&[(0.0, 0.0), (0.0, 1.0), (0.72, 1.0), (1.0, 0.82), (1.0, 0.62), (0.72, 0.45), (0.0, 0.45)]],
        'Q' => &[
            &[
                (0.28, 1.0),
                (0.72, 1.0),
                (1.0, 0.74),
                (1.0, 0.26),
                (0.72, 0.0),
                (0.28, 0.0),
                (0.0, 0.26),
                (0.0, 0.74),
                (0.28, 1.0),
            ],
            &[(0.55, 0.28), (1.0, 0.0)],
        ],
        'R' | 'Ř' => &[
            &[(0.0, 0.0), (0.0, 1.0), (0.72, 1.0), (1.0, 0.82), (1.0, 0.62), (0.72, 0.48), (0.0, 0.48)],
            &[(0.52, 0.48), (1.0, 0.0)],
        ],
        'S' | 'Š' => &[&[
            (0.95, 0.84),
            (0.72, 1.0),
            (0.25, 1.0),
            (0.0, 0.82),
            (0.0, 0.64),
            (0.25, 0.52),
            (0.75, 0.46),
            (1.0, 0.32),
            (1.0, 0.16),
            (0.75, 0.0),
            (0.22, 0.0),
            (0.0, 0.16),
        ]],
        'T' => &[&[(0.0, 1.0), (1.0, 1.0)], &[(0.5, 0.0), (0.5, 1.0)]],
        'U' | 'Ü' | 'Ú' | 'Ù' | 'Û' => &[&[(0.0, 1.0), (0.0, 0.24), (0.28, 0.0), (0.72, 0.0), (1.0, 0.24), (1.0, 1.0)]],
        'V' => &[&[(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)]],
        'W' => &[&[(0.0, 1.0), (0.24, 0.0), (0.5, 0.65), (0.76, 0.0), (1.0, 1.0)]],
        'X' => &[&[(0.0, 0.0), (1.0, 1.0)], &[(0.0, 1.0), (1.0, 0.0)]],
        'Y' => &[&[(0.0, 1.0), (0.5, 0.5), (1.0, 1.0)], &[(0.5, 0.5), (0.5, 0.0)]],
        'Z' | 'Ž' => &[&[(0.0, 1.0), (1.0, 1.0), (0.0, 0.0), (1.0, 0.0)]],
        _ => &[&[(0.15, 0.15), (0.85, 0.15), (0.85, 0.85), (0.15, 0.85), (0.15, 0.15)]],
    }
}

