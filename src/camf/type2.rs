// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use super::MAX_CALIBRATION_BYTES;
use crate::{Result, invalid, reader::Reader, zeroed};

/// Decodes the CAMF-2 XOR stream with wrapping 32-bit seed arithmetic.
/// The caller supplies the directory-bounded section, excluding any synthetic padding.
pub(super) fn decrypt(section: Reader<'_>) -> Result<Vec<u8>> {
    let mut state = section.u32(24)?;
    let encrypted = &section.0[28..];
    if encrypted.is_empty() || encrypted.len() > MAX_CALIBRATION_BYTES {
        return Err(invalid("invalid CAMF size"));
    }
    let mut decoded = zeroed(encrypted.len())?;
    for (output, &input) in decoded.iter_mut().zip(encrypted) {
        state = state.wrapping_mul(1597).wrapping_add(51749) % 244944;
        *output = input ^ (state * 256 / 244944) as u8;
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decrypts_known_keystreams_with_wrapping_seed_arithmetic() {
        for (seed, expected) in [
            (0u32, [54, 155, 50, 31, 104, 232, 45, 222, 91, 69, 141, 196]),
            (
                u32::MAX,
                [52, 49, 228, 110, 28, 67, 167, 24, 0, 109, 96, 255],
            ),
        ] {
            let mut section = [0; 40];
            section[24..28].copy_from_slice(&seed.to_le_bytes());
            assert_eq!(decrypt(Reader(&section)).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_incomplete_header_and_empty_payload() {
        for length in 0..=28 {
            assert!(decrypt(Reader(&[0; 28][..length])).is_err());
        }
    }

    #[test]
    fn rejects_payload_above_calibration_budget() {
        let section = vec![0; 28 + MAX_CALIBRATION_BYTES + 1];
        assert_eq!(
            decrypt(Reader(&section)).unwrap_err().to_string(),
            "invalid CAMF size"
        );
    }
}
