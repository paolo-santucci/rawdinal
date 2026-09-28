// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

use rawdinal::{Dng, LinearRawProcessing};

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
            .and_then(|dng| dng.decode())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(image.channels, 3, "{}", path.display());
        assert!(!image.samples.is_empty(), "{}", path.display());
        assert!(
            image.samples.iter().all(|sample| sample.is_finite()),
            "{}",
            path.display()
        );
        assert_eq!(image.processing.linearization, LinearRawProcessing::Applied);
        assert_eq!(
            image.processing.black_subtraction,
            LinearRawProcessing::Applied
        );
        assert_eq!(
            image.processing.white_normalization,
            LinearRawProcessing::Applied
        );
        assert_eq!(image.processing.demosaic, LinearRawProcessing::NotPresent);
        assert_eq!(
            image.processing.opcode_list_1,
            LinearRawProcessing::NotPresent
        );
        assert_eq!(
            image.processing.opcode_list_2,
            LinearRawProcessing::NotPresent
        );
        assert_eq!(
            image.processing.opcode_list_3,
            LinearRawProcessing::NotPresent
        );
        eprintln!(
            "{}: checksum={:016x}",
            path.display(),
            checksum(&image.samples)
        );
    }
}
