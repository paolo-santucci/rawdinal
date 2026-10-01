#[path = "support/dng.rs"]
mod fixture;

use fixture::{Fixture, floats, hex, longs, rationals, shorts};
use rawdinal::{
    ByteOrder, DecodeError, DecodeLimits, Dng, DngDecodeOptions, DngTag, LinearRawProcessing,
    ProfileGainTable,
};

fn decode(fixture: Fixture) -> rawdinal::DngDecodedImage {
    Dng::parse(&fixture.build())
        .unwrap()
        .decode_with_options(Default::default())
        .unwrap()
}

#[test]
fn integer_defaults_and_original_codes_are_preserved() {
    let input = [0, 1, 65535, 12345, 32768, 65000];
    let bytes = Fixture::rgb(2, 1, &input).build();
    let image = Dng::parse(&bytes)
        .unwrap()
        .decode_with_options(DngDecodeOptions {
            retain_encoded_samples: true,
            retain_sample_flags: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(image.encoded_samples.as_deref(), Some(input.as_slice()));
    assert_eq!(image.sample_flags.as_deref().unwrap(), [0, 0, 3, 0, 0, 0]);
    assert_eq!(
        image.processing.linearization,
        LinearRawProcessing::NotPresent
    );
    assert_eq!(image.samples, input.map(|value| f32::from(value) / 65535.0));
}

#[test]
fn repeated_black_and_negative_deltas_use_global_channel_denominators() {
    let mut fixture = Fixture::rgb(2, 2, &[50; 12]);
    fixture.set(50713, 3, shorts(&[2, 1]));
    fixture.set(
        50714,
        5,
        rationals(&[(10, 1), (20, 1), (30, 1), (20, 1), (30, 1), (40, 1)]),
    );
    fixture.set(50715, 10, rationals(&[(-2, 1), (-4, 1)]));
    fixture.set(50716, 10, rationals(&[(-1, 1), (-3, 1)]));
    fixture.set(50717, 4, longs(&[100; 3]));
    let image = decode(fixture);
    let expected = [
        43.0 / 85.0,
        33.0 / 75.0,
        23.0 / 65.0,
        45.0 / 85.0,
        35.0 / 75.0,
        25.0 / 65.0,
        35.0 / 85.0,
        25.0 / 75.0,
        15.0 / 65.0,
        37.0 / 85.0,
        27.0 / 75.0,
        17.0 / 65.0,
    ];
    assert_eq!(image.samples, expected);
}

#[test]
fn active_area_offsets_black_deltas_and_marks_masked_samples() {
    let mut fixture = Fixture::rgb(3, 2, &[100; 18]);
    fixture.set(50829, 4, longs(&[1, 1, 2, 3]));
    fixture.set(50715, 10, rationals(&[(10, 1), (20, 1)]));
    fixture.set(50717, 4, longs(&[200; 3]));
    let image = Dng::parse(&fixture.build())
        .unwrap()
        .decode_with_options(DngDecodeOptions {
            retain_sample_flags: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        &image.samples[12..],
        &[0.5, 0.5, 0.5, 80.0 / 180.0, 80.0 / 180.0, 80.0 / 180.0]
    );
    assert_eq!(
        image.sample_flags.unwrap(),
        [4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn rational_black_fractional_crop_and_scoped_calibration_survive() {
    let mut fixture = Fixture::rgb(2, 2, &[25; 12]);
    fixture.set(50714, 5, rationals(&[(1, 2); 3]));
    fixture.set(50717, 4, longs(&[100; 3]));
    fixture.set(50719, 5, rationals(&[(1, 2), (0, 3)]));
    fixture.set(50720, 5, rationals(&[(3, 2), (4, 2)]));
    fixture.set(50728, 5, rationals(&[(1, 2), (1, 1), (2, 3)]));
    fixture.set(
        52531,
        10,
        rationals(&[
            (1, 1),
            (0, 1),
            (0, 1),
            (0, 1),
            (1, 1),
            (0, 1),
            (0, 1),
            (0, 1),
            (1, 1),
        ]),
    );
    let image = decode(fixture);
    assert_eq!(image.samples, vec![(24.5 / 99.5) as f32; 12]);
    assert_eq!(image.default_crop_origin, None);
    assert_eq!(image.default_crop_origin_exact, Some([0.5, 0.0]));
    assert_eq!(image.default_crop_size_exact, Some([1.5, 2.0]));
    let calibration = image.metadata.calibration().unwrap();
    assert_eq!(calibration.as_shot_neutral, Some(vec![0.5, 1.0, 2.0 / 3.0]));
    assert_eq!(
        calibration.color_matrices[2],
        Some(vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
    );
}

#[test]
fn short_lut_repeats_last_entry_without_changing_default_white() {
    let mut fixture = Fixture::rgb(1, 1, &[0, 1, 100]);
    fixture.set(50712, 3, shorts(&[100, 500]));
    assert_eq!(
        decode(fixture).samples,
        [100.0 / 65535.0, 500.0 / 65535.0, 500.0 / 65535.0]
    );
}

#[test]
fn strips_and_planar_channels_assemble_without_edge_padding() {
    let mut fixture = Fixture::rgb(2, 3, &[]);
    fixture.tags.remove(&322);
    fixture.tags.remove(&323);
    fixture.set(278, 4, longs(&[2]));
    fixture.set(284, 3, shorts(&[2]));
    fixture.blocks = vec![
        shorts(&[1, 2, 3, 4]),
        shorts(&[5, 6]),
        shorts(&[11, 12, 13, 14]),
        shorts(&[15, 16]),
        shorts(&[21, 22, 23, 24]),
        shorts(&[25, 26]),
    ];
    let expected = [
        1, 11, 21, 2, 12, 22, 3, 13, 23, 4, 14, 24, 5, 15, 25, 6, 16, 26,
    ]
    .map(|v| v as f32 / 65535.0);
    assert_eq!(decode(fixture).samples, expected);
}

#[test]
fn packed_ten_bit_rows_restart_at_byte_boundaries() {
    let mut fixture = Fixture::rgb(1, 2, &[]);
    fixture.set(258, 3, shorts(&[10; 3]));
    fixture.blocks = vec![vec![0, 0, 0x1f, 0xfc, 0xff, 0xc0, 0x10, 0x08]];
    assert_eq!(
        decode(fixture).samples,
        [0.0, 1.0 / 1023.0, 1.0, 1.0, 1.0 / 1023.0, 2.0 / 1023.0]
    );
}

fn opcode(id: u32, flags: u32, payload: &[u8]) -> Vec<u8> {
    [
        1u32.to_be_bytes().as_slice(),
        &id.to_be_bytes(),
        &0x01030000u32.to_be_bytes(),
        &flags.to_be_bytes(),
        &(payload.len() as u32).to_be_bytes(),
        payload,
    ]
    .concat()
}

fn area(bounds: [u32; 4], plane: u32, planes: u32, pitch: [u32; 2]) -> Vec<u8> {
    [bounds.as_slice(), &[plane, planes], &pitch]
        .concat()
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect()
}

#[test]
fn unknown_optional_opcodes_skip_but_mandatory_and_malformed_fail() {
    let mut fixture = Fixture::rgb(1, 1, &[1, 2, 3]);
    fixture.set(51008, 7, opcode(999, 1, &[]));
    assert_eq!(
        decode(fixture).processing.opcode_list_1,
        LinearRawProcessing::SkippedOptional
    );
    for flags in [0, 2] {
        let mut fixture = Fixture::rgb(1, 1, &[1, 2, 3]);
        fixture.set(51008, 7, opcode(999, flags, &[]));
        assert!(matches!(
            Dng::parse(&fixture.build()),
            Err(DecodeError::UnsupportedFeature)
        ));
    }
    let mut fixture = Fixture::rgb(1, 1, &[1, 2, 3]);
    fixture.set(51008, 7, vec![0, 0, 0, 1]);
    assert!(matches!(
        Dng::parse(&fixture.build()),
        Err(DecodeError::Truncated)
    ));
}

#[test]
fn map_table_precedes_lut_and_diagnostics_retain_original_codes() {
    let mut fixture = Fixture::rgb(1, 1, &[0, 1, 2]);
    fixture.set(50712, 3, shorts(&[0, 10, 100]));
    fixture.set(50717, 4, longs(&[100; 3]));
    let mut payload = area([0, 0, 1, 1], 0, 3, [1, 1]);
    payload.extend(3u32.to_be_bytes());
    payload.extend([2u16, 1, 0].into_iter().flat_map(u16::to_be_bytes));
    fixture.set(51008, 7, opcode(7, 0, &payload));
    let image = Dng::parse(&fixture.build())
        .unwrap()
        .decode_with_options(DngDecodeOptions {
            retain_encoded_samples: true,
            retain_sample_flags: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(image.samples, [1.0, 0.1, 0.0]);
    assert_eq!(image.encoded_samples.unwrap(), [0, 1, 2]);
    assert_eq!(image.sample_flags.unwrap(), [8, 8, 10]);
}

#[test]
fn gain_map_interpolates_at_pixel_centers_in_active_coordinates() {
    let mut fixture = Fixture::rgb(2, 2, &[100; 12]);
    fixture.set(50717, 4, longs(&[1000; 3]));
    let mut payload = area([0, 0, 2, 2], 0, 3, [1, 1]);
    payload.extend([2u32, 2].into_iter().flat_map(u32::to_be_bytes));
    payload.extend(
        [0.5f64, 0.5, 0.25, 0.25]
            .into_iter()
            .flat_map(f64::to_be_bytes),
    );
    payload.extend(1u32.to_be_bytes());
    payload.extend(
        [1.0f32, 2.0, 3.0, 4.0]
            .into_iter()
            .flat_map(f32::to_be_bytes),
    );
    fixture.set(51009, 7, opcode(9, 0, &payload));
    let image = decode(fixture);
    for (actual, expected) in image.samples.iter().zip([
        0.1f32, 0.1, 0.1, 0.2, 0.2, 0.2, 0.3, 0.3, 0.3, 0.4, 0.4, 0.4,
    ]) {
        assert!((actual - expected).abs() < 1e-7);
    }
    assert_eq!(image.processing.opcode_list_2, LinearRawProcessing::Applied);
}

#[test]
fn row_scale_pitch_and_stage_three_trim_preserve_stored_geometry() {
    let mut fixture = Fixture::rgb(2, 3, &[100; 18]);
    fixture.set(50717, 4, longs(&[1000; 3]));
    let mut payload = area([0, 0, 3, 2], 1, 1, [2, 1]);
    payload.extend(2u32.to_be_bytes());
    payload.extend([2.0f32, 3.0].into_iter().flat_map(f32::to_be_bytes));
    fixture.set(51009, 7, opcode(12, 0, &payload));
    fixture.set(
        51022,
        7,
        opcode(
            6,
            0,
            &[1u32, 0, 3, 2]
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect::<Vec<_>>(),
        ),
    );
    let image = decode(fixture);
    assert_eq!(
        (image.width, image.height, image.valid_area),
        (2, 3, [1, 0, 3, 2])
    );
    assert_eq!(image.samples[1], 0.2);
    assert_eq!(image.samples[7], 0.1);
    assert!((image.samples[13] - 0.3).abs() < 1e-7);
}

#[test]
fn semantic_mask_uses_its_own_sample_domain_and_mapping() {
    let raw = Fixture::rgb(2, 2, &[100; 12]);
    let mut mask = Fixture::rgb(2, 1, &[]);
    for tag in [271, 50706, 50707] {
        mask.tags.remove(&tag);
    }
    mask.set(254, 4, longs(&[65540]));
    mask.set(258, 3, shorts(&[8]));
    mask.set(262, 3, shorts(&[52527]));
    mask.set(277, 3, shorts(&[1]));
    mask.set(52526, 2, b"sky\0".to_vec());
    mask.set(52528, 2, b"one\0".to_vec());
    mask.set(52536, 4, longs(&[2, 3, 10, 8]));
    mask.blocks = vec![vec![0, 255]];
    let bytes = fixture::build(vec![raw, mask]);
    let masks = Dng::parse(&bytes).unwrap().decode_semantic_masks().unwrap();
    assert_eq!(masks[0].samples, [0, 255]);
    assert_eq!(masks[0].name.as_deref(), Some("sky"));
    assert_eq!(masks[0].sub_area, Some([2, 3, 10, 8]));
}

fn profile_tag() -> DngTag {
    let mut bytes = longs(&[1, 1]);
    bytes.extend(
        [1.0f64, 1.0, 0.0, 0.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    bytes.extend(longs(&[2]));
    bytes.extend(floats(&[1.0, 0.0, 0.0, 0.0, 0.0]));
    bytes.extend(floats(&[1.0, 3.0]));
    DngTag {
        id: 52525,
        field_type: 7,
        count: bytes.len() as u32,
        data: bytes,
    }
}

#[test]
fn profile_gain_table_uses_n_indexing_and_exposure_weight() {
    let profile = ProfileGainTable::parse(&profile_tag(), ByteOrder::LittleEndian).unwrap();
    assert_eq!(
        profile.gain([0.25, 0.0, 0.0], [0.5, 0.5], 1.0).unwrap(),
        2.0
    );
    assert_eq!(
        profile.gain([0.25, 0.0, 0.0], [0.5, 0.5], 2.0).unwrap(),
        3.0
    );
    assert_eq!(profile.gain([1.0, 0.0, 0.0], [0.5, 0.5], 1.0).unwrap(), 3.0);
    assert!(profile.gain([f64::NAN; 3], [0.5, 0.5], 1.0).is_err());
}

#[test]
fn profile_payload_truncations_and_invalid_dimensions_are_rejected() {
    let tag = profile_tag();
    for length in 0..tag.data.len() {
        let mut truncated = tag.clone();
        truncated.data.truncate(length);
        assert!(ProfileGainTable::parse(&truncated, ByteOrder::LittleEndian).is_err());
    }
    let mut oversized = tag;
    oversized.data[..8].fill(255);
    assert!(ProfileGainTable::parse(&oversized, ByteOrder::LittleEndian).is_err());
}

#[test]
fn lossless_jpeg_xl_matches_libjxl_eight_and_sixteen_bit_vectors() {
    for (bits, data, expected) in [
        (
            8,
            "ff0a08101009080401007c004b188b159258c4ede881d9010ec85f20080032bf404ddb9ba44e70bf0e4402",
            vec![0, 128, 255, 255, 64, 0, 10, 20, 30, 200, 150, 100],
        ),
        (
            16,
            "0000000c4a584c200d0a870a00000014667479706a786c20000000006a786c20000000096a786c6c0a000000436a786c63ff0a0810fc00142d0208040100b0004b188b150a2de176f400806f7edc17080220d0e7f94f0518cf620a0c54f0cc4d6835d91dd1c534ebf803cc01",
            vec![
                0, 32768, 65535, 65535, 12345, 0, 1000, 2000, 3000, 60000, 40000, 20000,
            ],
        ),
    ] {
        let mut fixture = Fixture::rgb(2, 2, &[]);
        fixture.set(258, 3, shorts(&[bits; 3]));
        fixture.set(259, 3, shorts(&[52546]));
        fixture.blocks = vec![hex(data)];
        let image = Dng::parse(&fixture.build())
            .unwrap()
            .decode_with_options(DngDecodeOptions {
                retain_encoded_samples: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(image.encoded_samples.unwrap(), expected);
    }
}

#[test]
fn metadata_and_decoded_sample_limits_are_enforced() {
    let bytes = Fixture::rgb(2, 2, &[0; 12]).build();
    assert!(matches!(
        Dng::parse_with_limits(&bytes, DecodeLimits::default().with_max_metadata_bytes(8)),
        Err(DecodeError::ResourceLimit)
    ));
    assert!(matches!(
        Dng::parse_with_limits(&bytes, DecodeLimits::default().with_max_decoded_samples(11)),
        Err(DecodeError::ResourceLimit)
    ));
}

#[test]
fn lossy_jpeg_matches_independent_libjpeg_vector() {
    let mut fixture = Fixture::rgb(2, 2, &[]);
    fixture.set(258, 3, shorts(&[8; 3]));
    fixture.set(259, 3, shorts(&[34892]));
    fixture.blocks = vec![hex(concat!(
        "ffd8ffe000104a46494600010100000100010000ffdb00430001010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101",
        "ffdb00430101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101",
        "ffc00011080002000203011100021101031101ffc40014000100000000000000000000000000000007ffc4001b10000300030101000000000000000000000304050607080201ffc40014010100000000000000000000000000000008ffc4001a110003010101010000000000000000000003040506070201",
        "ffda000c03010002110311003f004ee52c5f1ac839739b2f5ec7615bb96f4169daf66cd7913e955af5696bcc71ca34e9d1717338fd07dc319a75d68c565a64a539ca4293dfbfa0ee951e45ae8dbfb3665cead5eb6db554ead5a68acfd2a749fbafb4f50a0f3422b4ebceb452b2db6c948760e4218c4f64f7ebd7d7e736e99d1f3bceb039fcff0040dbc2830b15968f12247d5de99223c8990904a6ca953527c09cf9b3d30054451502155454225d710c43f1e3e7ffd9"
    ))];
    let image = Dng::parse(&fixture.build())
        .unwrap()
        .decode_with_options(DngDecodeOptions {
            retain_encoded_samples: true,
            ..Default::default()
        })
        .unwrap();
    let expected = [0i32, 128, 255, 255, 63, 1, 10, 20, 30, 199, 151, 102];
    for (&actual, expected) in image.encoded_samples.unwrap().iter().zip(expected) {
        assert!((i32::from(actual) - expected).abs() <= 1);
    }
}

#[test]
fn polynomial_and_delta_opcodes_preserve_order_and_signed_values() {
    let mut fixture = Fixture::rgb(2, 1, &[0, 50, 100, 0, 50, 100]);
    fixture.set(50717, 4, longs(&[100; 3]));
    let mut polynomial = area([0, 0, 1, 2], 0, 3, [1, 1]);
    polynomial.extend(1u32.to_be_bytes());
    polynomial.extend([0.0f64, 0.5].into_iter().flat_map(f64::to_be_bytes));
    let mut delta = area([0, 0, 1, 2], 0, 3, [1, 1]);
    delta.extend(2u32.to_be_bytes());
    delta.extend([-0.25f32, 0.25].into_iter().flat_map(f32::to_be_bytes));
    fixture.set(51009, 7, opcode(8, 0, &polynomial));
    fixture.set(51022, 7, opcode(11, 0, &delta));
    assert_eq!(decode(fixture).samples, [-0.25, 0.0, 0.25, 0.25, 0.5, 0.75]);
}

#[test]
fn mixed_opcode_lists_report_partial_application() {
    let mut fixture = Fixture::rgb(1, 1, &[1, 2, 3]);
    let mut payload = area([0, 0, 1, 1], 0, 3, [1, 1]);
    payload.extend(1u32.to_be_bytes());
    payload.extend([0.0f64, 1.0].into_iter().flat_map(f64::to_be_bytes));
    let supported = opcode(8, 0, &payload);
    let unknown = opcode(999, 1, &[]);
    let list = [
        2u32.to_be_bytes().as_slice(),
        &supported[4..],
        &unknown[4..],
    ]
    .concat();
    fixture.set(51009, 7, list);
    assert_eq!(
        decode(fixture).processing.opcode_list_2,
        LinearRawProcessing::PartiallyApplied
    );
}

#[test]
fn gain_table_version_two_decodes_integer_range_and_gamma() {
    let original = profile_tag();
    let mut bytes = original.data[..64].to_vec();
    bytes.extend(longs(&[0]));
    bytes.extend(floats(&[2.0, 1.0, 3.0]));
    bytes.extend([0, 255]);
    let tag = DngTag {
        id: 52544,
        field_type: 7,
        count: bytes.len() as u32,
        data: bytes,
    };
    let table = ProfileGainTable::parse(&tag, ByteOrder::LittleEndian).unwrap();
    assert_eq!(table.gain([0.5, 0.0, 0.0], [0.5, 0.5], 1.0).unwrap(), 2.0);
}

#[test]
fn truncated_and_mutated_extended_inputs_never_panic() {
    let bytes = Fixture::rgb(2, 2, &[1; 12]).build();
    for end in 0..bytes.len() {
        assert!(
            std::panic::catch_unwind(|| Dng::parse(&bytes[..end]).and_then(|dng| dng.decode()))
                .is_ok()
        );
    }
    for offset in 0..bytes.len() {
        let mut mutated = bytes.clone();
        mutated[offset] ^= 0xff;
        assert!(
            std::panic::catch_unwind(|| Dng::parse(&mutated).and_then(|dng| dng.decode())).is_ok()
        );
    }
}

#[test]
#[ignore = "allocates a synthetic 48 MP image; not real-device qualification"]
fn decodes_synthetic_48_megapixel_single_strip() {
    let (width, height) = (8064u32, 6048u32);
    let mut fixture = Fixture::rgb(width, height, &[]);
    fixture.set(258, 3, shorts(&[8; 3]));
    fixture.tags.remove(&322);
    fixture.tags.remove(&323);
    fixture.set(278, 4, longs(&[height]));
    fixture.blocks = vec![vec![128; width as usize * height as usize * 3]];
    let image = decode(fixture);
    assert_eq!((image.width, image.height), (width, height));
    assert_eq!(image.samples.len(), width as usize * height as usize * 3);
    assert!(image.samples.iter().all(|&value| value == 128.0 / 255.0));
}

#[test]
fn lossless_reduced_bit_depth_jpeg_matches_libjpeg_vectors() {
    for (bits, payload, expected) in [
        (
            8,
            "ffd8ffee000e41646f626500640000000000ffc30011080001000203521100471100421100ffc400160001010100000000000000000000000000080706ffda000c035200470042000100003fc3efe80d8083ffd9",
            vec![0, 16, 255, 128, 64, 32],
        ),
        (
            10,
            "ffd8ffee000e41646f626500640000000000ffc300110a0001000203521100471100421100ffc4001600010101000000000000000000000000000a0906ffda000c035200470042000100003ff03eff00a00d8020ffd9",
            vec![0, 16, 1023, 512, 64, 32],
        ),
    ] {
        let mut fixture = Fixture::rgb(2, 1, &[]);
        fixture.set(258, 3, shorts(&[bits; 3]));
        fixture.set(259, 3, shorts(&[7]));
        fixture.blocks = vec![hex(payload)];
        let image = Dng::parse(&fixture.build())
            .unwrap()
            .decode_with_options(DngDecodeOptions {
                retain_encoded_samples: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(image.encoded_samples.unwrap(), expected);
    }
}

#[test]
fn jpeg_xl_twelve_bit_codestream_uses_sixteen_bit_dng_storage_range() {
    let mut fixture = Fixture::rgb(2, 1, &[]);
    fixture.set(259, 3, shorts(&[52546]));
    fixture.blocks = vec![hex(
        "ff0a00701850b4080804010060004b188b15c2316c5c06285f20040002bdb5b9edfd43476100",
    )];
    let image = Dng::parse(&fixture.build())
        .unwrap()
        .decode_with_options(DngDecodeOptions {
            retain_encoded_samples: true,
            ..Default::default()
        })
        .unwrap();
    let expected = [0.0f64, 128.0, 4095.0, 2000.0, 1000.0, 2048.0]
        .map(|value| (value * 65535.0 / 4095.0).round() as u16);
    assert_eq!(image.encoded_samples.unwrap(), expected);
}

#[test]
fn lossy_xyb_jpeg_xl_matches_libjxl_srgb_output() {
    let mut fixture = Fixture::rgb(2, 2, &[]);
    fixture.set(258, 3, shorts(&[8; 3]));
    fixture.set(259, 3, shorts(&[52546]));
    fixture.blocks = vec![hex(
        "ff0a08900100130800e001773fc8010050a132cab8c1cbb99e2f3f74584cdbb8ced056db16963036db0b83848443d8b8f96c6e8e9551f41df1060c2aa440b6d95829b68cac62eb484645c6c8463e383eb7b4389323335e94c3c157d22eb8bf8cc17a3df0bbdcc73b3fe0054d890defabb2a8e8973a371acc31c4c61479bf73ad056600",
    )];
    let image = Dng::parse(&fixture.build())
        .unwrap()
        .decode_with_options(DngDecodeOptions {
            retain_encoded_samples: true,
            ..Default::default()
        })
        .unwrap();
    for (actual, expected) in image
        .encoded_samples
        .unwrap()
        .into_iter()
        .zip([5i32, 128, 252, 253, 62, 57, 5, 19, 40, 207, 148, 75])
    {
        assert!(
            (i32::from(actual) - expected).abs() <= 1,
            "{actual} != {expected}"
        );
    }
}

#[test]
fn row_and_column_interleave_are_undone_before_normalization() {
    let expected = (0u16..45).collect::<Vec<_>>();
    let mut encoded = Vec::new();
    for row in [0, 2, 1] {
        for column in [0, 2, 4, 1, 3] {
            encoded
                .extend_from_slice(&expected[(row * 5 + column) * 3..(row * 5 + column + 1) * 3]);
        }
    }
    let mut fixture = Fixture::rgb(5, 3, &encoded);
    fixture.set(50975, 4, longs(&[2]));
    fixture.set(52547, 4, longs(&[2]));
    let image = Dng::parse(&fixture.build())
        .unwrap()
        .decode_with_options(DngDecodeOptions {
            retain_encoded_samples: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(image.encoded_samples.unwrap(), expected);
}

#[test]
fn degenerate_opcode_areas_and_zero_profile_gains_are_rejected() {
    for (bounds, pitch) in [([2, 3, 2, 5], [1, 1]), ([0, 0, 0, 0], [2, 1])] {
        let mut fixture = Fixture::rgb(2, 2, &[1; 12]);
        let mut payload = area(bounds, 0, 3, pitch);
        payload.extend(0u32.to_be_bytes());
        payload.extend(1.0f64.to_be_bytes());
        fixture.set(51009, 7, opcode(8, 0, &payload));
        assert!(Dng::parse(&fixture.build()).is_err());
    }
    let mut tag = profile_tag();
    tag.data[64..68].fill(0);
    assert!(ProfileGainTable::parse(&tag, ByteOrder::LittleEndian).is_err());
}

#[test]
fn partial_tiles_discard_padding_without_crossing_channels() {
    let mut fixture = Fixture::rgb(3, 3, &[]);
    fixture.set(322, 4, longs(&[2]));
    fixture.set(323, 4, longs(&[2]));
    let pixel = |id| [id, 100 + id, 200 + id];
    fixture.blocks = [
        [1, 2, 4, 5],
        [3, 999, 6, 999],
        [7, 8, 999, 999],
        [9, 999, 999, 999],
    ]
    .into_iter()
    .map(|ids| shorts(&ids.into_iter().flat_map(pixel).collect::<Vec<_>>()))
    .collect();
    let image = Dng::parse(&fixture.build())
        .unwrap()
        .decode_with_options(DngDecodeOptions {
            retain_encoded_samples: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        image.encoded_samples.unwrap(),
        (1..=9).flat_map(pixel).collect::<Vec<_>>()
    );
}

#[test]
fn nonlinear_linearization_preserves_negative_and_above_white_samples() {
    let mut fixture = Fixture::rgb(1, 1, &[0, 1, 2]);
    fixture.set(50712, 3, shorts(&[2047, 2049, 2051]));
    fixture.set(50714, 3, shorts(&[2048; 3]));
    fixture.set(50717, 3, shorts(&[2050; 3]));
    assert_eq!(decode(fixture).samples, [-0.5, 0.5, 1.5]);
}

#[test]
fn legacy_descriptor_rejects_geometry_it_cannot_represent() {
    let mut fixture = Fixture::rgb(2, 2, &[0; 12]);
    fixture.set(50719, 5, rationals(&[(1, 2), (0, 1)]));
    fixture.set(50720, 5, rationals(&[(3, 2), (2, 1)]));
    let bytes = fixture.build();
    let dng = Dng::parse(&bytes).unwrap();
    assert!(matches!(dng.decode(), Err(DecodeError::UnsupportedFeature)));
    assert_eq!(
        dng.decode_with_options(Default::default())
            .unwrap()
            .default_crop_origin_exact,
        Some([0.5, 0.0])
    );
}
