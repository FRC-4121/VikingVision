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
    fn clone_to_arc(&self) -> std::sync::Arc<dyn Data> {
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
/// This is stored internally in column-major order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Mat3(pub [f64; 9]);
impl Mat3 {
    /// The identity matrix.
    pub const EYE: Self = Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    /// Strip a scaling from the matrix to get the normalized matrix and its scale.
    pub fn without_scale(&self) -> (Self, Vec3) {
        let Mat3([m0, m1, m2, m3, m4, m5, m6, m7, m8]) = *self;
        let sx = (m0 * m0 + m3 * m3 + m6 * m6).sqrt();
        let sy = (m1 * m1 + m4 * m4 + m7 * m7).sqrt();
        let sz = (m2 * m2 + m5 * m5 + m8 * m8).sqrt();
        let r = Self([
            m0 / sx,
            m1 / sy,
            m2 / sz,
            m3 / sx,
            m4 / sy,
            m5 / sz,
            m6 / sx,
            m7 / sy,
            m8 / sz,
        ]);
        (r, Vec3([sx, sy, sz]))
    }
    /// Remove the scale from this matrix.
    pub fn remove_scale(&mut self) -> Vec3 {
        let Mat3([m0, m1, m2, m3, m4, m5, m6, m7, m8]) = self;
        let sx = (*m0 * *m0 + *m3 * *m3 + *m6 * *m6).sqrt();
        let sy = (*m1 * *m1 + *m4 * *m4 + *m7 * *m7).sqrt();
        let sz = (*m2 * *m2 + *m5 * *m5 + *m8 * *m8).sqrt();
        *m0 /= sx;
        *m1 /= sy;
        *m2 /= sz;
        *m3 /= sx;
        *m4 /= sy;
        *m5 /= sz;
        *m6 /= sx;
        *m7 /= sy;
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
    pub fn to_euler(&self) -> EulerXYZ {
        let Mat3([m0, _, _, m3, m4, m5, m6, m7, m8]) = *self;
        let pitch = (-m6).asin();
        let cos_pitch = pitch.cos();
        let (roll, yaw) = if cos_pitch.abs() > 1e-6 {
            (m7.atan2(m8), m3.atan2(m0))
        } else {
            // Gimbal lock
            ((-m5).atan2(m4), 0.0)
        };
        EulerXYZ([roll, pitch, yaw])
    }
    /// Multiply this matrix by another matrix.
    pub fn mul_mat(&self, rhs: &Self) -> Self {
        let mut out = [0.0; 9];
        for (n, val) in out.iter_mut().enumerate() {
            let mut a_idx = n % 3;
            let mut b_idx = n / 3 * 3;
            for _ in 0..3 {
                *val += self.0[a_idx] * rhs.0[b_idx];
                a_idx += 3;
                b_idx += 1;
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
            - self.0[3] * (self.0[1] * self.0[8] - self.0[2] * self.0[7])
            + self.0[6] * (self.0[1] * self.0[5] - self.0[2] * self.0[4])
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
    fn clone_to_arc(&self) -> std::sync::Arc<dyn Data> {
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
    fn clone_to_arc(&self) -> std::sync::Arc<dyn Data> {
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
    fn clone_to_arc(&self) -> std::sync::Arc<dyn Data> {
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
