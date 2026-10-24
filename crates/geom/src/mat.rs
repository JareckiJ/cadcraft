use serde::{Deserialize, Serialize};

use crate::{Vec2, Vec3};

/// A 2D affine transform `[a c e; b d f; 0 0 1]` (column vectors, like SVG/PDF).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mat3 {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Mat3 {
    fn default() -> Self {
        Mat3::IDENTITY
    }
}

impl Mat3 {
    pub const IDENTITY: Mat3 = Mat3 { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(v: Vec2) -> Mat3 {
        Mat3 { e: v.x, f: v.y, ..Mat3::IDENTITY }
    }
    pub fn scale(sx: f64, sy: f64) -> Mat3 {
        Mat3 { a: sx, d: sy, ..Mat3::IDENTITY }
    }
    pub fn rotate(angle: f64) -> Mat3 {
        let (s, c) = angle.sin_cos();
        Mat3 { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }
    /// Scale about `base`.
    pub fn scale_about(base: Vec2, s: f64) -> Mat3 {
        Mat3::translate(base).then_before(Mat3::scale(s, s)).then_before(Mat3::translate(-base))
    }
    pub fn rotate_about(base: Vec2, angle: f64) -> Mat3 {
        Mat3::translate(base).then_before(Mat3::rotate(angle)).then_before(Mat3::translate(-base))
    }
    /// Reflection across the line through `p1`, `p2`.
    pub fn mirror(p1: Vec2, p2: Vec2) -> Mat3 {
        let d = (p2 - p1).normalized();
        if d == Vec2::ZERO {
            return Mat3::IDENTITY;
        }
        let (c2, s2) = (d.x * d.x - d.y * d.y, 2.0 * d.x * d.y);
        let r = Mat3 { a: c2, b: s2, c: s2, d: -c2, e: 0.0, f: 0.0 };
        Mat3::translate(p1).then_before(r).then_before(Mat3::translate(-p1))
    }
    /// `self * o`: apply `o` first, then `self`.
    pub fn then_before(self, o: Mat3) -> Mat3 {
        Mat3 {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }
    /// Apply `self` first, then `o`.
    pub fn then(self, o: Mat3) -> Mat3 {
        o.then_before(self)
    }
    pub fn apply(&self, p: Vec2) -> Vec2 {
        Vec2::new(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }
    pub fn apply_vec(&self, v: Vec2) -> Vec2 {
        Vec2::new(self.a * v.x + self.c * v.y, self.b * v.x + self.d * v.y)
    }
    pub fn det(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }
    pub fn inverse(&self) -> Option<Mat3> {
        let det = self.det();
        if det.abs() < 1e-300 || !det.is_finite() {
            return None;
        }
        let id = 1.0 / det;
        let a = self.d * id;
        let b = -self.b * id;
        let c = -self.c * id;
        let d = self.a * id;
        let e = -(a * self.e + c * self.f);
        let f = -(b * self.e + d * self.f);
        Some(Mat3 { a, b, c, d, e, f })
    }
    /// Uniform scale factor (geometric mean of the axis scales).
    pub fn scale_factor(&self) -> f64 {
        self.det().abs().sqrt()
    }
    /// Rotation angle of the transformed X axis.
    pub fn rotation(&self) -> f64 {
        crate::norm_angle(self.b.atan2(self.a))
    }
    pub fn is_mirroring(&self) -> bool {
        self.det() < 0.0
    }
}

/// A 4x4 transform, row-major, for 3D views and block transforms with elevation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mat4 {
    pub m: [[f64; 4]; 4],
}

impl Default for Mat4 {
    fn default() -> Self {
        Mat4::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] };
    pub fn mul(&self, o: &Mat4) -> Mat4 {
        let mut r = [[0.0; 4]; 4];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.m[i][k] * o.m[k][j]).sum();
            }
        }
        Mat4 { m: r }
    }
    pub fn apply(&self, p: Vec3) -> Vec3 {
        let m = &self.m;
        let w = m[3][0] * p.x + m[3][1] * p.y + m[3][2] * p.z + m[3][3];
        let w = if w.abs() < 1e-300 { 1.0 } else { w };
        Vec3::new(
            (m[0][0] * p.x + m[0][1] * p.y + m[0][2] * p.z + m[0][3]) / w,
            (m[1][0] * p.x + m[1][1] * p.y + m[1][2] * p.z + m[1][3]) / w,
            (m[2][0] * p.x + m[2][1] * p.y + m[2][2] * p.z + m[2][3]) / w,
        )
    }
    pub fn from_mat3(t: &Mat3) -> Mat4 {
        Mat4 { m: [[t.a, t.c, 0.0, t.e], [t.b, t.d, 0.0, t.f], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }
    /// The "arbitrary axis algorithm" frame for an extrusion direction (DXF OCS → WCS).
    pub fn ocs(normal: Vec3) -> Mat4 {
        let n = normal.normalized();
        if n == Vec3::ZERO || (n.x.abs() < 1e-12 && n.y.abs() < 1e-12 && n.z > 0.0) {
            return Mat4::IDENTITY;
        }
        let ax = if n.x.abs() < 1.0 / 64.0 && n.y.abs() < 1.0 / 64.0 { Vec3::new(0.0, 1.0, 0.0).cross(n) } else { Vec3::Z.cross(n) }.normalized();
        let ay = n.cross(ax).normalized();
        Mat4 { m: [[ax.x, ay.x, n.x, 0.0], [ax.y, ay.y, n.y, 0.0], [ax.z, ay.z, n.z, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }

    /// Translation by a 3D displacement vector.
    pub fn translate(v: Vec3) -> Mat4 {
        Mat4 { m: [[1.0, 0.0, 0.0, v.x], [0.0, 1.0, 0.0, v.y], [0.0, 0.0, 1.0, v.z], [0.0, 0.0, 0.0, 1.0]] }
    }

    /// Non-uniform scale along X, Y, Z axes.
    pub fn scale(s: Vec3) -> Mat4 {
        Mat4 { m: [[s.x, 0.0, 0.0, 0.0], [0.0, s.y, 0.0, 0.0], [0.0, 0.0, s.z, 0.0], [0.0, 0.0, 0.0, 1.0]] }
    }

    /// Scale about a 3D base point.
    pub fn scale_about(base: Vec3, s: Vec3) -> Mat4 {
        Mat4::translate(base).mul(&Mat4::scale(s)).mul(&Mat4::translate(-base))
    }

    /// Rotation around an arbitrary axis in 3D (specified by base origin and axis direction vector).
    pub fn rotate_axis(origin: Vec3, axis_dir: Vec3, angle_rad: f64) -> Mat4 {
        let u = axis_dir.normalized();
        if u.len() < 1e-12 {
            return Mat4::IDENTITY;
        }
        let (s, c) = angle_rad.sin_cos();
        let c1 = 1.0 - c;
        let r = Mat4 {
            m: [
                [c + u.x * u.x * c1, u.x * u.y * c1 - u.z * s, u.x * u.z * c1 + u.y * s, 0.0],
                [u.y * u.x * c1 + u.z * s, c + u.y * u.y * c1, u.y * u.z * c1 - u.x * s, 0.0],
                [u.z * u.x * c1 - u.y * s, u.z * u.y * c1 + u.x * s, c + u.z * u.z * c1, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        Mat4::translate(origin).mul(&r).mul(&Mat4::translate(-origin))
    }

    /// Reflection (mirror) across a 3D plane defined by a point on the plane and its normal vector.
    pub fn mirror_plane(plane_pt: Vec3, normal: Vec3) -> Mat4 {
        let n = normal.normalized();
        if n.len() < 1e-12 {
            return Mat4::IDENTITY;
        }
        let ref_mat = Mat4 {
            m: [
                [1.0 - 2.0 * n.x * n.x, -2.0 * n.x * n.y, -2.0 * n.x * n.z, 0.0],
                [-2.0 * n.y * n.x, 1.0 - 2.0 * n.y * n.y, -2.0 * n.y * n.z, 0.0],
                [-2.0 * n.z * n.x, -2.0 * n.z * n.y, 1.0 - 2.0 * n.z * n.z, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        Mat4::translate(plane_pt).mul(&ref_mat).mul(&Mat4::translate(-plane_pt))
    }

    /// 3D Alignment from 3 source points to 3 destination points.
    /// Point 1 defines origin, point 2 defines X axis, point 3 defines XY plane.
    pub fn align_3point(src: [Vec3; 3], dst: [Vec3; 3]) -> Option<Mat4> {
        let sx = (src[1] - src[0]).normalized();
        let dx = (dst[1] - dst[0]).normalized();
        if sx.len() < 1e-12 || dx.len() < 1e-12 {
            return Some(Mat4::translate(dst[0] - src[0]));
        }

        let sv = src[2] - src[0];
        let mut sz = sx.cross(sv).normalized();
        if sz.len() < 1e-12 {
            sz = if sx.x.abs() < 0.8 { Vec3::X.cross(sx) } else { Vec3::Y.cross(sx) }.normalized();
        }
        let sy = sz.cross(sx).normalized();

        let dv = dst[2] - dst[0];
        let mut dz = dx.cross(dv).normalized();
        if dz.len() < 1e-12 {
            dz = if dx.x.abs() < 0.8 { Vec3::X.cross(dx) } else { Vec3::Y.cross(dx) }.normalized();
        }
        let dy = dz.cross(dx).normalized();

        // R_src: columns are sx, sy, sz. R_src^T: rows are sx, sy, sz.
        // R_dst: columns are dx, dy, dz.
        // R = R_dst * R_src^T
        let r = [
            [dx.x * sx.x + dy.x * sy.x + dz.x * sz.x, dx.x * sx.y + dy.x * sy.y + dz.x * sz.y, dx.x * sx.z + dy.x * sy.z + dz.x * sz.z],
            [dx.y * sx.x + dy.y * sy.x + dz.y * sz.x, dx.y * sx.y + dy.y * sy.y + dz.y * sz.y, dx.y * sx.z + dy.y * sy.z + dz.y * sz.z],
            [dx.z * sx.x + dy.z * sy.x + dz.z * sz.x, dx.z * sx.y + dy.z * sy.y + dz.z * sz.y, dx.z * sx.z + dy.z * sy.z + dz.z * sz.z],
        ];

        // Translation: dst[0] - R * src[0]
        let s0 = src[0];
        let rs0 = Vec3::new(
            r[0][0] * s0.x + r[0][1] * s0.y + r[0][2] * s0.z,
            r[1][0] * s0.x + r[1][1] * s0.y + r[1][2] * s0.z,
            r[2][0] * s0.x + r[2][1] * s0.y + r[2][2] * s0.z,
        );
        let t = dst[0] - rs0;

        Some(Mat4 { m: [[r[0][0], r[0][1], r[0][2], t.x], [r[1][0], r[1][1], r[1][2], t.y], [r[2][0], r[2][1], r[2][2], t.z], [0.0, 0.0, 0.0, 1.0]] })
    }

    /// 2-point alignment (with optional uniform scaling based on distance between points).
    pub fn align_2point(src: [Vec3; 2], dst: [Vec3; 2], scale: bool) -> Option<Mat4> {
        let vs = src[1] - src[0];
        let vd = dst[1] - dst[0];
        let ls = vs.len();
        let ld = vd.len();
        if ls < 1e-12 || ld < 1e-12 {
            return Some(Mat4::translate(dst[0] - src[0]));
        }

        let us = vs / ls;
        let ud = vd / ld;
        let dot = (us.dot(ud)).clamp(-1.0, 1.0);
        let cross = us.cross(ud);
        let angle = dot.acos();

        let rot = if cross.len() > 1e-12 {
            Mat4::rotate_axis(Vec3::ZERO, cross.normalized(), angle)
        } else if dot < 0.0 {
            let axis = if us.x.abs() < 0.8 { Vec3::X.cross(us) } else { Vec3::Y.cross(us) }.normalized();
            Mat4::rotate_axis(Vec3::ZERO, axis, std::f64::consts::PI)
        } else {
            Mat4::IDENTITY
        };

        let k = if scale { ld / ls } else { 1.0 };
        let sc = Mat4::scale(Vec3::new(k, k, k));

        // Result: T(dst[0]) * Scale * Rot * T(-src[0])
        Some(Mat4::translate(dst[0]).mul(&sc).mul(&rot).mul(&Mat4::translate(-src[0])))
    }
}
