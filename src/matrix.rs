//! `Matrix4x3_2012`: the affine transform used for bone bind poses, animation keyframes and the
//! mesh's `matrixLoad4x3` command.
//!
//! Twelve 20.12 fixed-point words: three basis vectors followed by the translation, i.e. the file
//! order is `x.x x.y x.z  y.x y.y y.z  z.x z.y z.z  t.x t.y t.z`. Because the basis vectors are
//! stored as `SIMD3` in the reference, they are consecutive - so this is a flat 12-word read.
//!
//! Transforming a point is `v.x * x + v.y * y + v.z * z + translation`, which means the basis
//! vectors are the *columns* of the equivalent 4x4 matrix.

use anyhow::Result;

use crate::bytes::Reader;
use crate::fixed;

pub type Vec3 = [f64; 3];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix4x3 {
    pub x: Vec3,
    pub y: Vec3,
    pub z: Vec3,
    pub translation: Vec3,
}

impl Default for Matrix4x3 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Matrix4x3 {
    pub const IDENTITY: Self = Self {
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        z: [0.0, 0.0, 1.0],
        translation: [0.0, 0.0, 0.0],
    };

    /// Reads 12 consecutive 20.12 words.
    pub fn read(reader: &mut Reader<'_>) -> Result<Self> {
        let mut values = [0.0f64; 12];
        for value in &mut values {
            *value = fixed::from_2012(reader.u32()?);
        }
        Ok(Self {
            x: [values[0], values[1], values[2]],
            y: [values[3], values[4], values[5]],
            z: [values[6], values[7], values[8]],
            translation: [values[9], values[10], values[11]],
        })
    }

    /// Builds one from 12 already-decoded values, in the same order `read` consumes them.
    pub fn from_values(values: &[f64; 12]) -> Self {
        Self {
            x: [values[0], values[1], values[2]],
            y: [values[3], values[4], values[5]],
            z: [values[6], values[7], values[8]],
            translation: [values[9], values[10], values[11]],
        }
    }

    /// `v.x * x + v.y * y + v.z * z + translation`.
    pub fn transform(&self, v: Vec3) -> Vec3 {
        [
            v[0] * self.x[0] + v[1] * self.y[0] + v[2] * self.z[0] + self.translation[0],
            v[0] * self.x[1] + v[1] * self.y[1] + v[2] * self.z[1] + self.translation[1],
            v[0] * self.x[2] + v[1] * self.y[2] + v[2] * self.z[2] + self.translation[2],
        ]
    }

    /// The equivalent 4x4 matrix in glTF's column-major order.
    ///
    /// glTF stores matrices column by column, and our basis vectors are already the columns, so the
    /// translation lands in the last four entries.
    pub fn to_gltf_matrix(&self) -> [f32; 16] {
        [
            self.x[0] as f32,
            self.x[1] as f32,
            self.x[2] as f32,
            0.0,
            self.y[0] as f32,
            self.y[1] as f32,
            self.y[2] as f32,
            0.0,
            self.z[0] as f32,
            self.z[1] as f32,
            self.z[2] as f32,
            0.0,
            self.translation[0] as f32,
            self.translation[1] as f32,
            self.translation[2] as f32,
            1.0,
        ]
    }

    /// True when every component is finite, which the defective exported rotations were not.
    pub fn is_finite(&self) -> bool {
        self.x.iter().chain(&self.y).chain(&self.z).chain(&self.translation).all(|value| value.is_finite())
    }

    /// The inverse of this affine transform, or `None` when the basis is degenerate.
    ///
    /// The three basis vectors are the columns of the 3x3 part, so inverting means inverting that
    /// 3x3 by cofactors and then mapping the translation through it: the inverse of `[R | t]` is
    /// `[R^-1 | -R^-1 t]`. glTF wants this for a skin's inverse bind matrices.
    pub fn inverse(&self) -> Option<Self> {
        let (a, b, c) = (self.x, self.y, self.z);

        let det = a[0] * (b[1] * c[2] - b[2] * c[1]) - b[0] * (a[1] * c[2] - a[2] * c[1])
            + c[0] * (a[1] * b[2] - a[2] * b[1]);
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let scale = 1.0 / det;

        // cofactors of the 3x3 part, already transposed into inverse-column order
        let ix = [
            (b[1] * c[2] - b[2] * c[1]) * scale,
            -(a[1] * c[2] - a[2] * c[1]) * scale,
            (a[1] * b[2] - a[2] * b[1]) * scale,
        ];
        let iy = [
            -(b[0] * c[2] - b[2] * c[0]) * scale,
            (a[0] * c[2] - a[2] * c[0]) * scale,
            -(a[0] * b[2] - a[2] * b[0]) * scale,
        ];
        let iz = [
            (b[0] * c[1] - b[1] * c[0]) * scale,
            -(a[0] * c[1] - a[1] * c[0]) * scale,
            (a[0] * b[1] - a[1] * b[0]) * scale,
        ];

        let t = self.translation;
        let translation = [
            -(ix[0] * t[0] + iy[0] * t[1] + iz[0] * t[2]),
            -(ix[1] * t[0] + iy[1] * t[1] + iz[1] * t[2]),
            -(ix[2] * t[0] + iy[2] * t[1] + iz[2] * t[2]),
        ];

        Some(Self {
            x: ix,
            y: iy,
            z: iz,
            translation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(values: &[f64; 12]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in values {
            bytes.extend_from_slice(&fixed::to_2012(*value).to_le_bytes());
        }
        bytes
    }

    #[test]
    fn reads_and_transforms_the_identity() {
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        let bytes = reader(&identity);
        let matrix = Matrix4x3::read(&mut Reader::new(&bytes)).unwrap();
        assert!(matrix.is_finite());
        assert_eq!(matrix.transform([1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn applies_translation_and_scale() {
        // scale x2 in x, and translate by (10, 0, -5)
        let values = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 10.0, 0.0, -5.0];
        let bytes = reader(&values);
        let matrix = Matrix4x3::read(&mut Reader::new(&bytes)).unwrap();
        assert_eq!(matrix.transform([1.0, 1.0, 1.0]), [12.0, 1.0, -4.0]);
    }

    #[test]
    fn gltf_matrix_is_column_major() {
        let values = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 3.0];
        let matrix = Matrix4x3::from_values(&values);
        let gltf = matrix.to_gltf_matrix();
        assert_eq!(&gltf[12..15], &[1.0, 2.0, 3.0]);
        assert_eq!(gltf[15], 1.0);
    }

    #[test]
    fn the_inverse_undoes_the_transform() {
        // a scale, a shear and a translation - nothing special, just not the identity
        let values = [2.0, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.25, 3.0, 10.0, -4.0, 2.0];
        let matrix = Matrix4x3::from_values(&values);
        let inverse = matrix.inverse().expect("an invertible basis");

        for point in [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0], [-5.5, 0.25, 7.0]] {
            let there = matrix.transform(point);
            let back = inverse.transform(there);
            for axis in 0..3 {
                assert!(
                    (back[axis] - point[axis]).abs() < 1e-9,
                    "axis {axis}: {back:?} should round-trip to {point:?}"
                );
            }
        }
    }

    #[test]
    fn a_degenerate_basis_has_no_inverse() {
        // every basis vector is zero
        let mut degenerate = Matrix4x3::IDENTITY;
        degenerate.x = [0.0, 0.0, 0.0];
        degenerate.y = [0.0, 0.0, 0.0];
        degenerate.z = [0.0, 0.0, 0.0];
        assert!(degenerate.inverse().is_none());
    }
}
