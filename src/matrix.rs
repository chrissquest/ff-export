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
}
