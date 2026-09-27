//! Fixed-point conversions used by the mesh, animation and GPU command formats.
//!
//! Three widths appear, all with **12 fractional bits**, and all signed:
//!
//! | Name | Storage | Where |
//! | --- | --- | --- |
//! | `FixedPoint2012` | 32-bit (20 integer bits) | bone matrices, animation keyframes, mesh scale |
//! | `FixedPoint412` | 16-bit (4 integer bits) | vertex coordinates |
//! | `FixedPoint124` | 16-bit | texture coordinates |
//!
//! The 12 fractional bits are not a guess: the reference recorded that the mesh header's first word
//! is `131072`, and `131072 / 4096 = 32.0` matches the "for ff1, always 32" scale note, while the
//! exported texture coordinates are all exact multiples of `1/4096` (for example
//! `0.77490234375 = 3174 / 4096`).

/// Fractional bits in every format here.
pub const FRACTIONAL_BITS: u32 = 12;
/// `1 << FRACTIONAL_BITS`, as a float.
pub const ONE: f64 = 4096.0;

/// 20.12 signed fixed point (32-bit storage).
pub fn from_2012(raw: u32) -> f64 {
    (raw as i32) as f64 / ONE
}

/// 4.12 signed fixed point (16-bit storage).
pub fn from_412(raw: i16) -> f64 {
    raw as f64 / ONE
}

/// 1.12 signed fixed point (16-bit storage) - texture coordinates.
pub fn from_124(raw: i16) -> f64 {
    raw as f64 / ONE
}

/// Encodes a value back into 20.12 form, for round-trip tests.
pub fn to_2012(value: f64) -> u32 {
    (value * ONE).round() as i32 as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mesh_scale_word_decodes_to_32() {
        // every mesh header starts with 0x00020000 and the format note says "always 32"
        assert_eq!(from_2012(0x0002_0000), 32.0);
    }

    #[test]
    fn texture_coordinates_are_exact_multiples_of_one_over_4096() {
        assert!((from_124(3174) - 0.774_902_343_75).abs() < f64::EPSILON);
        assert_eq!(from_124(4096), 1.0);
        assert_eq!(from_124(0), 0.0);
    }

    #[test]
    fn negative_values_are_signed() {
        assert_eq!(from_2012((-4096i32) as u32), -1.0);
        assert_eq!(from_412(-2048), -0.5);
    }

    #[test]
    fn round_trips_through_2012() {
        for value in [-10.5, -1.0, 0.0, 1.0, 32.0, 1234.25] {
            let raw = to_2012(value);
            assert!((from_2012(raw) - value).abs() < 1.0 / ONE);
        }
    }
}
