// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use rawdinal::{Entry, SensorFormat, X3f};
use std::path::{Path, PathBuf};

const REFERENCE_DECODER: &str = "Kalpanika/x3f@e8b48cac0de92c1f0e694ac87a9e0fc31d65cbe3";
const CANONICAL_REFERENCE_MANIFEST: &str = "schema=1\ndecoder=Kalpanika/x3f@e8b48cac0de92c1f0e694ac87a9e0fc31d65cbe3\nx3f_version=0x00040001\nraw_type_format=0x00010023\ncamf_type=5\ncamf_bytes=12\nlayer_0_width=2944\nlayer_0_height=1836\nlayer_0_source=x3rgb16\nlayer_0_component=0\nlayer_1_width=2944\nlayer_1_height=1836\nlayer_1_source=x3rgb16\nlayer_1_component=1\nlayer_2_width=6272\nlayer_2_height=3672\nlayer_2_source=top16\nlayer_2_component=0\n";

struct ReferencePlane {
    width: usize,
    height: usize,
    source: String,
    component: u32,
}

struct ReferenceManifest {
    x3f_version: u32,
    raw_type_format: u32,
    camf_type: u32,
    camf_bytes: usize,
    layers: [ReferencePlane; 3],
}

fn parse_decimal(value: &str, key: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("invalid decimal value for {key}"))
}

fn parse_u32(value: &str, key: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("invalid integer value for {key}"))
}

fn parse_hex(value: &str, key: &str) -> Result<u32, String> {
    value
        .strip_prefix("0x")
        .ok_or_else(|| format!("invalid hexadecimal value for {key}"))
        .and_then(|value| {
            u32::from_str_radix(value, 16)
                .map_err(|_| format!("invalid hexadecimal value for {key}"))
        })
}

fn required<T>(value: Option<T>, key: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("missing {key}"))
}

fn reference_plane(
    width: Option<usize>,
    height: Option<usize>,
    source: Option<String>,
    component: Option<u32>,
    index: usize,
) -> Result<ReferencePlane, String> {
    Ok(ReferencePlane {
        width: required(width, &format!("layer_{index}_width"))?,
        height: required(height, &format!("layer_{index}_height"))?,
        source: required(source, &format!("layer_{index}_source"))?,
        component: required(component, &format!("layer_{index}_component"))?,
    })
}

fn set_once<T>(slot: &mut Option<T>, value: T, key: &str) -> Result<(), String> {
    if slot.replace(value).is_some() {
        Err(format!("duplicate {key}"))
    } else {
        Ok(())
    }
}

fn parse_reference_manifest(text: &str) -> Result<ReferenceManifest, String> {
    let mut schema = None;
    let mut decoder = None;
    let mut x3f_version = None;
    let mut raw_type_format = None;
    let mut camf_type = None;
    let mut camf_bytes = None;
    let mut widths = [None; 3];
    let mut heights = [None; 3];
    let mut sources = [None, None, None];
    let mut components = [None; 3];

    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("malformed manifest line: {line}"))?;
        if key.is_empty() || value.is_empty() || value.contains('=') {
            return Err(format!("malformed manifest line: {line}"));
        }
        match key {
            "schema" => set_once(&mut schema, parse_u32(value, key)?, key)?,
            "decoder" => set_once(&mut decoder, value.to_owned(), key)?,
            "x3f_version" => set_once(&mut x3f_version, parse_hex(value, key)?, key)?,
            "raw_type_format" => set_once(&mut raw_type_format, parse_hex(value, key)?, key)?,
            "camf_type" => set_once(&mut camf_type, parse_u32(value, key)?, key)?,
            "camf_bytes" => set_once(&mut camf_bytes, parse_decimal(value, key)?, key)?,
            _ => {
                let (layer, field) = key
                    .strip_prefix("layer_")
                    .and_then(|key| key.split_once('_'))
                    .ok_or_else(|| format!("unknown manifest field: {key}"))?;
                let index: usize = layer
                    .parse()
                    .map_err(|_| format!("invalid layer index: {layer}"))?;
                let slot = match index {
                    0..=2 => index,
                    _ => return Err(format!("invalid layer index: {layer}")),
                };
                match field {
                    "width" => set_once(&mut widths[slot], parse_decimal(value, key)?, key)?,
                    "height" => set_once(&mut heights[slot], parse_decimal(value, key)?, key)?,
                    "source" => set_once(&mut sources[slot], value.to_owned(), key)?,
                    "component" => set_once(&mut components[slot], parse_u32(value, key)?, key)?,
                    _ => return Err(format!("unknown manifest field: {key}")),
                }
            }
        }
    }

    if required(schema, "schema")? != 1 {
        return Err("unsupported manifest schema".to_owned());
    }
    if required(decoder, "decoder")? != REFERENCE_DECODER {
        return Err("unexpected reference decoder".to_owned());
    }
    let layers = [
        reference_plane(widths[0], heights[0], sources[0].take(), components[0], 0)?,
        reference_plane(widths[1], heights[1], sources[1].take(), components[1], 1)?,
        reference_plane(widths[2], heights[2], sources[2].take(), components[2], 2)?,
    ];
    Ok(ReferenceManifest {
        x3f_version: required(x3f_version, "x3f_version")?,
        raw_type_format: required(raw_type_format, "raw_type_format")?,
        camf_type: required(camf_type, "camf_type")?,
        camf_bytes: required(camf_bytes, "camf_bytes")?,
        layers,
    })
}

fn little_endian_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| "X3F metadata offset overflow".to_owned())?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| "truncated X3F metadata".to_owned())?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn sample_metadata(bytes: &[u8]) -> Result<(u32, u32, u32), String> {
    let version = little_endian_u32(bytes, 4)?;
    let directory_offset = usize::try_from(little_endian_u32(
        bytes,
        bytes
            .len()
            .checked_sub(4)
            .ok_or_else(|| "truncated X3F metadata".to_owned())?,
    )?)
    .map_err(|_| "directory offset exceeds address space".to_owned())?;
    let entry_count = usize::try_from(little_endian_u32(
        bytes,
        directory_offset
            .checked_add(8)
            .ok_or_else(|| "directory offset overflow".to_owned())?,
    )?)
    .map_err(|_| "directory entry count exceeds address space".to_owned())?;
    let mut raw_type_format = None;
    let mut camf_type = None;
    for index in 0..entry_count {
        let entry = index
            .checked_mul(12)
            .and_then(|offset| directory_offset.checked_add(12)?.checked_add(offset))
            .ok_or_else(|| "directory entry offset overflow".to_owned())?;
        let kind = bytes
            .get(
                entry
                    .checked_add(8)
                    .ok_or_else(|| "directory entry offset overflow".to_owned())?
                    ..entry
                        .checked_add(12)
                        .ok_or_else(|| "directory entry offset overflow".to_owned())?,
            )
            .ok_or_else(|| "truncated X3F metadata".to_owned())?;
        if kind == b"IMA2" || kind == b"CAMF" {
            let section_offset = usize::try_from(little_endian_u32(bytes, entry)?)
                .map_err(|_| "section offset exceeds address space".to_owned())?;
            let section_type = little_endian_u32(
                bytes,
                section_offset
                    .checked_add(8)
                    .ok_or_else(|| "section offset overflow".to_owned())?,
            )?;
            if kind == b"IMA2" && section_type == 1 {
                let raw_type = u16::try_from(section_type)
                    .map_err(|_| "RAW section type exceeds manifest range".to_owned())?;
                let raw_format = u16::try_from(little_endian_u32(
                    bytes,
                    section_offset
                        .checked_add(12)
                        .ok_or_else(|| "section offset overflow".to_owned())?,
                )?)
                .map_err(|_| "RAW section format exceeds manifest range".to_owned())?;
                set_once(
                    &mut raw_type_format,
                    (u32::from(raw_type) << 16) | u32::from(raw_format),
                    "RAW section",
                )?;
            }
            if kind == b"CAMF" {
                set_once(&mut camf_type, section_type, "CAMF section")?;
            }
        }
    }
    Ok((
        version,
        required(raw_type_format, "RAW section")?,
        required(camf_type, "CAMF section")?,
    ))
}

fn assert_plane_sources(manifest: &ReferenceManifest, sample: &Path) {
    let expected = match manifest.raw_type_format {
        0x0001_001e => [("x3rgb16", 0), ("x3rgb16", 1), ("x3rgb16", 2)],
        0x0001_0023 | 0x0001_0025 => [("x3rgb16", 0), ("x3rgb16", 1), ("top16", 0)],
        format => panic!("unsupported reference RAW format: {format:#x}"),
    };
    for (index, (plane, (source, component))) in manifest.layers.iter().zip(expected).enumerate() {
        assert_eq!(
            plane.source,
            source,
            "{}: layer {index} source",
            sample.display()
        );
        assert_eq!(
            plane.component,
            component,
            "{}: layer {index} component",
            sample.display()
        );
    }
}

fn read_fixture(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn assert_matches_independent_reference(sample: &Path, reference_directory: &Path) -> u32 {
    let bytes = read_fixture(sample);
    let manifest_path = reference_directory.join("reference.txt");
    let manifest = parse_reference_manifest(
        &std::fs::read_to_string(&manifest_path)
            .unwrap_or_else(|error| panic!("{}: {error}", manifest_path.display())),
    )
    .unwrap_or_else(|error| panic!("{}: {error}", manifest_path.display()));
    let (x3f_version, raw_type_format, camf_type) =
        sample_metadata(&bytes).unwrap_or_else(|error| panic!("{}: {error}", sample.display()));
    assert_eq!(
        manifest.x3f_version,
        x3f_version,
        "{}: X3F version",
        sample.display()
    );
    assert_eq!(
        manifest.raw_type_format,
        raw_type_format,
        "{}: RAW type format",
        sample.display()
    );
    assert_eq!(
        manifest.camf_type,
        camf_type,
        "{}: CAMF type",
        sample.display()
    );
    assert_plane_sources(&manifest, sample);
    let file = X3f::parse(&bytes).unwrap_or_else(|error| panic!("{}: {error}", sample.display()));
    let calibration_path = reference_directory.join("calibration.bin");
    let reference = read_fixture(&calibration_path);
    let calibration = file
        .camf_bytes()
        .unwrap_or_else(|error| panic!("{}: {error}", sample.display()));
    assert_eq!(
        manifest.camf_bytes,
        reference.len(),
        "{}: reference CAMF length",
        calibration_path.display()
    );
    assert_eq!(
        manifest.camf_bytes,
        calibration.len(),
        "{}: decoded CAMF length",
        sample.display()
    );
    assert_eq!(calibration, reference, "{}: CAMF", sample.display());
    let image = file
        .decode()
        .unwrap_or_else(|error| panic!("{}: {error}", sample.display()));
    for (index, (plane, metadata)) in image.layers.iter().zip(&manifest.layers).enumerate() {
        assert_eq!(
            plane.width,
            metadata.width,
            "{}: layer {index} width",
            sample.display()
        );
        assert_eq!(
            plane.height,
            metadata.height,
            "{}: layer {index} height",
            sample.display()
        );
        let reference_path = reference_directory.join(format!("layer-{index}.u16le"));
        let reference = read_fixture(&reference_path);
        assert_eq!(
            reference.len(),
            2 * plane.samples.len(),
            "{}: layer {index} byte length",
            reference_path.display()
        );
        for (pixel, (value, pair)) in plane
            .samples
            .iter()
            .zip(reference.chunks_exact(2))
            .enumerate()
        {
            assert_eq!(
                *value,
                u16::from_le_bytes([pair[0], pair[1]]),
                "{}: layer {index}, pixel {pixel}",
                sample.display()
            );
        }
    }
    raw_type_format
}

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
    fixture_parts(quattro_header(), calibration_section(), raw_section())
}

fn quattro_header() -> Vec<u8> {
    let mut file = vec![0; 40];
    file[..4].copy_from_slice(b"FOVb");
    put_u32(&mut file, 4, 0x40002);
    file
}

fn fixture_parts(file: Vec<u8>, camf: Vec<u8>, raw: Vec<u8>) -> Vec<u8> {
    fixture_sections(file, vec![(*b"CAMF", camf), (*b"IMA2", raw)])
}

fn fixture_sections(mut file: Vec<u8>, sections: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut entries = Vec::with_capacity(sections.len());
    for (kind, section) in sections {
        entries.push((kind, file.len(), section.len()));
        file.extend(section);
    }
    let directory_offset = file.len();
    let mut directory = vec![0; 16 + entries.len() * 12];
    directory[..4].copy_from_slice(b"SECd");
    put_u32(&mut directory, 4, 0x20000);
    put_u32(&mut directory, 8, entries.len());
    for (index, (kind, offset, length)) in entries.into_iter().enumerate() {
        let entry = 12 + index * 12;
        put_u32(&mut directory, entry, offset);
        put_u32(&mut directory, entry + 4, length);
        directory[entry + 8..entry + 12].copy_from_slice(&kind);
    }
    let directory_offset_slot = directory.len() - 4;
    put_u32(&mut directory, directory_offset_slot, directory_offset);
    file.extend(directory);
    file
}

fn jpeg_section(payload: &[u8]) -> Vec<u8> {
    let mut section = vec![0; 28];
    section[..4].copy_from_slice(b"SECi");
    put_u32(&mut section, 8, 2);
    put_u32(&mut section, 12, 0x12);
    section.extend(payload);
    section
}

fn merrill_raw_section() -> Vec<u8> {
    let mut section = vec![0; 100];
    section[..4].copy_from_slice(b"SECi");
    put_u32(&mut section, 4, 0x20000);
    put_u32(&mut section, 8, 1);
    put_u32(&mut section, 12, 0x1e);
    put_u32(&mut section, 16, 4);
    put_u32(&mut section, 20, 4);
    for channel in 0..3 {
        section[28 + 2 * channel] = 10 + channel as u8;
    }
    section[36] = 1;
    for offset in [40, 44, 48] {
        put_u32(&mut section, offset, 2);
    }
    section
}

fn padded_quattro_raw_section() -> Vec<u8> {
    let mut section = raw_section();
    section.resize(103, 0);
    put_u32(&mut section, 12, 0x23);
    section[36..38].copy_from_slice(&6u16.to_le_bytes());
    put_u32(&mut section, 64, 3);
    section
}

#[test]
fn rejects_tiny_synthetic_quattro_geometry() {
    let bytes = fixture();
    assert!(X3f::parse(&bytes).unwrap().decode().is_err());
}

#[test]
fn parses_canonical_reference_manifest_and_rejects_invalid_manifests() {
    let manifest = parse_reference_manifest(CANONICAL_REFERENCE_MANIFEST).unwrap();
    assert_eq!(manifest.raw_type_format, 0x0001_0023);
    assert_eq!(manifest.layers[2].source, "top16");

    let failures = [
        ("malformed", "schema".to_owned()),
        (
            "duplicate",
            format!("{CANONICAL_REFERENCE_MANIFEST}schema=1\n"),
        ),
        (
            "unknown",
            format!("{CANONICAL_REFERENCE_MANIFEST}unknown=field\n"),
        ),
        (
            "missing",
            CANONICAL_REFERENCE_MANIFEST.replace("camf_bytes=12\n", ""),
        ),
        (
            "wrong decoder",
            CANONICAL_REFERENCE_MANIFEST
                .replace(&format!("decoder={REFERENCE_DECODER}"), "decoder=other"),
        ),
        (
            "unsupported schema",
            CANONICAL_REFERENCE_MANIFEST.replace("schema=1", "schema=2"),
        ),
    ];
    for (name, manifest) in failures {
        assert!(parse_reference_manifest(&manifest).is_err(), "{name}");
    }
}

#[test]
fn accepts_merrill_container_versions_but_rejects_tiny_geometry() {
    for version in [0x30000, 0x30001] {
        let mut header = vec![0; 40];
        header[..4].copy_from_slice(b"FOVb");
        put_u32(&mut header, 4, version);
        let bytes = fixture_parts(header, calibration_section(), merrill_raw_section());
        let file = X3f::parse(&bytes).unwrap();
        assert_eq!(file.sensor_format().unwrap(), SensorFormat::Merrill);
        assert!(file.decode().is_err());
    }
}

#[test]
fn rejects_tiny_quattro_top_plane_geometry() {
    let mut header = vec![0; 40];
    header[..4].copy_from_slice(b"FOVb");
    put_u32(&mut header, 4, 0x40001);
    let bytes = fixture_parts(header, calibration_section(), padded_quattro_raw_section());
    let file = X3f::parse(&bytes).unwrap();
    assert_eq!(file.sensor_format().unwrap(), SensorFormat::Quattro);
    assert!(file.decode().is_err());
}

#[test]
fn decodes_camf_predictor_and_text_entry() {
    let bytes = fixture();
    let file = X3f::parse(&bytes).unwrap();
    let metadata = file.calibration().unwrap();
    assert!(
        matches!(metadata.entries.get("Tag"), Some(Entry::Text(text)) if text == "hello world")
    );
    assert_eq!(file.camf_bytes().unwrap(), metadata.decoded_bytes());
}

#[test]
fn rejects_camf_types_that_do_not_match_the_sensor_format() {
    let mut quattro_camf = calibration_section();
    put_u32(&mut quattro_camf, 8, 4);
    let quattro_bytes = fixture_parts(quattro_header(), quattro_camf, raw_section());
    let quattro = X3f::parse(&quattro_bytes).unwrap();
    assert!(quattro.camf_bytes().is_err());
    assert!(quattro.calibration().is_err());

    let mut merrill_header = vec![0; 40];
    merrill_header[..4].copy_from_slice(b"FOVb");
    put_u32(&mut merrill_header, 4, 0x30000);
    let merrill_bytes = fixture_parts(merrill_header, calibration_section(), merrill_raw_section());
    let merrill = X3f::parse(&merrill_bytes).unwrap();
    assert!(merrill.camf_bytes().is_err());
    assert!(merrill.calibration().is_err());
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
fn selects_the_first_jpeg_section_and_rejects_duplicate_raw_or_camf_sections() {
    let first_jpeg = b"\xff\xd8first";
    let bytes = fixture_sections(
        quattro_header(),
        vec![
            (*b"CAMF", calibration_section()),
            (*b"IMA2", raw_section()),
            (*b"IMA2", jpeg_section(first_jpeg)),
            (*b"IMA2", jpeg_section(b"\xff\xd8second")),
        ],
    );
    assert_eq!(
        X3f::parse(&bytes).unwrap().preview(),
        Some(first_jpeg.as_slice())
    );

    let duplicate_raw = fixture_sections(
        quattro_header(),
        vec![
            (*b"CAMF", calibration_section()),
            (*b"IMA2", raw_section()),
            (*b"IMA2", raw_section()),
        ],
    );
    assert_eq!(
        X3f::parse(&duplicate_raw).err().unwrap().to_string(),
        "multiple RAW sections"
    );

    let duplicate_camf = fixture_sections(
        quattro_header(),
        vec![
            (*b"CAMF", calibration_section()),
            (*b"CAMF", calibration_section()),
            (*b"IMA2", raw_section()),
        ],
    );
    assert_eq!(
        X3f::parse(&duplicate_camf).err().unwrap().to_string(),
        "multiple CAMF sections"
    );
}

#[test]
fn rejects_unsupported_raw_format() {
    let mut bytes = fixture();
    let raw_offset = 40 + calibration_section().len();
    put_u32(&mut bytes, raw_offset + 12, 0x99);
    assert!(X3f::parse(&bytes).unwrap().decode().is_err());
}

#[test]
fn rejects_unobserved_container_versions_and_format_combinations() {
    let mut version = fixture();
    put_u32(&mut version, 4, 0x40000);
    assert!(X3f::parse(&version).is_err());
    let mut combination = fixture();
    put_u32(&mut combination, 4, 0x40001);
    assert!(X3f::parse(&combination).unwrap().decode().is_err());
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
#[ignore = "requires X3F_SAMPLE and X3F_REFERENCE_DIR emitted by the pinned X3F Tools decoder"]
fn sample_matches_independent_reference_byte_for_byte() {
    let sample = PathBuf::from(std::env::var_os("X3F_SAMPLE").expect("X3F_SAMPLE"));
    let reference_directory =
        PathBuf::from(std::env::var_os("X3F_REFERENCE_DIR").expect("X3F_REFERENCE_DIR"));
    let _ = assert_matches_independent_reference(&sample, &reference_directory);
}

#[test]
#[ignore = "requires X3F_SAMPLE_DIR and X3F_REFERENCE_DIR containing the independent reference corpus"]
fn corpus_matches_independent_references_byte_for_byte() {
    let sample_directory =
        PathBuf::from(std::env::var_os("X3F_SAMPLE_DIR").expect("X3F_SAMPLE_DIR"));
    let reference_root =
        PathBuf::from(std::env::var_os("X3F_REFERENCE_DIR").expect("X3F_REFERENCE_DIR"));
    let mut samples: Vec<_> = std::fs::read_dir(&sample_directory)
        .unwrap_or_else(|error| panic!("{}: {error}", sample_directory.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("{}: {error}", sample_directory.display()))
                .path()
        })
        .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("x3f"))
        .collect();
    samples.sort();
    assert_eq!(
        samples.len(),
        164,
        "{}: X3F file count",
        sample_directory.display()
    );
    let mut counts = [0usize; 3];
    for sample in samples {
        let stem = sample
            .file_stem()
            .unwrap_or_else(|| panic!("{}: missing filename stem", sample.display()));
        match assert_matches_independent_reference(&sample, &reference_root.join(stem)) {
            0x0001_001e => counts[0] += 1,
            0x0001_0023 => counts[1] += 1,
            0x0001_0025 => counts[2] += 1,
            format => panic!(
                "{}: unexpected RAW type format {format:#010x}",
                sample.display()
            ),
        }
    }
    assert_eq!(counts, [59, 100, 5]);
}

#[test]
#[ignore = "requires X3F_SAMPLE_DIR containing supported X3F files"]
fn corpus_decodes_supported_sensor_layouts() {
    let directory =
        std::path::PathBuf::from(std::env::var_os("X3F_SAMPLE_DIR").expect("X3F_SAMPLE_DIR"));
    let mut counts = [0usize; 3];
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("x3f") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let file = X3f::parse(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let format = file
            .sensor_format()
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let image = file
            .decode()
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let camf = file
            .camf_bytes()
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let _calibration = file
            .calibration()
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(!camf.is_empty(), "{}: empty CAMF", path.display());
        assert!(
            image.layers.iter().all(|plane| !plane.samples.is_empty()),
            "{}: empty sensor plane",
            path.display()
        );
        match format {
            SensorFormat::Merrill => {
                assert!(
                    image
                        .layers
                        .iter()
                        .all(|plane| (plane.width, plane.height) == (4928, 3264)),
                    "{}: unexpected Merrill geometry",
                    path.display()
                );
                counts[0] += 1;
            }
            SensorFormat::Quattro => {
                assert_eq!(
                    (image.layers[0].width, image.layers[0].height),
                    (2944, 1836),
                    "{}: unexpected dp3 lower geometry",
                    path.display()
                );
                assert_eq!(
                    (image.layers[1].width, image.layers[1].height),
                    (2944, 1836),
                    "{}: unexpected dp3 middle geometry",
                    path.display()
                );
                assert_eq!(
                    (image.layers[2].width, image.layers[2].height),
                    (6272, 3672),
                    "{}: unexpected dp3 top geometry",
                    path.display()
                );
                counts[1] += 1;
            }
            SensorFormat::SdQuattro => {
                assert_eq!(
                    image.layers[2].width,
                    image.layers[0].width * 2,
                    "{}: unexpected sd Quattro width",
                    path.display()
                );
                assert_eq!(
                    image.layers[2].height,
                    image.layers[0].height * 2,
                    "{}: unexpected sd Quattro height",
                    path.display()
                );
                counts[2] += 1;
            }
        }
    }
    assert_eq!(counts, [59, 100, 5]);
    eprintln!(
        "corpus passed: Merrill={}, Quattro={}, sd Quattro={}",
        counts[0], counts[1], counts[2]
    );
}
