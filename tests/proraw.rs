// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use rawdinal::{Dng, LinearRawProcessing};

#[path = "support/proraw_reference.rs"]
mod reference;

fn checksum(samples: &[f32]) -> u64 {
    samples
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, sample| {
            (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

#[test]
#[ignore = "requires PRORAW_SAMPLE pointing to a supported Apple ProRAW DNG"]
fn decodes_proraw_sample() {
    let path = std::path::PathBuf::from(std::env::var_os("PRORAW_SAMPLE").expect("PRORAW_SAMPLE"));
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let image = Dng::parse(&bytes)
        .and_then(|dng| dng.decode())
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert_eq!((image.width, image.height, image.channels), (4032, 3024, 3));
    assert!(image.samples.iter().all(|sample| sample.is_finite()));
    assert_eq!(
        image.processing.profile_gain_table_map,
        LinearRawProcessing::Unapplied
    );
    assert_eq!(
        image.profile_gain_table_map.as_ref().unwrap().len(),
        3_158_080
    );
    let checksum = checksum(&image.samples);
    assert_eq!(checksum, 0xdefd_c044_2475_559c);
    eprintln!(
        "{}: {}x{}x{}, samples={}, checksum={checksum:016x}",
        path.display(),
        image.width,
        image.height,
        image.channels,
        image.samples.len(),
    );
}

#[test]
#[ignore = "requires PRORAW_CORPUS pointing to a directory of supported Apple ProRAW DNG files"]
fn decodes_proraw_corpus() {
    let directory =
        std::path::PathBuf::from(std::env::var_os("PRORAW_CORPUS").expect("PRORAW_CORPUS"));
    let mut paths = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
                .path()
        })
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dng"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    assert!(
        !paths.is_empty(),
        "{} has no DNG files",
        directory.display()
    );
    for path in paths {
        let bytes =
            std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let image = Dng::parse(&bytes)
            .and_then(|dng| dng.decode_with_options(Default::default()))
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(image.channels, 3, "{}", path.display());
        assert!(!image.samples.is_empty(), "{}", path.display());
        assert!(
            image.samples.iter().all(|sample| sample.is_finite()),
            "{}",
            path.display()
        );
        assert!(matches!(
            image.processing.linearization,
            LinearRawProcessing::Applied | LinearRawProcessing::NotPresent
        ));
        assert_eq!(
            image.processing.black_subtraction,
            LinearRawProcessing::Applied
        );
        assert_eq!(
            image.processing.white_normalization,
            LinearRawProcessing::Applied
        );
        assert_eq!(image.processing.demosaic, LinearRawProcessing::NotPresent);
        for state in [
            image.processing.opcode_list_1,
            image.processing.opcode_list_2,
            image.processing.opcode_list_3,
        ] {
            assert!(matches!(
                state,
                LinearRawProcessing::Applied
                    | LinearRawProcessing::NotPresent
                    | LinearRawProcessing::SkippedOptional
                    | LinearRawProcessing::PartiallyApplied
            ));
        }
        eprintln!(
            "{}: checksum={:016x}",
            path.display(),
            checksum(&image.samples)
        );
    }
}

#[test]
#[ignore = "requires PRORAW_SAMPLE and schema-2 PRORAW_REFERENCE_DIR"]
fn assembled_normalized_image_matches_formula_reference() {
    let source = std::fs::read(std::env::var_os("PRORAW_SAMPLE").unwrap()).unwrap();
    let directory = std::path::PathBuf::from(std::env::var_os("PRORAW_REFERENCE_DIR").unwrap());
    let provenance = reference::verified_manifest(&source, &directory);
    let image = Dng::parse(&source)
        .unwrap()
        .decode_with_options(rawdinal::DngDecodeOptions {
            retain_encoded_samples: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(provenance["width"], image.width);
    assert_eq!(provenance["height"], image.height);
    assert_eq!(provenance["raw_ifd_offset"], image.metadata.raw_offset);
    let encoded = std::fs::read(directory.join("encoded.u16le")).unwrap();
    let codes = image.encoded_samples.as_ref().unwrap();
    assert_eq!(encoded.len(), codes.len() * 2);
    for (actual, bytes) in codes.iter().zip(encoded.chunks_exact(2)) {
        assert_eq!(*actual, u16::from_le_bytes(bytes.try_into().unwrap()));
    }
    let normalized = std::fs::read(directory.join("normalized.f32le")).unwrap();
    assert_eq!(normalized.len(), image.samples.len() * 4);
    let mut max_ulps = 0;
    for (&actual, bytes) in image.samples.iter().zip(normalized.chunks_exact(4)) {
        let expected = f32::from_le_bytes(bytes.try_into().unwrap());
        assert!(actual.is_finite() && expected.is_finite());
        let ulps = actual.to_bits().abs_diff(expected.to_bits());
        max_ulps = max_ulps.max(ulps);
        assert!(ulps <= 2, "{actual} != {expected}: {ulps} ULPs");
    }
    eprintln!(
        "{} assembled samples match; maximum normalization difference {max_ulps} ULPs",
        image.samples.len()
    );
}

#[test]
#[ignore = "requires PRORAW_SAMPLE with profile gain table and semantic masks"]
fn decodes_proraw_auxiliary_metadata() {
    let source = std::fs::read(std::env::var_os("PRORAW_SAMPLE").unwrap()).unwrap();
    let dng = Dng::parse(&source).unwrap();
    assert!(dng.jpeg_preview().unwrap().is_some());
    let masks = dng.decode_semantic_masks().unwrap();
    assert!(!masks.is_empty());
    for mask in &masks {
        assert_eq!(
            mask.samples.len(),
            mask.width as usize * mask.height as usize
        );
    }
    let image = dng.decode_with_options(Default::default()).unwrap();
    let profile = rawdinal::ProfileGainTable::parse(
        image.metadata.raw().tag(52525).unwrap(),
        image.tiff_byte_order,
    )
    .unwrap();
    assert!(profile.gain([0.18; 3], [0.5; 2], 1.0).unwrap().is_finite());
    assert_eq!(
        image
            .metadata
            .calibration()
            .unwrap()
            .as_shot_neutral
            .unwrap()
            .len(),
        3
    );
    eprintln!(
        "{} masks; gain-table dimensions {:?}; scoped camera calibration available",
        masks.len(),
        profile.dimensions()
    );
}
