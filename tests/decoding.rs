// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use rawdinal::{Entry, X3f};

fn put_u32(bytes: &mut [u8], offset: usize, value: usize) {
    bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
}

fn encode_bytes(decoded: &[u8]) -> Vec<u8> {
    let mut bits = Vec::new();
    let mut previous = 0i32;
    for &byte in decoded {
        let difference = i32::from(byte) - previous;
        previous = i32::from(byte);
        let length = 32 - difference.unsigned_abs().leading_zeros();
        for shift in (0..4).rev() {
            bits.push(((length >> shift) & 1) as u8);
        }
        let encoded = if difference < 0 {
            difference + (1 << length) - 1
        } else {
            difference
        };
        for shift in (0..length).rev() {
            bits.push(((encoded >> shift) & 1) as u8);
        }
    }
    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0, |byte, (index, bit)| byte | (bit << (7 - index)))
        })
        .collect()
}

fn calibration_section() -> Vec<u8> {
    let mut decoded = vec![0; 40];
    decoded[..4].copy_from_slice(b"CMbT");
    put_u32(&mut decoded, 8, 40);
    put_u32(&mut decoded, 12, 20);
    put_u32(&mut decoded, 16, 24);
    decoded[20..24].copy_from_slice(b"Tag\0");
    put_u32(&mut decoded, 24, 12);
    decoded[28..].copy_from_slice(b"hello world\0");
    let stream = encode_bytes(&decoded);
    let mut section = vec![0; 60];
    section[..4].copy_from_slice(b"SECc");
    put_u32(&mut section, 4, 0x20000);
    put_u32(&mut section, 8, 5);
    put_u32(&mut section, 12, decoded.len());
    for symbol in 0..9 {
        section[28 + 2 * symbol] = 4;
        section[29 + 2 * symbol] = (symbol as u8) << 4;
    }
    put_u32(&mut section, 56, stream.len());
    section.extend(stream);
    section
}

fn raw_section() -> Vec<u8> {
    let mut section = vec![0; 102];
    section[..4].copy_from_slice(b"SECi");
    put_u32(&mut section, 4, 0x20000);
    put_u32(&mut section, 8, 1);
    put_u32(&mut section, 12, 0x25);
    put_u32(&mut section, 16, 4);
    put_u32(&mut section, 20, 4);
    for channel in 0..3 {
        let size = if channel == 2 { 4u16 } else { 2u16 };
        section[28 + 4 * channel..30 + 4 * channel].copy_from_slice(&size.to_le_bytes());
        section[30 + 4 * channel..32 + 4 * channel].copy_from_slice(&size.to_le_bytes());
        section[40 + 2 * channel] = 10 + channel as u8;
    }
    section[48] = 1;
    put_u32(&mut section, 56, 1);
    put_u32(&mut section, 60, 1);
    put_u32(&mut section, 64, 2);
    section
}

fn fixture() -> Vec<u8> {
    let mut file = vec![0; 40];
    file[..4].copy_from_slice(b"FOVb");
    put_u32(&mut file, 4, 0x40002);
    let camf = calibration_section();
    let raw = raw_section();
    file.extend(&camf);
    file.extend(&raw);
    let directory_offset = file.len();
    let mut directory = vec![0; 40];
    directory[..4].copy_from_slice(b"SECd");
    put_u32(&mut directory, 4, 0x20000);
    put_u32(&mut directory, 8, 2);
    put_u32(&mut directory, 12, 40);
    put_u32(&mut directory, 16, camf.len());
    directory[20..24].copy_from_slice(b"CAMF");
    put_u32(&mut directory, 24, 40 + camf.len());
    put_u32(&mut directory, 28, raw.len());
    directory[32..36].copy_from_slice(b"IMA2");
    put_u32(&mut directory, 36, directory_offset);
    file.extend(directory);
    file
}

#[test]
fn decodes_three_physical_planes_without_color_processing() {
    let bytes = fixture();
    let image = X3f::parse(&bytes).unwrap().decode().unwrap();
    for (index, layer) in image.layers.iter().enumerate() {
        assert!(
            layer
                .samples
                .iter()
                .all(|&sample| sample == 10 + index as u16)
        );
        assert_eq!(
            (layer.width, layer.height),
            if index == 2 { (4, 4) } else { (2, 2) }
        );
    }
}

#[test]
fn decodes_camf_predictor_and_text_entry() {
    let bytes = fixture();
    let metadata = X3f::parse(&bytes).unwrap().calibration().unwrap();
    assert!(
        matches!(metadata.entries.get("Tag"), Some(Entry::Text(text)) if text == "hello world")
    );
}

#[test]
fn rejects_all_truncated_prefixes() {
    let bytes = fixture();
    for length in 0..bytes.len() {
        assert!(X3f::parse(&bytes[..length]).is_err());
    }
}

#[test]
fn rejects_overlapping_sections() {
    let mut bytes = fixture();
    let directory = bytes.len() - 40;
    put_u32(&mut bytes, directory + 24, 40);
    assert!(X3f::parse(&bytes).is_err());
}

#[test]
fn rejects_unsupported_raw_format() {
    let mut bytes = fixture();
    let raw_offset = 40 + calibration_section().len();
    put_u32(&mut bytes, raw_offset + 12, 0x99);
    assert!(X3f::parse(&bytes).unwrap().decode().is_err());
}

#[test]
fn single_byte_mutations_do_not_panic() {
    let original = fixture();
    for index in 0..original.len() {
        for replacement in [0, 1, 127, 255] {
            let mut bytes = original.clone();
            bytes[index] = replacement;
            if let Ok(file) = X3f::parse(&bytes) {
                let _ = file.calibration();
                let _ = file.decode();
            }
        }
    }
}

#[test]
#[ignore = "requires X3F_SAMPLE and X3F_REFERENCE_DIR containing independent decoded files"]
fn sample_matches_independent_reference_byte_for_byte() {
    let bytes = std::fs::read(std::env::var_os("X3F_SAMPLE").expect("X3F_SAMPLE")).unwrap();
    let directory =
        std::path::PathBuf::from(std::env::var_os("X3F_REFERENCE_DIR").expect("X3F_REFERENCE_DIR"));
    let file = X3f::parse(&bytes).unwrap();
    let reference = std::fs::read(directory.join("calibration.bin")).unwrap();
    assert_eq!(file.calibration().unwrap().decoded_bytes(), reference);
    for (index, plane) in file.decode().unwrap().layers.iter().enumerate() {
        let reference = std::fs::read(directory.join(format!("layer-{index}.u16le"))).unwrap();
        assert_eq!(reference.len(), 2 * plane.samples.len());
        for (pixel, (sample, pair)) in plane
            .samples
            .iter()
            .zip(reference.chunks_exact(2))
            .enumerate()
        {
            assert_eq!(
                *sample,
                u16::from_le_bytes([pair[0], pair[1]]),
                "layer {index}, pixel {pixel}"
            );
        }
    }
}
