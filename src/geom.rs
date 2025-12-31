//! 3D geometry utilities

use crate::pipeline::prelude::Data;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::ops::{Add, Mul, Neg, Sub};
use std::sync::Arc;

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec3(pub [f64; 3]);
impl Vec3 {
    pub const ZERO: Self = Self([0.0, 0.0, 0.0]);
    pub const fn x(self) -> f64 {
        self.0[0]
    }
    pub const fn y(self) -> f64 {
        self.0[1]
    }
    pub const fn z(self) -> f64 {
        self.0[2]
    }
    pub const fn neg(self) -> Self {
        let Self([x1, y1, z1]) = self;
        Self([-x1, -y1, -z1])
    }
    pub const fn add(self, other: Self) -> Self {
        let Self([x1, y1, z1]) = self;
        let Self([x2, y2, z2]) = other;
        Self([x1 + x2, y1 + y2, z1 + z2])
    }
    pub const fn sub(self, other: Self) -> Self {
        let Self([x1, y1, z1]) = self;
        let Self([x2, y2, z2]) = other;
        Self([x1 - x2, y1 - y2, z1 - z2])
    }
    pub const fn dot(self, other: Self) -> f64 {
        let Self([x1, y1, z1]) = self;
        let Self([x2, y2, z2]) = other;
        x1 * x2 + y1 * y2 + z1 * z2
    }
    pub const fn abs_squared(self) -> f64 {
        self.dot(self)
    }
    pub fn abs(self) -> f64 {
        self.abs_squared().sqrt()
    }
}
impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, rhs: Self) -> Self::Output {
        self.add(rhs)
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, rhs: Self) -> Self::Output {
        self.sub(rhs)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Self::Output {
        self.neg()
    }
}
impl Data for Vec3 {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["x", "y", "z"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "x" => Some(Cow::Borrowed(&self.0[0])),
            "y" => Some(Cow::Borrowed(&self.0[1])),
            "z" => Some(Cow::Borrowed(&self.0[2])),
            _ => None,
        }
    }
}

/// A 3x3 double-precision matrix.
///
/// This is stored internally in row-major order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mat3(pub [f64; 9]);
impl Mat3 {
    /// The identity matrix.
    pub const EYE: Self = Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    /// Strip a scaling from the matrix to get the normalized matrix and its scale.
    pub fn without_scale(&self) -> (Self, Vec3) {
        let Mat3([m0, m1, m2, m3, m4, m5, m6, m7, m8]) = *self;
        let sx = (m0 * m0 + m1 * m1 + m2 * m2).sqrt();
        let sy = (m3 * m3 + m4 * m4 + m5 * m5).sqrt();
        let sz = (m6 * m6 + m7 * m7 + m8 * m8).sqrt();
        let r = Self([
            m0 / sx,
            m1 / sx,
            m2 / sx,
            m3 / sy,
            m4 / sy,
            m5 / sy,
            m6 / sz,
            m7 / sz,
            m8 / sz,
        ]);
        (r, Vec3([sx, sy, sz]))
    }
    /// Remove the scale from this matrix.
    pub fn remove_scale(&mut self) -> Vec3 {
        let Mat3([m0, m1, m2, m3, m4, m5, m6, m7, m8]) = self;
        let sx = (*m0 * *m0 + *m1 * *m1 + *m2 * *m2).sqrt();
        let sy = (*m3 * *m3 + *m4 * *m4 + *m5 * *m5).sqrt();
        let sz = (*m6 * *m6 + *m7 * *m7 + *m8 * *m8).sqrt();
        *m0 /= sx;
        *m1 /= sx;
        *m2 /= sx;
        *m3 /= sy;
        *m4 /= sy;
        *m5 /= sy;
        *m6 /= sz;
        *m7 /= sz;
        *m8 /= sz;
        Vec3([sx, sy, sz])
    }
    /// Convert this matrix to a quaternion.
    ///
    /// This assumes that the matrix is orthonormal.
    pub fn to_quat(&self) -> Quat {
        let Mat3([m0, m1, m2, m3, m4, m5, m6, m7, m8]) = *self;
        let trace = m0 + m4 + m8;
        if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            Quat([(m7 - m5) / s, (m2 - m6) / s, (m3 - m1) / s, 0.25 * s])
        } else if m0 > m4 && m0 > m8 {
            let s = (1.0 + m0 - m4 - m8).sqrt() * 2.0;
            Quat([0.25 * s, (m1 + m3) / s, (m2 + m6) / s, (m7 - m5) / s])
        } else if m4 > m8 {
            let s = (1.0 + m4 - m0 - m8).sqrt() * 2.0;
            Quat([(m1 + m3) / s, 0.25 * s, (m5 + m7) / s, (m2 - m6) / s])
        } else {
            let s = (1.0 + m8 - m0 - m4).sqrt() * 2.0;
            Quat([(m2 + m6) / s, (m5 + m7) / s, 0.25 * s, (m3 - m1) / s])
        }
    }
    /// Convert this matrix to a set of XYZ Euler angles.
    ///
    /// This assumes that the matrix is orthonormal.
    pub fn to_euler(&self) -> EulerXYZ {
        let Mat3([m0, m1, m2, m3, m4, m5, _m6, _m7, m8]) = *self;
        let pitch = m2.asin();
        let cos_pitch = pitch.cos();
        let (roll, yaw) = if cos_pitch.abs() > 1e-6 {
            ((-m5).atan2(m8), (-m1).atan2(m0))
        } else {
            (m3.atan2(m4), 0.0)
        };
        EulerXYZ([roll, pitch, yaw])
    }
    /// Multiply this matrix by another matrix.
    pub fn mul_mat(&self, rhs: &Self) -> Self {
        let mut out = [0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                let mut sum = 0.0;
                for k in 0..3 {
                    sum += self.0[i * 3 + k] * rhs.0[k * 3 + j];
                }
                out[i * 3 + j] = sum;
            }
        }
        Self(out)
    }
    /// Multiply this matrix by a column vector.
    pub fn mul_vec(&self, rhs: Vec3) -> Vec3 {
        let Vec3([x, y, z]) = rhs;
        Vec3(std::array::from_fn(|i| {
            x * self.0[i * 3] + y * self.0[i * 3 + 1] + z * self.0[i * 3 + 2]
        }))
    }
    /// Find the determinant of the matrix.
    pub fn det(&self) -> f64 {
        self.0[0] * (self.0[4] * self.0[8] - self.0[5] * self.0[7])
            - self.0[1] * (self.0[3] * self.0[8] - self.0[5] * self.0[6])
            + self.0[2] * (self.0[3] * self.0[7] - self.0[4] * self.0[6])
    }
    /// Invert the matrix.
    ///
    /// Panics in debug mode if the matrix is singular.
    pub fn inverse(&self) -> Self {
        let det = self.det();
        let [m0, m1, m2, m3, m4, m5, m6, m7, m8] = self.0;
        debug_assert!(det.abs() > 1e-6, "attempted to invert a singular matrix");
        let inv_det = det.recip();
        Self([
            (m4 * m8 - m5 * m7) * inv_det,
            (m2 * m7 - m1 * m8) * inv_det,
            (m1 * m5 - m2 * m4) * inv_det,
            (m5 * m6 - m3 * m8) * inv_det,
            (m0 * m8 - m2 * m6) * inv_det,
            (m2 * m3 - m0 * m5) * inv_det,
            (m3 * m7 - m4 * m6) * inv_det,
            (m1 * m6 - m0 * m7) * inv_det,
            (m0 * m4 - m1 * m3) * inv_det,
        ])
    }
}
impl Mul for Mat3 {
    type Output = Mat3;
    fn mul(self, rhs: Self) -> Self::Output {
        self.mul_mat(&rhs)
    }
}
impl Mul for &Mat3 {
    type Output = Mat3;
    fn mul(self, rhs: Self) -> Self::Output {
        self.mul_mat(rhs)
    }
}
impl Mul<Vec3> for Mat3 {
    type Output = Vec3;
    fn mul(self, rhs: Vec3) -> Self::Output {
        self.mul_vec(rhs)
    }
}
impl Data for Mat3 {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["quat", "euler"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "quat" => Some(Cow::Owned(Arc::new(self.to_quat()) as _)),
            "euler" => Some(Cow::Owned(Arc::new(self.to_euler()) as _)),
            _ => None,
        }
    }
}

/// A quaternion, stored in XYZW order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quat(pub [f64; 4]);
impl Quat {
    pub const fn x(self) -> f64 {
        self.0[0]
    }
    pub const fn y(self) -> f64 {
        self.0[1]
    }
    pub const fn z(self) -> f64 {
        self.0[2]
    }
    pub const fn w(self) -> f64 {
        self.0[3]
    }
}
impl Data for Quat {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["x", "y", "z", "w"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "x" => Some(Cow::Borrowed(&self.0[0])),
            "y" => Some(Cow::Borrowed(&self.0[1])),
            "z" => Some(Cow::Borrowed(&self.0[2])),
            "w" => Some(Cow::Borrowed(&self.0[3])),
            _ => None,
        }
    }
}

/// XYZ intrinsic euler angles (roll, pitch, yaw).
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EulerXYZ(pub [f64; 3]);
impl EulerXYZ {
    #[doc(alias = "roll")]
    pub const fn x(self) -> f64 {
        self.0[0]
    }
    #[doc(alias = "pitch")]
    pub const fn y(self) -> f64 {
        self.0[1]
    }
    #[doc(alias = "yaw")]
    pub const fn z(self) -> f64 {
        self.0[2]
    }
}
impl Data for EulerXYZ {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["x", "y", "z"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "x" => Some(Cow::Borrowed(&self.0[0])),
            "y" => Some(Cow::Borrowed(&self.0[1])),
            "z" => Some(Cow::Borrowed(&self.0[2])),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::*;

    const EPSILON: f64 = 1e-10;

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
        println!("approx_eq: {a:?} vs {b:?}");
        approx_eq(a.x(), b.x()) && approx_eq(a.y(), b.y()) && approx_eq(a.z(), b.z())
    }

    fn mat3_approx_eq(a: &Mat3, b: &Mat3) -> bool {
        println!("approx_eq: {a:?} vs {b:?}");
        a.0.iter().zip(b.0.iter()).all(|(x, y)| approx_eq(*x, *y))
    }

    fn quat_approx_eq(a: Quat, b: Quat) -> bool {
        println!("approx_eq: {a:?} vs {b:?}");
        approx_eq(a.x(), b.x())
            && approx_eq(a.y(), b.y())
            && approx_eq(a.z(), b.z())
            && approx_eq(a.w(), b.w())
    }

    fn euler_approx_eq(a: EulerXYZ, b: EulerXYZ) -> bool {
        println!("approx_eq: {a:?} vs {b:?}");
        approx_eq(a.x().rem_euclid(TAU), b.x().rem_euclid(TAU))
            && approx_eq(a.y().rem_euclid(TAU), b.y().rem_euclid(TAU))
            && approx_eq(a.z().rem_euclid(TAU), b.z().rem_euclid(TAU))
    }

    #[test]
    fn test_vec3_operations() {
        let v1 = Vec3([1.0, 2.0, 3.0]);
        let v2 = Vec3([4.0, 5.0, 6.0]);

        // Addition
        let sum = v1 + v2;
        assert_eq!(sum, Vec3([5.0, 7.0, 9.0]));

        // Subtraction
        let diff = v2 - v1;
        assert_eq!(diff, Vec3([3.0, 3.0, 3.0]));

        // Negation
        let neg = -v1;
        assert_eq!(neg, Vec3([-1.0, -2.0, -3.0]));

        // Dot product
        let dot = v1.dot(v2);
        assert_eq!(dot, 32.0); // 1*4 + 2*5 + 3*6 = 32

        // Magnitude
        let v = Vec3([3.0, 4.0, 0.0]);
        assert_eq!(v.abs(), 5.0);
    }

    #[test]
    fn test_mat3_identity() {
        let identity = Mat3::EYE;
        let v = Vec3([1.0, 2.0, 3.0]);
        let result = identity.mul_vec(v);
        assert_eq!(result, v);
    }

    #[test]
    fn test_mat3_mul_vec() {
        // Simple scaling matrix: scale by 2, 3, 4
        let mat = Mat3([2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0]);
        let v = Vec3([1.0, 2.0, 3.0]);
        let result = mat.mul_vec(v);
        assert_eq!(result, Vec3([2.0, 6.0, 12.0]));
    }

    #[test]
    fn test_mat3_mul_mat() {
        // Test matrix multiplication with identity
        let mat = Mat3([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        let result = mat * Mat3::EYE;
        assert!(mat3_approx_eq(&result, &mat));

        // Test simple rotation-like multiplication
        let a = Mat3([1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0]);
        let b = Mat3([0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0]);
        let result = a * b;
        let expected = Mat3([0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        assert!(mat3_approx_eq(&result, &expected));
    }

    #[test]
    fn test_mat3_determinant() {
        // Identity has determinant 1
        assert!(approx_eq(Mat3::EYE.det(), 1.0));

        // Scaling matrix
        let mat = Mat3([2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0]);
        assert!(approx_eq(mat.det(), 24.0)); // 2 * 3 * 4

        // General matrix
        let mat = Mat3([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        assert!(approx_eq(mat.det(), 0.0)); // Singular matrix
    }

    #[test]
    fn test_mat3_inverse() {
        // Inverse of identity is identity
        let inv = Mat3::EYE.inverse();
        assert!(mat3_approx_eq(&inv, &Mat3::EYE));

        // Inverse of scaling matrix
        let mat = Mat3([2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0]);
        let inv = mat.inverse();
        let expected = Mat3([0.5, 0.0, 0.0, 0.0, 1.0 / 3.0, 0.0, 0.0, 0.0, 0.25]);
        assert!(mat3_approx_eq(&inv, &expected));

        // M * M^-1 = I
        let result = mat * inv;
        assert!(mat3_approx_eq(&result, &Mat3::EYE));

        // More complex matrix
        let mat = Mat3([1.0, 2.0, 3.0, 0.0, 1.0, 4.0, 5.0, 6.0, 0.0]);
        let inv = mat.inverse();
        let result = mat * inv;
        assert!(mat3_approx_eq(&result, &Mat3::EYE));
    }

    #[test]
    fn test_mat3_scale_extraction() {
        // Pure scaling matrix
        let mat = Mat3([2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0]);
        let (normalized, scale) = mat.without_scale();
        assert!(vec3_approx_eq(scale, Vec3([2.0, 3.0, 4.0])));
        assert!(mat3_approx_eq(&normalized, &Mat3::EYE));

        // Scaled rotation (90 degree around Z-axis, scaled by 2, 3, 4)
        let mat = Mat3([0.0, -2.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0]);
        let (normalized, scale) = mat.without_scale();
        assert!(vec3_approx_eq(scale, Vec3([2.0, 3.0, 4.0])));

        let expected_normalized = Mat3([0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        assert!(mat3_approx_eq(&normalized, &expected_normalized));
    }

    #[test]
    fn test_mat3_remove_scale() {
        let mut mat = Mat3([2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0]);
        let scale = mat.remove_scale();
        assert!(vec3_approx_eq(scale, Vec3([2.0, 3.0, 4.0])));
        assert!(mat3_approx_eq(&mat, &Mat3::EYE));
    }

    #[test]
    fn test_mat3_to_quat() {
        // Identity matrix -> identity quaternion
        let quat = Mat3::EYE.to_quat();
        assert!(quat_approx_eq(quat, Quat([0.0, 0.0, 0.0, 1.0])));

        // 90 degree rotation around Z-axis
        let mat = Mat3([0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let quat = mat.to_quat();
        // 90 degree around Z: quat = [0, 0, sin(45 degrees), cos(45 degrees)] = [0, 0, 0.707, 0.707]
        let expected = Quat([0.0, 0.0, FRAC_1_SQRT_2, FRAC_1_SQRT_2]);
        assert!(quat_approx_eq(quat, expected));

        // 180 degree rotation around X-axis
        let mat = Mat3([1.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, -1.0]);
        let quat = mat.to_quat();
        // 180 degree around X: quat = [1, 0, 0, 0]
        let expected = Quat([1.0, 0.0, 0.0, 0.0]);
        assert!(quat_approx_eq(quat, expected));
    }

    #[test]
    fn test_mat3_to_euler() {
        // Identity matrix -> zero rotations
        let euler = Mat3::EYE.to_euler();
        println!("Identity: euler = {:?}", euler);
        assert!(euler_approx_eq(euler, EulerXYZ([0.0, 0.0, 0.0])));

        // 90° rotation around Z-axis (pure yaw)
        let mat = Mat3([0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let euler = mat.to_euler();
        println!("90 degree Z (yaw): euler = {:?}", euler);
        assert!(euler_approx_eq(euler, EulerXYZ([0.0, 0.0, FRAC_PI_2])));

        // 45° rotation around Z-axis
        let angle = FRAC_PI_4;
        let c = angle.cos();
        let s = angle.sin();
        let mat = Mat3([c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0]);
        let euler = mat.to_euler();
        println!("45 degree Z (yaw): euler = {:?}", euler);
        assert!(euler_approx_eq(euler, EulerXYZ([0.0, 0.0, angle])));

        // 30° rotation around Y-axis (pure pitch)
        let angle = PI / 6.0;
        let c = angle.cos();
        let s = angle.sin();
        let mat = Mat3([c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c]);
        let euler = mat.to_euler();
        println!("30 degree Y (pitch): euler = {:?}", euler);
        assert!(euler_approx_eq(euler, EulerXYZ([0.0, angle, 0.0])));

        // 60° rotation around X-axis (pure roll)
        let angle = PI / 3.0;
        let c = angle.cos();
        let s = angle.sin();
        let mat = Mat3([1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c]);
        let euler = mat.to_euler();
        println!("60 degree X (roll): euler = {:?}", euler);
        assert!(euler_approx_eq(euler, EulerXYZ([angle, 0.0, 0.0])));

        // Combined rotation: 30 degree yaw, 20 degree pitch, 10 degree roll
        let roll = PI / 18.0; // 10 degrees
        let pitch = PI / 9.0; // 20 degrees
        let yaw = PI / 6.0; // 30 degree

        let (sr, cr) = roll.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();

        let mat = Mat3([
            cy * cp,
            -cp * sy,
            sp,
            cr * sy + cy * sr * sp,
            cr * cy - sr * sp * sy,
            -cp * sr,
            sr * sy - cr * cy * sp,
            cy * sr + cr * sp * sy,
            cr * cp,
        ]);
        let euler = mat.to_euler();
        println!(
            "Combined (10 degree roll, 20 degree pitch, 30 degree yaw): euler = {:?}",
            euler
        );
        assert!(euler_approx_eq(euler, EulerXYZ([roll, pitch, yaw])));
    }

    #[test]
    fn test_rotation_consistency() {
        // Create a rotation matrix, convert to quaternion,
        // and verify properties
        let angle = FRAC_PI_4; // 45 degrees
        let c = angle.cos();
        let s = angle.sin();

        // Rotation around Z-axis
        let mat = Mat3([c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0]);

        // Check that det = 1 for rotation matrices
        assert!(approx_eq(mat.det(), 1.0));

        // Check that M * M^T = I for orthonormal matrices
        let mt = Mat3([c, s, 0.0, -s, c, 0.0, 0.0, 0.0, 1.0]);
        let result = mat * mt;
        assert!(mat3_approx_eq(&result, &Mat3::EYE));

        // Quaternion should be unit length
        let quat = mat.to_quat();
        let quat_len_sq =
            quat.x() * quat.x() + quat.y() * quat.y() + quat.z() * quat.z() + quat.w() * quat.w();
        assert!(
            approx_eq(quat_len_sq, 1.0),
            "non-unit quaternion: len^2 = {quat_len_sq}"
        );
    }

    #[test]
    fn test_row_major_layout() {
        // Verify row-major layout by checking matrix-vector multiplication
        // For a row-major matrix, M * v should give us the dot product of each row with v
        let mat = Mat3([
            1.0, 2.0, 3.0, // First row
            4.0, 5.0, 6.0, // Second row
            7.0, 8.0, 9.0, // Third row
        ]);
        let v = Vec3([1.0, 1.0, 1.0]);
        let result = mat.mul_vec(v);

        // Expected: each component is the sum of the corresponding row
        assert_eq!(result, Vec3([6.0, 15.0, 24.0]));
    }

    #[test]
    fn test_complex_transformation_chain() {
        // Scale, then rotate, then translate (translation not in Mat3, but we can test scale+rotate)
        let scale = Mat3([2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0]);

        let rotate_z = Mat3([0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);

        let combined = rotate_z * scale;
        let v = Vec3([1.0, 0.0, 0.0]);
        let result = combined.mul_vec(v);

        // First scaled to [2,0,0], then rotated 90 degrees around Z to [0,2,0]
        assert!(vec3_approx_eq(result, Vec3([0.0, 2.0, 0.0])));
    }
}
