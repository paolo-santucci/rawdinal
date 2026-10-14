// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use rawdinal::{Entry, X3f};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const DECODED_TEXT: &[u8] =
    b"CMbT\0\0\x01\0\x28\0\0\0\x14\0\0\0\x18\0\0\0Tag\0\x0c\0\0\0hello world\0";
const ENCRYPTED_TEXT: &[u8] = &[
    119, 124, 134, 58, 28, 67, 166, 24, 40, 109, 96, 255, 91, 44, 35, 219, 172, 79, 159, 50, 60,
    72, 184, 81, 4, 243, 237, 201, 192, 69, 144, 90, 162, 14, 161, 183, 159, 173, 65, 200,
];

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn camf_section() -> Vec<u8> {
    let mut section = vec![0; 28];
    section[..4].copy_from_slice(b"SECc");
    put_u32(&mut section, 4, 0x20000);
    put_u32(&mut section, 8, 2);
    section[16..20].copy_from_slice(b"FCEb");
    put_u32(&mut section, 20, 0x10000);
    put_u32(&mut section, 24, u32::MAX);
    section.extend_from_slice(ENCRYPTED_TEXT);
    section
}

fn encrypted_section(payload: &[u8]) -> Vec<u8> {
    let mut section = camf_section();
    section.truncate(28);
    let mut state = u32::MAX;
    for &byte in payload {
        state = state.wrapping_mul(1597).wrapping_add(51749) % 244944;
        section.push(byte ^ (state * 256 / 244944) as u8);
    }
    section
}

fn raw_section(image_type: u32, format: u32) -> Vec<u8> {
    let mut section = vec![0; 28];
    section[..4].copy_from_slice(b"SECi");
    put_u32(&mut section, 4, 0x20000);
    put_u32(&mut section, 8, image_type);
    put_u32(&mut section, 12, format);
    section
}

fn container(version: u32, sections: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut bytes = vec![0; 40];
    bytes[..4].copy_from_slice(b"FOVb");
    put_u32(&mut bytes, 4, version);
    let mut directory = b"SECd\0\0\x02\0".to_vec();
    directory.extend_from_slice(&(sections.len() as u32).to_le_bytes());
    for (kind, section) in sections {
        directory.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        directory.extend_from_slice(&(section.len() as u32).to_le_bytes());
        directory.extend_from_slice(&kind);
        bytes.extend_from_slice(&section);
    }
    let offset = bytes.len() as u32;
    bytes.extend(directory);
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes
}

fn legacy_file(version: u32, kind: [u8; 4], camf: Vec<u8>) -> Vec<u8> {
    container(version, vec![(kind, raw_section(3, 6)), (*b"CAMF", camf)])
}

#[test]
fn legacy_camf2_is_accessible_without_sensor_decoding() {
    for (version, kind) in [
        (0x20001, *b"IMAG"),
        (0x20002, *b"IMAG"),
        (0x20002, *b"IMA2"),
    ] {
        let bytes = legacy_file(version, kind, camf_section());
        let file = X3f::parse(&bytes).unwrap();
        assert_eq!(file.camf_bytes().unwrap(), DECODED_TEXT);
        let calibration = file.calibration().unwrap();
        assert_eq!(calibration.decoded_bytes(), DECODED_TEXT);
        assert!(
            matches!(&calibration.entries["Tag"], Entry::Text(value) if value == "hello world")
        );
        assert_eq!(
            file.sensor_format().unwrap_err().to_string(),
            "unsupported RAW image type"
        );
        assert!(file.decode().is_err());
    }
}

#[test]
fn legacy_type3_does_not_enter_merrill_dispatch() {
    let bytes = container(
        0x20003,
        vec![(*b"IMA2", raw_section(3, 0x1e)), (*b"CAMF", camf_section())],
    );
    let file = X3f::parse(&bytes).unwrap();
    assert_eq!(
        file.sensor_format().unwrap_err().to_string(),
        "unsupported RAW image type"
    );
    assert!(file.decode().is_err());
    assert!(file.camf_bytes().is_err());
}

#[test]
fn rejects_camf2_for_supported_sensor_formats() {
    for (version, format) in [
        (0x20003, 0x1e),
        (0x40001, 0x23),
        (0x40002, 0x25),
        (0x40002, 0x27),
    ] {
        let bytes = container(
            version,
            vec![
                (*b"IMA2", raw_section(1, format)),
                (*b"CAMF", camf_section()),
            ],
        );
        let file = X3f::parse(&bytes).unwrap();
        assert!(file.sensor_format().is_ok());
        assert_eq!(
            file.camf_bytes().unwrap_err().to_string(),
            "unsupported CAMF type for sensor format"
        );
    }
}

#[test]
fn rejects_unobserved_legacy_camf_combinations() {
    for version in [0x20003, 0x30000, 0x40001, 0x40002] {
        let bytes = legacy_file(version, *b"IMA2", camf_section());
        assert!(
            X3f::parse(&bytes)
                .and_then(|file| file.camf_bytes())
                .is_err()
        );
    }
    for kind in [0, 1, 3, 4, 5, 6, u32::MAX] {
        let mut section = camf_section();
        put_u32(&mut section, 8, kind);
        let bytes = legacy_file(0x20002, *b"IMA2", section);
        assert!(X3f::parse(&bytes).unwrap().camf_bytes().is_err());
    }
}

#[test]
fn legacy_directory_validation_rejects_duplicates_and_missing_sections() {
    for sections in [
        vec![(*b"IMAG", raw_section(3, 6))],
        vec![(*b"CAMF", camf_section())],
        vec![
            (*b"IMAG", raw_section(3, 6)),
            (*b"IMA2", raw_section(3, 6)),
            (*b"CAMF", camf_section()),
        ],
        vec![
            (*b"IMAG", raw_section(3, 6)),
            (*b"CAMF", camf_section()),
            (*b"CAMF", camf_section()),
        ],
    ] {
        assert!(X3f::parse(&container(0x20002, sections)).is_err());
    }
}

#[test]
fn imag_does_not_alias_supported_type1_layouts() {
    let bytes = container(
        0x20003,
        vec![(*b"IMAG", raw_section(1, 0x1e)), (*b"CAMF", camf_section())],
    );
    assert!(X3f::parse(&bytes).is_err());
}

#[test]
fn rejects_invalid_legacy_section_signatures_and_versions() {
    for (offset, value) in [(0, 0), (4, 0x10000)] {
        let mut section = camf_section();
        put_u32(&mut section, offset, value);
        let bytes = legacy_file(0x20002, *b"IMA2", section);
        assert!(
            X3f::parse(&bytes)
                .and_then(|file| file.camf_bytes())
                .is_err()
        );
    }
    let mut raw = raw_section(3, 6);
    raw[0] = 0;
    let bytes = container(0x20001, vec![(*b"IMAG", raw), (*b"CAMF", camf_section())]);
    assert!(X3f::parse(&bytes).is_err());
}

#[test]
fn rejects_camf2_truncation_during_extraction_or_entry_parsing() {
    let section = camf_section();
    for length in 0..section.len() {
        let bytes = legacy_file(0x20002, *b"IMA2", section[..length].to_vec());
        assert!(
            X3f::parse(&bytes)
                .and_then(|file| file.calibration())
                .is_err(),
            "length {length}"
        );
        if length <= 28 {
            assert!(
                X3f::parse(&bytes)
                    .and_then(|file| file.camf_bytes())
                    .is_err()
            );
        }
    }
}

#[test]
fn rejects_malformed_decrypted_entries_without_changing_extracted_bytes() {
    let mut invalid_payloads = vec![DECODED_TEXT.repeat(2)];
    for (offset, value) in [
        (0, 0),
        (8, 19),
        (8, u32::MAX),
        (12, 19),
        (12, u32::MAX),
        (16, 19),
        (16, u32::MAX),
        (24, u32::MAX),
    ] {
        let mut payload = DECODED_TEXT.to_vec();
        put_u32(&mut payload, offset, value);
        invalid_payloads.push(payload);
    }
    let mut invalid_utf8 = DECODED_TEXT.to_vec();
    invalid_utf8[28] = 255;
    invalid_payloads.push(invalid_utf8);
    let mut unterminated_name = DECODED_TEXT.to_vec();
    put_u32(&mut unterminated_name, 12, 39);
    unterminated_name[39] = b'x';
    invalid_payloads.push(unterminated_name);
    for payload in invalid_payloads {
        let bytes = legacy_file(0x20002, *b"IMA2", encrypted_section(&payload));
        let file = X3f::parse(&bytes).unwrap();
        assert_eq!(file.camf_bytes().unwrap(), payload);
        assert!(file.calibration().is_err());
    }
}

#[test]
fn legacy_camf2_mutations_do_not_panic() {
    let original = legacy_file(0x20001, *b"IMAG", camf_section());
    for offset in 0..original.len() {
        for value in [0, 127, 255] {
            let mut bytes = original.clone();
            bytes[offset] = value;
            if let Ok(file) = X3f::parse(&bytes) {
                let _ = file.calibration();
                let _ = file.decode();
            }
        }
    }
}

#[test]
#[ignore = "requires X3F_PIXLS_SAMPLE_DIR with the hash-verified SD9, SD10 and SD14 originals"]
fn legacy_camf2_matches_known_payload_hashes() {
    let root =
        PathBuf::from(std::env::var_os("X3F_PIXLS_SAMPLE_DIR").expect("X3F_PIXLS_SAMPLE_DIR"));
    for (path, source_hash, decoded_hash, size, count) in [
        (
            "Sigma SD9/RAW_SIGMA_SD9_SRGB.X3F",
            "9faed9efe3bb53f34534b487da3045f01d58935f7377450097b6063947964109",
            "ca7305c4c4a588e9f59e37f4dd4b2ba9a06d98b560b50d6c907ed7a89f0d0048",
            37032,
            48,
        ),
        (
            "Sigma SD10/RAW_SIGMA_SD10.X3F",
            "ea436ba459a7478d7b338428656fae905469de269f928749f3388db2b7b0d53f",
            "d18fd95fd66c4229c3b503fee5301daece59e787646149cc8a5126ed71f54509",
            52520,
            54,
        ),
        (
            "SIGMA SD14/20120818-SDIM4579.X3F",
            "c32f247eb372c3e66ad3b433518ec11f898fdc317c8432ebe025b46d4917bcf3",
            "66a70fcc4b04f247954888f147e305713f7543bcaff7d350c34c3f7f5b093b75",
            39220,
            94,
        ),
    ] {
        let bytes = std::fs::read(root.join(path)).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            source_hash,
            "{path}"
        );
        let file = X3f::parse(&bytes).unwrap();
        let payload = file.camf_bytes().unwrap();
        assert_eq!(payload.len(), size, "{path}");
        assert_eq!(
            format!("{:x}", Sha256::digest(&payload)),
            decoded_hash,
            "{path}"
        );
        let calibration = file.calibration().unwrap();
        assert_eq!(calibration.entries.len(), count, "{path}");
        assert_eq!(calibration.decoded_bytes(), payload, "{path}");
        assert!(file.decode().is_err(), "{path}");
    }
}
