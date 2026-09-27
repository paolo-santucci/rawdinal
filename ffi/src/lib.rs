// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! C ownership boundary for sensor-domain X3F decoding and experimental rendering.
//! All parsing and processing live in the safe Rust crate; each exported call contains
//! its own unwind boundary.

use rawdinal::{
    ByteOrder, ContainerProbe, DecodeError, DecodeLimits, Dng, LinearRawImage, LinearRawProcessing,
    SensorImage as DecodedSensorImage, X3f,
    experimental::{LinearImage, Reconstruction, render},
    probe,
};
use std::{
    ffi::c_char,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};

pub struct Image {
    pixels: LinearImage,
    exif: Vec<u8>,
}

#[repr(C)]
pub struct ImageInfo {
    width: u32,
    height: u32,
    exif: *const u8,
    exif_size: usize,
}

impl Default for ImageInfo {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            exif: ptr::null(),
            exif_size: 0,
        }
    }
}

pub struct SensorImage {
    image: DecodedSensorImage,
    camf: Vec<u8>,
    exif: Vec<u8>,
}

pub struct RawV1Image {
    image: LinearRawImage,
}

#[repr(C)]
pub struct RawV1Info {
    version: u32,
    reserved: u32,
    width: u32,
    height: u32,
    channels: u32,
    stride_samples: usize,
    sample_count: usize,
    samples: *const f32,
    component_ids: *const u8,
    component_id_count: usize,
    dng_version: [u8; 4],
    dng_backward_version: [u8; 4],
    dng_backward_version_present: u32,
    tiff_byte_order: u32,
    make: *const u8,
    make_length: usize,
    model: *const u8,
    model_length: usize,
    active_area_top: u32,
    active_area_left: u32,
    active_area_bottom: u32,
    active_area_right: u32,
    orientation: u32,
    orientation_present: u32,
    default_crop_origin_x: u32,
    default_crop_origin_y: u32,
    default_crop_origin_present: u32,
    default_crop_size_width: u32,
    default_crop_size_height: u32,
    default_crop_size_present: u32,
    linearization: u32,
    black_subtraction: u32,
    white_normalization: u32,
    white_balance: u32,
    color_conversion: u32,
    default_crop: u32,
    orientation_processing: u32,
    baseline_exposure: u32,
    profile_tone_curve: u32,
    demosaic: u32,
    opcode_list_1: u32,
    opcode_list_2: u32,
    opcode_list_3: u32,
    profile_gain_table_map_processing: u32,
    semantic_masks: u32,
    profile_gain_table_map: *const u8,
    profile_gain_table_map_length: usize,
    scene_linear: u32,
    camera_native: u32,
    already_demosaiced: u32,
    colorimetric_reference: u32,
    colorimetric_reference_present: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct RawV1ProbeInfo {
    version: u32,
    reserved: u32,
    classification: u32,
    container: u32,
    codec: u32,
    width: u32,
    height: u32,
    channels: u32,
}

#[repr(C)]
#[derive(Default)]
pub struct RawV1Capabilities {
    version: u32,
    reserved: u32,
    codec_bits: u32,
    reserved2: u32,
}

impl Default for RawV1Info {
    fn default() -> Self {
        Self {
            version: 0,
            reserved: 0,
            width: 0,
            height: 0,
            channels: 0,
            stride_samples: 0,
            sample_count: 0,
            samples: ptr::null(),
            component_ids: ptr::null(),
            component_id_count: 0,
            dng_version: [0; 4],
            dng_backward_version: [0; 4],
            dng_backward_version_present: 0,
            tiff_byte_order: 0,
            make: ptr::null(),
            make_length: 0,
            model: ptr::null(),
            model_length: 0,
            active_area_top: 0,
            active_area_left: 0,
            active_area_bottom: 0,
            active_area_right: 0,
            orientation: 0,
            orientation_present: 0,
            default_crop_origin_x: 0,
            default_crop_origin_y: 0,
            default_crop_origin_present: 0,
            default_crop_size_width: 0,
            default_crop_size_height: 0,
            default_crop_size_present: 0,
            linearization: 0,
            black_subtraction: 0,
            white_normalization: 0,
            white_balance: 0,
            color_conversion: 0,
            default_crop: 0,
            orientation_processing: 0,
            baseline_exposure: 0,
            profile_tone_curve: 0,
            demosaic: 0,
            opcode_list_1: 0,
            opcode_list_2: 0,
            opcode_list_3: 0,
            profile_gain_table_map_processing: 0,
            semantic_masks: 0,
            profile_gain_table_map: ptr::null(),
            profile_gain_table_map_length: 0,
            scene_linear: 0,
            camera_native: 0,
            already_demosaiced: 0,
            colorimetric_reference: 0,
            colorimetric_reference_present: 0,
        }
    }
}

#[repr(C)]
pub struct SensorPlane {
    identity: u32,
    reserved: u32,
    width: usize,
    height: usize,
    stride_samples: usize,
    sample_count: usize,
    samples: *const u16,
}

impl Default for SensorPlane {
    fn default() -> Self {
        Self {
            identity: 0,
            reserved: 0,
            width: 0,
            height: 0,
            stride_samples: 0,
            sample_count: 0,
            samples: ptr::null(),
        }
    }
}

/// Borrows the embedded JPEG without decoding sensor data. Returns nonzero on error.
///
/// # Safety
/// `data` must reference `length` initialized bytes. `preview` and `preview_length`
/// must reference writable, disjoint output objects. The returned slice remains
/// valid only while the input storage is live and unchanged.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_preview(
    data: *const u8,
    length: usize,
    preview: *mut *const u8,
    preview_length: *mut usize,
) -> i32 {
    if preview.is_null() || preview_length.is_null() {
        return 1;
    }
    unsafe {
        preview.write(ptr::null());
        preview_length.write(0);
    }
    if data.is_null() || !(44..=512 * 1024 * 1024).contains(&length) {
        return 1;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let bytes = unsafe { slice::from_raw_parts(data, length) };
        let Ok(file) = X3f::parse(bytes) else {
            return 1;
        };
        let Some(jpeg) = file.preview() else {
            return 1;
        };
        unsafe {
            preview.write(jpeg.as_ptr());
            preview_length.write(jpeg.len());
        }
        0
    }))
    .unwrap_or(2)
}

/// Returns 0 on success, 1 on invalid/unsupported input, or 2 on an internal panic.
/// The caller owns the resulting handle and releases it with `rawdinal_free`.
///
/// # Safety
/// `data` must reference `length` initialized bytes and remain valid during the call.
/// `output` and `info` must point to writable, nonoverlapping objects. When non-null,
/// `error` must reference `error_capacity` writable bytes, disjoint from all inputs.
/// The output slot must not hold an unreleased image. Null arguments are rejected.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_decode(
    data: *const u8,
    length: usize,
    output: *mut *mut Image,
    info: *mut ImageInfo,
    error: *mut c_char,
    error_capacity: usize,
) -> i32 {
    if output.is_null() || info.is_null() {
        return 1;
    }
    unsafe {
        output.write(ptr::null_mut());
        info.write(ImageInfo::default());
    }
    if data.is_null() || !(44..=512 * 1024 * 1024).contains(&length) {
        unsafe {
            write_error(
                error,
                error_capacity,
                "X3F input size outside supported limits",
            );
        }
        return 1;
    }
    let result = catch_unwind(AssertUnwindSafe(|| -> rawdinal::Result<Image> {
        let bytes = unsafe { slice::from_raw_parts(data, length) };
        let file = X3f::parse(bytes)?;
        let calibration = file.calibration()?;
        let white_balance = file.white_balance(&calibration)?;
        let pixels = render(
            &file.decode()?,
            &calibration,
            white_balance,
            Reconstruction::Guided,
        )?;
        Ok(Image {
            pixels,
            exif: file.exif()?.unwrap_or_default().to_vec(),
        })
    }));
    match result {
        Ok(Ok(image)) => {
            unsafe {
                info.write(ImageInfo {
                    width: image.pixels.width as u32,
                    height: image.pixels.height as u32,
                    exif: image.exif.as_ptr(),
                    exif_size: image.exif.len(),
                });
                output.write(Box::into_raw(Box::new(image)));
                write_error(error, error_capacity, "");
            }
            0
        }
        Ok(Err(message)) => {
            unsafe {
                write_error(error, error_capacity, &message.0);
            }
            1
        }
        Err(_) => {
            unsafe {
                write_error(error, error_capacity, "internal X3F error");
            }
            2
        }
    }
}

/// Copies unbounded, linear-sRGB pixels into caller-owned RGBA storage; alpha is 1.
/// Returns nonzero for an invalid buffer size or a caught internal panic.
///
/// # Safety
/// `image` must be a live handle from `rawdinal_decode`. `destination` must reference
/// `float_count` writable f32 values, disjoint from the handle's storage. The handle
/// must not be freed concurrently. The destination requires width * height * 4 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_copy_rgba(
    image: *const Image,
    destination: *mut f32,
    float_count: usize,
) -> i32 {
    if image.is_null() || destination.is_null() {
        return 1;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let image = unsafe { &(*image).pixels };
        if image.rgb.len().checked_mul(4) != Some(float_count) {
            return 1;
        }
        let destination = unsafe { slice::from_raw_parts_mut(destination, float_count) };
        for (rgba, rgb) in destination.chunks_exact_mut(4).zip(&image.rgb) {
            rgba[..3].copy_from_slice(rgb);
            rgba[3] = 1.0;
        }
        0
    }))
    .unwrap_or(2)
}

/// Releases an image; null is accepted.
///
/// # Safety
/// A non-null handle must have been returned by `rawdinal_decode`, must not have
/// been released, and must not be accessed by any other call during or after release.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_free(image: *mut Image) {
    if !image.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
            drop(Box::from_raw(image));
        }));
    }
}

/// Decodes the supported Apple ProRAW DNG linear-raw layout into an owned raw-v1 handle.
///
/// # Safety
/// `data` must reference `length` initialized bytes for this call. `output` must reference a
/// writable slot that does not contain an unreleased raw-v1 handle. When non-null, `error` must
/// reference `error_capacity` writable bytes disjoint from the input and output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_decode(
    data: *const u8,
    length: usize,
    output: *mut *mut RawV1Image,
    error: *mut c_char,
    error_capacity: usize,
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| {
        unsafe {
            clear_raw_output(output);
            write_error(error, error_capacity, "");
        }
        if output.is_null() || data.is_null() {
            unsafe {
                write_error(error, error_capacity, "invalid raw-v1 arguments");
            }
            return RAW_V1_INVALID_INPUT;
        }
        if raw_v1_input_limit_exceeded(length) {
            unsafe {
                write_error(error, error_capacity, "raw-v1 input exceeds resource limit");
            }
            return RAW_V1_RESOURCE_LIMIT;
        }
        let bytes = unsafe { slice::from_raw_parts(data, length) };
        match decode_raw_v1(bytes) {
            Ok(image) => {
                unsafe {
                    output.write(Box::into_raw(Box::new(image)));
                }
                RAW_V1_OK
            }
            Err(error_value) => {
                unsafe {
                    write_error(error, error_capacity, &error_value.to_string());
                }
                raw_v1_status(error_value)
            }
        }
    })) {
        Ok(status) => status,
        Err(_) => {
            unsafe {
                clear_raw_output(output);
                write_error(error, error_capacity, "internal raw-v1 error");
            }
            RAW_V1_PANIC
        }
    }
}

fn raw_v1_input_limit_exceeded(length: usize) -> bool {
    length > DecodeLimits::default().max_input_bytes || length > isize::MAX as usize
}

fn decode_raw_v1(bytes: &[u8]) -> std::result::Result<RawV1Image, DecodeError> {
    match raw_v1_probe_description(bytes)? {
        RawV1ProbeInfo {
            classification: RAW_V1_PROBE_SUPPORTED,
            ..
        } => Dng::parse(bytes)
            .and_then(|dng| dng.decode())
            .map(|image| RawV1Image { image }),
        RawV1ProbeInfo {
            classification: RAW_V1_PROBE_NOT_RECOGNIZED,
            ..
        } => Err(DecodeError::NotRecognized),
        _ => Err(DecodeError::UnsupportedFeature),
    }
}

fn raw_v1_probe_description(bytes: &[u8]) -> std::result::Result<RawV1ProbeInfo, DecodeError> {
    if bytes.starts_with(b"FOVb") {
        return Ok(RawV1ProbeInfo {
            version: 1,
            classification: RAW_V1_PROBE_RECOGNIZED_UNSUPPORTED,
            container: RAW_V1_CONTAINER_X3F,
            ..RawV1ProbeInfo::default()
        });
    }
    if !matches!(bytes.get(..2), Some(b"II" | b"MM")) {
        return Ok(raw_v1_not_recognized());
    }
    if bytes.len() < 4 {
        return Err(DecodeError::Truncated);
    }
    match bytes.get(..4) {
        Some(b"II*\0" | b"MM\0*") => {}
        Some(b"II+\0" | b"MM\0+") => return Ok(raw_v1_not_recognized()),
        _ => return Err(DecodeError::InvalidContainer),
    }
    if bytes.len() < 8 {
        return Err(DecodeError::Truncated);
    }
    match probe(bytes, DecodeLimits::default())? {
        ContainerProbe::Dng(facts) => {
            let codec = raw_v1_codec(facts.compression);
            match Dng::parse(bytes) {
                Ok(_) => Ok(RawV1ProbeInfo {
                    version: 1,
                    classification: RAW_V1_PROBE_SUPPORTED,
                    container: RAW_V1_CONTAINER_DNG,
                    codec,
                    width: facts.width,
                    height: facts.height,
                    channels: u32::from(facts.samples_per_pixel),
                    ..RawV1ProbeInfo::default()
                }),
                Err(DecodeError::UnsupportedFormat | DecodeError::UnsupportedFeature) => {
                    Ok(RawV1ProbeInfo {
                        version: 1,
                        classification: RAW_V1_PROBE_RECOGNIZED_UNSUPPORTED,
                        container: RAW_V1_CONTAINER_DNG,
                        codec,
                        width: facts.width,
                        height: facts.height,
                        channels: u32::from(facts.samples_per_pixel),
                        ..RawV1ProbeInfo::default()
                    })
                }
                Err(error) => Err(error),
            }
        }
        ContainerProbe::Unknown => Ok(raw_v1_not_recognized()),
        ContainerProbe::X3f(_) => Ok(RawV1ProbeInfo {
            version: 1,
            classification: RAW_V1_PROBE_RECOGNIZED_UNSUPPORTED,
            container: RAW_V1_CONTAINER_X3F,
            ..RawV1ProbeInfo::default()
        }),
        _ => Ok(raw_v1_not_recognized()),
    }
}

fn raw_v1_not_recognized() -> RawV1ProbeInfo {
    RawV1ProbeInfo {
        version: 1,
        classification: RAW_V1_PROBE_NOT_RECOGNIZED,
        ..RawV1ProbeInfo::default()
    }
}

fn raw_v1_codec(compression: u16) -> u32 {
    match compression {
        7 => RAW_V1_CODEC_LOSSLESS_JPEG,
        52_546 => RAW_V1_CODEC_JPEG_XL,
        _ => RAW_V1_CODEC_UNKNOWN,
    }
}

/// Probes a raw-v1 input without decoding samples.
///
/// # Safety
/// `data` must reference `length` initialized bytes. `output` must be writable and disjoint from
/// input and error. When non-null, `error` must reference `error_capacity` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_probe(
    data: *const u8,
    length: usize,
    output: *mut RawV1ProbeInfo,
    error: *mut c_char,
    error_capacity: usize,
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| {
        unsafe {
            clear_raw_probe_info(output);
            write_error(error, error_capacity, "");
        }
        if data.is_null() || output.is_null() {
            unsafe { write_error(error, error_capacity, "invalid raw-v1 arguments") };
            return RAW_V1_INVALID_INPUT;
        }
        if raw_v1_input_limit_exceeded(length) {
            unsafe { write_error(error, error_capacity, "raw-v1 input exceeds resource limit") };
            return RAW_V1_RESOURCE_LIMIT;
        }
        let bytes = unsafe { slice::from_raw_parts(data, length) };
        match raw_v1_probe_description(bytes) {
            Ok(info) => {
                unsafe { output.write(info) };
                RAW_V1_OK
            }
            Err(error_value) => {
                unsafe { write_error(error, error_capacity, &error_value.to_string()) };
                raw_v1_status(error_value)
            }
        }
    })) {
        Ok(status) => status,
        Err(_) => {
            unsafe {
                clear_raw_probe_info(output);
                write_error(error, error_capacity, "internal raw-v1 error");
            }
            RAW_V1_PANIC
        }
    }
}

/// Writes raw-v1 codec capabilities.
///
/// # Safety
/// `output` must reference writable storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_get_capabilities(output: *mut RawV1Capabilities) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| {
        unsafe { clear_raw_capabilities(output) };
        if output.is_null() {
            return RAW_V1_INVALID_INPUT;
        }
        unsafe {
            output.write(RawV1Capabilities {
                version: 1,
                reserved: 0,
                codec_bits: RAW_V1_CODEC_BIT_LOSSLESS_JPEG,
                reserved2: 0,
            })
        };
        RAW_V1_OK
    })) {
        Ok(status) => status,
        Err(_) => {
            unsafe { clear_raw_capabilities(output) };
            RAW_V1_PANIC
        }
    }
}

/// Borrows the raw-v1 descriptor stored in a live handle.
///
/// # Safety
/// `image` must be a live raw-v1 handle. `output` must reference writable storage disjoint from
/// the handle and its owned storage. The handle must not be accessed or released concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_get_info(
    image: *const RawV1Image,
    output: *mut RawV1Info,
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| {
        unsafe {
            clear_raw_info(output);
        }
        if image.is_null() || output.is_null() {
            return RAW_V1_INVALID_INPUT;
        }
        let Some(info) = raw_v1_info(unsafe { &(*image).image }) else {
            return RAW_V1_INVALID_INPUT;
        };
        unsafe { output.write(info) };
        RAW_V1_OK
    })) {
        Ok(status) => status,
        Err(_) => {
            unsafe {
                clear_raw_info(output);
            }
            RAW_V1_PANIC
        }
    }
}

fn raw_v1_info(image: &LinearRawImage) -> Option<RawV1Info> {
    let (
        [
            active_area_top,
            active_area_left,
            active_area_bottom,
            active_area_right,
        ],
        orientation,
    ) = (image.active_area, image.orientation);
    let ([default_crop_origin_x, default_crop_origin_y], default_crop_origin_present) = image
        .default_crop_origin
        .map(|value| (value, 1))
        .unwrap_or(([0, 0], 0));
    let ([default_crop_size_width, default_crop_size_height], default_crop_size_present) = image
        .default_crop_size
        .map(|value| (value, 1))
        .unwrap_or(([0, 0], 0));
    let (dng_backward_version, dng_backward_version_present) = image
        .dng_backward_version
        .map(|value| (value, 1))
        .unwrap_or(([0; 4], 0));
    let (model, model_length) = image
        .model
        .as_ref()
        .map(|value| (value.as_ptr().cast(), value.len()))
        .unwrap_or((ptr::null(), 0));
    let (profile_gain_table_map, profile_gain_table_map_length) = image
        .profile_gain_table_map
        .as_ref()
        .map(|value| (value.as_ptr(), value.len()))
        .unwrap_or((ptr::null(), 0));
    let (colorimetric_reference, colorimetric_reference_present) = image
        .colorimetric_reference
        .map(|value| (value, 1))
        .unwrap_or((0, 0));
    let width = usize::try_from(image.width).ok()?;
    let height = usize::try_from(image.height).ok()?;
    let channels = usize::from(image.channels);
    let stride_samples = width.checked_mul(channels)?;
    let sample_count = stride_samples.checked_mul(height)?;
    if width == 0
        || height == 0
        || channels == 0
        || image.component_ids.len() != channels
        || image.samples.len() != sample_count
    {
        return None;
    }
    Some(RawV1Info {
        version: 1,
        reserved: 0,
        width: image.width,
        height: image.height,
        channels: u32::from(image.channels),
        stride_samples,
        sample_count,
        samples: image.samples.as_ptr(),
        component_ids: image.component_ids.as_ptr(),
        component_id_count: image.component_ids.len(),
        dng_version: image.dng_version,
        dng_backward_version,
        dng_backward_version_present,
        tiff_byte_order: raw_v1_byte_order(image.tiff_byte_order),
        make: image.make.as_ptr(),
        make_length: image.make.len(),
        model,
        model_length,
        active_area_top,
        active_area_left,
        active_area_bottom,
        active_area_right,
        orientation: orientation.map(u32::from).unwrap_or(0),
        orientation_present: u32::from(orientation.is_some()),
        default_crop_origin_x,
        default_crop_origin_y,
        default_crop_origin_present,
        default_crop_size_width,
        default_crop_size_height,
        default_crop_size_present,
        linearization: raw_v1_processing(image.processing.linearization),
        black_subtraction: raw_v1_processing(image.processing.black_subtraction),
        white_normalization: raw_v1_processing(image.processing.white_normalization),
        white_balance: raw_v1_processing(image.processing.white_balance),
        color_conversion: raw_v1_processing(image.processing.color_conversion),
        default_crop: raw_v1_processing(image.processing.default_crop),
        orientation_processing: raw_v1_processing(image.processing.orientation),
        baseline_exposure: raw_v1_processing(image.processing.baseline_exposure),
        profile_tone_curve: raw_v1_processing(image.processing.profile_tone_curve),
        demosaic: raw_v1_processing(image.processing.demosaic),
        opcode_list_1: raw_v1_processing(image.processing.opcode_list_1),
        opcode_list_2: raw_v1_processing(image.processing.opcode_list_2),
        opcode_list_3: raw_v1_processing(image.processing.opcode_list_3),
        profile_gain_table_map_processing: raw_v1_processing(
            image.processing.profile_gain_table_map,
        ),
        semantic_masks: raw_v1_processing(image.processing.semantic_masks),
        profile_gain_table_map,
        profile_gain_table_map_length,
        scene_linear: 1,
        camera_native: 1,
        already_demosaiced: 1,
        colorimetric_reference,
        colorimetric_reference_present,
    })
}

fn raw_v1_processing(value: LinearRawProcessing) -> u32 {
    match value {
        LinearRawProcessing::NotPresent => 0,
        LinearRawProcessing::Applied => 1,
        LinearRawProcessing::Unapplied => 2,
        LinearRawProcessing::Unknown => 3,
        LinearRawProcessing::SkippedOptional => 4,
        _ => 3,
    }
}

fn raw_v1_byte_order(value: ByteOrder) -> u32 {
    match value {
        ByteOrder::LittleEndian => RAW_V1_TIFF_BYTE_ORDER_LITTLE_ENDIAN,
        ByteOrder::BigEndian => RAW_V1_TIFF_BYTE_ORDER_BIG_ENDIAN,
        _ => RAW_V1_TIFF_BYTE_ORDER_UNKNOWN,
    }
}

fn raw_v1_status(error: DecodeError) -> i32 {
    match error {
        DecodeError::NotRecognized => RAW_V1_NOT_RECOGNIZED,
        DecodeError::UnsupportedFormat | DecodeError::UnsupportedFeature => RAW_V1_UNSUPPORTED,
        DecodeError::ResourceLimit => RAW_V1_RESOURCE_LIMIT,
        DecodeError::Allocation => RAW_V1_ALLOCATION,
        DecodeError::InvalidContainer
        | DecodeError::Truncated
        | DecodeError::InvalidOffset
        | DecodeError::InvalidTag
        | DecodeError::InvalidGeometry => RAW_V1_INVALID_INPUT,
        _ => RAW_V1_INVALID_INPUT,
    }
}

/// Releases a raw-v1 handle; null is accepted.
///
/// # Safety
/// A non-null handle must have been returned by `rawdinal_raw_v1_decode`, must not have been
/// released, and must not be accessed by another call during or after release.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_free(image: *mut RawV1Image) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !image.is_null() {
            unsafe {
                drop(Box::from_raw(image));
            }
        }
    }));
}

const RAW_V1_OK: i32 = 0;
const RAW_V1_PANIC: i32 = 2;
const RAW_V1_UNSUPPORTED: i32 = 3;
const RAW_V1_INVALID_INPUT: i32 = 4;
const RAW_V1_RESOURCE_LIMIT: i32 = 5;
const RAW_V1_ALLOCATION: i32 = 6;
const RAW_V1_NOT_RECOGNIZED: i32 = 7;
const RAW_V1_PROBE_NOT_RECOGNIZED: u32 = 0;
const RAW_V1_PROBE_SUPPORTED: u32 = 1;
const RAW_V1_PROBE_RECOGNIZED_UNSUPPORTED: u32 = 2;
const RAW_V1_CONTAINER_DNG: u32 = 1;
const RAW_V1_CONTAINER_X3F: u32 = 2;
const RAW_V1_CODEC_UNKNOWN: u32 = 0;
const RAW_V1_CODEC_LOSSLESS_JPEG: u32 = 1;
const RAW_V1_CODEC_JPEG_XL: u32 = 2;
const RAW_V1_CODEC_BIT_LOSSLESS_JPEG: u32 = 1;
const RAW_V1_TIFF_BYTE_ORDER_UNKNOWN: u32 = 0;
const RAW_V1_TIFF_BYTE_ORDER_LITTLE_ENDIAN: u32 = 1;
const RAW_V1_TIFF_BYTE_ORDER_BIG_ENDIAN: u32 = 2;

unsafe fn clear_raw_output(output: *mut *mut RawV1Image) {
    if !output.is_null() {
        unsafe {
            output.write(ptr::null_mut());
        }
    }
}

unsafe fn clear_raw_info(output: *mut RawV1Info) {
    if !output.is_null() {
        unsafe {
            output.write(RawV1Info::default());
        }
    }
}

unsafe fn clear_raw_probe_info(output: *mut RawV1ProbeInfo) {
    if !output.is_null() {
        unsafe { output.write(RawV1ProbeInfo::default()) };
    }
}

unsafe fn clear_raw_capabilities(output: *mut RawV1Capabilities) {
    if !output.is_null() {
        unsafe { output.write(RawV1Capabilities::default()) };
    }
}

/// Decodes physical sensor planes and uninterpreted CAMF without rendering. Returns zero on success.
///
/// # Safety
/// `data` must reference `length` initialized bytes and remain valid during the call. `output`
/// must point to a writable slot that does not contain an unreleased sensor handle. When non-null,
/// `error` must reference `error_capacity` writable bytes disjoint from the input and output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_sensor_v1_decode(
    data: *const u8,
    length: usize,
    output: *mut *mut SensorImage,
    error: *mut c_char,
    error_capacity: usize,
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| {
        if output.is_null() {
            return 1;
        }
        unsafe {
            output.write(ptr::null_mut());
        }
        if data.is_null() || !(44..=512 * 1024 * 1024).contains(&length) {
            unsafe {
                write_error(
                    error,
                    error_capacity,
                    "X3F input size outside supported limits",
                );
            }
            return 1;
        }
        let bytes = unsafe { slice::from_raw_parts(data, length) };
        let result = decode_sensor_image(bytes);
        match result {
            Ok(image) => {
                unsafe {
                    output.write(Box::into_raw(Box::new(image)));
                    write_error(error, error_capacity, "");
                }
                0
            }
            Err(message) => {
                unsafe {
                    write_error(error, error_capacity, &message.0);
                }
                1
            }
        }
    })) {
        Ok(status) => status,
        Err(_) => {
            if !output.is_null() {
                unsafe {
                    output.write(ptr::null_mut());
                }
            }
            unsafe {
                write_error(error, error_capacity, "internal X3F error");
            }
            2
        }
    }
}

fn decode_sensor_image(bytes: &[u8]) -> rawdinal::Result<SensorImage> {
    let file = X3f::parse(bytes)?;
    Ok(SensorImage {
        image: file.decode()?,
        camf: file.camf_bytes()?,
        exif: file.exif().ok().flatten().unwrap_or_default().to_vec(),
    })
}

/// Borrows one physical sensor plane. Returns zero on success.
///
/// # Safety
/// `image` must be a live handle returned by `rawdinal_sensor_v1_decode`, and `output` must point
/// to writable storage disjoint from the handle and its owned storage. The handle must not be
/// released or accessed concurrently during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_sensor_v1_get_plane(
    image: *const SensorImage,
    identity: u32,
    output: *mut SensorPlane,
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| {
        if output.is_null() {
            return 1;
        }
        unsafe {
            output.write(SensorPlane::default());
        }
        if image.is_null() || identity > 2 {
            return 1;
        }
        let plane = unsafe { &(*image).image.layers[identity as usize] };
        unsafe {
            output.write(SensorPlane {
                identity,
                reserved: 0,
                width: plane.width,
                height: plane.height,
                stride_samples: plane.width,
                sample_count: plane.samples.len(),
                samples: plane.samples.as_ptr(),
            });
        }
        0
    })) {
        Ok(status) => status,
        Err(_) => {
            if !output.is_null() {
                unsafe {
                    output.write(SensorPlane::default());
                }
            }
            2
        }
    }
}

/// Borrows decompressed CAMF bytes. Returns zero on success.
///
/// # Safety
/// `image` must be a live sensor handle. `data` and `length` must be writable, disjoint from each
/// other and handle-owned storage. The handle must not be released or accessed concurrently during
/// this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_sensor_v1_get_camf(
    image: *const SensorImage,
    data: *mut *const u8,
    length: *mut usize,
) -> i32 {
    unsafe { sensor_bytes(image, data, length, |image| image.camf.as_slice()) }
}

/// Borrows copied TIFF/EXIF bytes. Returns zero on success, including an empty EXIF payload.
///
/// # Safety
/// `image` must be a live sensor handle. `data` and `length` must be writable, disjoint from each
/// other and handle-owned storage. The handle must not be released or accessed concurrently during
/// this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_sensor_v1_get_exif(
    image: *const SensorImage,
    data: *mut *const u8,
    length: *mut usize,
) -> i32 {
    unsafe { sensor_bytes(image, data, length, |image| image.exif.as_slice()) }
}

/// Releases a sensor handle; null is accepted.
///
/// # Safety
/// A non-null handle must have been returned by `rawdinal_sensor_v1_decode`, must not have been
/// released, and must not be accessed by any other call during or after release.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_sensor_v1_free(image: *mut SensorImage) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !image.is_null() {
            unsafe {
                drop(Box::from_raw(image));
            }
        }
    }));
}

unsafe fn sensor_bytes(
    image: *const SensorImage,
    data: *mut *const u8,
    length: *mut usize,
    bytes: fn(&SensorImage) -> &[u8],
) -> i32 {
    match catch_unwind(AssertUnwindSafe(|| {
        unsafe {
            clear_bytes_output(data, length);
        }
        if image.is_null() || data.is_null() || length.is_null() {
            return 1;
        }
        let bytes = bytes(unsafe { &*image });
        unsafe {
            data.write(if bytes.is_empty() {
                ptr::null()
            } else {
                bytes.as_ptr()
            });
            length.write(bytes.len());
        }
        0
    })) {
        Ok(status) => status,
        Err(_) => {
            unsafe {
                clear_bytes_output(data, length);
            }
            2
        }
    }
}

unsafe fn clear_bytes_output(data: *mut *const u8, length: *mut usize) {
    if !data.is_null() {
        unsafe {
            data.write(ptr::null());
        }
    }
    if !length.is_null() {
        unsafe {
            length.write(0);
        }
    }
}

unsafe fn write_error(destination: *mut c_char, capacity: usize, message: &str) {
    if destination.is_null() || capacity == 0 {
        return;
    }
    let count = message.len().min(capacity - 1);
    unsafe {
        ptr::copy_nonoverlapping(message.as_ptr(), destination.cast(), count);
        destination.add(count).write(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    fn error_text(error: &[c_char]) -> String {
        unsafe {
            CStr::from_ptr(error.as_ptr())
                .to_string_lossy()
                .into_owned()
        }
    }

    fn sensor_handle(exif: Vec<u8>) -> *mut SensorImage {
        Box::into_raw(Box::new(SensorImage {
            image: DecodedSensorImage {
                layers: [
                    rawdinal::Plane {
                        width: 2,
                        height: 2,
                        samples: vec![10; 4],
                    },
                    rawdinal::Plane {
                        width: 2,
                        height: 2,
                        samples: vec![11; 4],
                    },
                    rawdinal::Plane {
                        width: 4,
                        height: 4,
                        samples: vec![12; 16],
                    },
                ],
            },
            camf: b"CMbT\0\0\0\0(\0\0\0\x14\0\0\0\x18\0\0\0Tag\0\x0c\0\0\0hello world\0".to_vec(),
            exif,
        }))
    }

    fn raw_v1_handle() -> *mut RawV1Image {
        Box::into_raw(Box::new(RawV1Image {
            image: LinearRawImage {
                width: 2,
                height: 1,
                channels: 3,
                component_ids: vec![1, 2, 3],
                dng_version: [1, 6, 0, 0],
                dng_backward_version: Some([1, 3, 0, 0]),
                tiff_byte_order: Default::default(),
                make: "Apple".into(),
                model: Some("iPhone".into()),
                colorimetric_reference: Some(0),
                active_area: [0, 0, 1, 2],
                orientation: Some(1),
                default_crop_origin: Some([0, 0]),
                default_crop_size: Some([2, 1]),
                profile_gain_table_map: Some(vec![4, 5]),
                samples: vec![0.0, 0.5, 1.0, 1.5, 2.0, 2.5],
                processing: Default::default(),
            },
        }))
    }

    #[test]
    fn raw_v1_status_mapping_is_stable() {
        assert_eq!(raw_v1_status(DecodeError::UnsupportedFormat), 3);
        assert_eq!(raw_v1_status(DecodeError::UnsupportedFeature), 3);
        assert_eq!(raw_v1_status(DecodeError::Truncated), 4);
        assert_eq!(raw_v1_status(DecodeError::InvalidGeometry), 4);
        assert_eq!(raw_v1_status(DecodeError::ResourceLimit), 5);
        assert_eq!(raw_v1_status(DecodeError::Allocation), 6);
        assert_eq!(raw_v1_status(DecodeError::NotRecognized), 7);
        assert_eq!(raw_v1_processing(LinearRawProcessing::Unknown), 3);
        assert_eq!(raw_v1_processing(LinearRawProcessing::SkippedOptional), 4);
        assert_eq!(raw_v1_codec(7), 1);
        assert_eq!(raw_v1_codec(52_546), 2);
        assert_eq!(raw_v1_codec(1), 0);
    }

    #[test]
    fn raw_v1_info_borrows_owned_metadata_and_samples() {
        let image = raw_v1_handle();
        let mut info = RawV1Info::default();
        assert_eq!(unsafe { rawdinal_raw_v1_get_info(image, &mut info) }, 0);
        assert_eq!((info.version, info.reserved), (1, 0));
        assert_eq!(
            (
                info.width,
                info.height,
                info.channels,
                info.stride_samples,
                info.sample_count,
            ),
            (2, 1, 3, 6, 6)
        );
        assert_eq!(info.dng_version, [1, 6, 0, 0]);
        assert_eq!(info.dng_backward_version, [1, 3, 0, 0]);
        assert_eq!(info.tiff_byte_order, 1);
        assert_eq!(
            (
                info.dng_backward_version_present,
                info.orientation,
                info.orientation_present,
                info.default_crop_origin_present,
                info.default_crop_size_present,
            ),
            (1, 1, 1, 1, 1)
        );
        assert_eq!(
            (
                info.linearization,
                info.demosaic,
                info.profile_gain_table_map_processing,
                info.semantic_masks,
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(
            (
                info.scene_linear,
                info.camera_native,
                info.already_demosaiced,
                info.colorimetric_reference,
                info.colorimetric_reference_present,
            ),
            (1, 1, 1, 0, 1)
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(info.samples, info.sample_count) },
            [0.0, 0.5, 1.0, 1.5, 2.0, 2.5]
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(info.component_ids, info.component_id_count) },
            [1, 2, 3]
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(info.make, info.make_length) },
            b"Apple"
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(info.model, info.model_length) },
            b"iPhone"
        );
        assert_eq!(
            unsafe {
                slice::from_raw_parts(
                    info.profile_gain_table_map,
                    info.profile_gain_table_map_length,
                )
            },
            [4, 5]
        );
        unsafe {
            rawdinal_raw_v1_free(image);
        }
    }

    #[test]
    fn raw_v1_failures_clear_outputs_and_terminate_errors() {
        let mut image = ptr::NonNull::<RawV1Image>::dangling().as_ptr();
        let mut error = [b'x' as c_char; 5];
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_decode(
                    b"II".as_ptr(),
                    2,
                    &mut image,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            4
        );
        assert!(image.is_null());
        assert_eq!(error[4], 0);
        image = ptr::NonNull::<RawV1Image>::dangling().as_ptr();
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_decode(
                    b"FOVb".as_ptr(),
                    4,
                    &mut image,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            3
        );
        assert!(image.is_null());
        let mut info = RawV1Info {
            version: 99,
            samples: ptr::NonNull::<f32>::dangling().as_ptr(),
            ..RawV1Info::default()
        };
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_info(ptr::null(), &mut info) },
            4
        );
        assert_eq!((info.version, info.samples.is_null()), (0, true));
        unsafe {
            rawdinal_raw_v1_free(ptr::null_mut());
        }
    }

    #[test]
    fn raw_v1_rejects_oversized_input_before_borrowing_it() {
        let mut image = ptr::NonNull::<RawV1Image>::dangling().as_ptr();
        let mut error = [b'x' as c_char; 8];
        let length = DecodeLimits::default().max_input_bytes + 1;
        assert!(raw_v1_input_limit_exceeded(length));
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_decode(
                    ptr::NonNull::<u8>::dangling().as_ptr(),
                    length,
                    &mut image,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            5
        );
        assert!(image.is_null());
        assert_eq!(error[error.len() - 1], 0);
    }

    #[test]
    fn raw_v1_info_rejects_inconsistent_owned_images() {
        let image = raw_v1_handle();
        unsafe {
            (*image).image.samples.pop();
        }
        let mut info = RawV1Info {
            version: 99,
            ..RawV1Info::default()
        };
        assert_eq!(unsafe { rawdinal_raw_v1_get_info(image, &mut info) }, 4);
        assert_eq!(info.version, 0);
        unsafe {
            rawdinal_raw_v1_free(image);
        }
    }

    #[test]
    fn raw_v1_probe_classifies_routing_boundaries_and_clears_failures() {
        let mut info = RawV1ProbeInfo::default();
        let mut error = [b'x' as c_char; 8];
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_probe(
                    b"random".as_ptr(),
                    b"random".len(),
                    &mut info,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            0
        );
        assert_eq!((info.version, info.classification), (1, 0));
        let mut image = ptr::null_mut();
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_decode(
                    b"random".as_ptr(),
                    b"random".len(),
                    &mut image,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            7
        );
        assert!(image.is_null());
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_probe(
                    b"FOVb".as_ptr(),
                    4,
                    &mut info,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            0
        );
        assert_eq!((info.classification, info.container), (2, 2));
        let tiff = b"II*\0\x08\0\0\0\x00\0\0\0\0\0\0\0";
        let unknown_tiff = b"II*\0\xff\xff\xff\xff";
        let big_tiff = [b'I', b'I', b'+', 0];
        let invalid_tiff = [b'I', b'I', b')', 0];
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_probe(
                    tiff.as_ptr(),
                    tiff.len(),
                    &mut info,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            0
        );
        assert_eq!(info.classification, 0);
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_probe(
                    unknown_tiff.as_ptr(),
                    unknown_tiff.len(),
                    &mut info,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            0
        );
        assert_eq!(info.classification, 0);
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_decode(
                    unknown_tiff.as_ptr(),
                    unknown_tiff.len(),
                    &mut image,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            7
        );
        assert!(image.is_null());
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_probe(
                    big_tiff.as_ptr(),
                    4,
                    &mut info,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            0
        );
        assert_eq!(info.classification, 0);
        info.version = 9;
        assert_eq!(
            unsafe {
                rawdinal_raw_v1_probe(
                    invalid_tiff.as_ptr(),
                    4,
                    &mut info,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            4
        );
        assert_eq!((info.version, error[error.len() - 1]), (0, 0));
    }

    #[test]
    fn raw_v1_capabilities_are_stable_and_clear_invalid_output() {
        let mut capabilities = RawV1Capabilities::default();
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_capabilities(&mut capabilities) },
            0
        );
        assert_eq!(
            (
                capabilities.version,
                capabilities.reserved,
                capabilities.codec_bits,
                capabilities.reserved2,
            ),
            (1, 0, 1, 0)
        );
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_capabilities(ptr::null_mut()) },
            4
        );
    }

    #[test]
    fn sensor_handle_owns_physical_planes_camf_and_empty_exif() {
        let image = sensor_handle(Vec::new());
        for (identity, (width, height, count, sample)) in
            [(0, (2, 2, 4, 10)), (1, (2, 2, 4, 11)), (2, (4, 4, 16, 12))]
        {
            let mut plane = SensorPlane::default();
            assert_eq!(
                unsafe { rawdinal_sensor_v1_get_plane(image, identity, &mut plane) },
                0
            );
            assert_eq!(
                (
                    plane.identity,
                    plane.reserved,
                    plane.width,
                    plane.height,
                    plane.stride_samples,
                    plane.sample_count
                ),
                (identity, 0, width, height, width, count)
            );
            assert_eq!(
                unsafe { slice::from_raw_parts(plane.samples, plane.sample_count) },
                vec![sample; count]
            );
        }
        let mut camf = ptr::null();
        let mut camf_length = 0;
        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_camf(image, &mut camf, &mut camf_length) },
            0
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(camf, camf_length) },
            b"CMbT\0\0\0\0(\0\0\0\x14\0\0\0\x18\0\0\0Tag\0\x0c\0\0\0hello world\0"
        );
        let mut exif = [1u8].as_ptr();
        let mut exif_length = 1;
        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_exif(image, &mut exif, &mut exif_length) },
            0
        );
        assert_eq!((exif.is_null(), exif_length), (true, 0));
        unsafe {
            rawdinal_sensor_v1_free(image);
        }
    }

    #[test]
    fn sensor_getters_clear_outputs_on_failure() {
        let image = sensor_handle(Vec::new());
        let mut plane = SensorPlane {
            identity: 2,
            reserved: 1,
            width: 1,
            height: 1,
            stride_samples: 1,
            sample_count: 1,
            samples: [7u16].as_ptr(),
        };
        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_plane(image, 3, &mut plane) },
            1
        );
        assert_eq!(
            (
                plane.identity,
                plane.reserved,
                plane.width,
                plane.height,
                plane.stride_samples,
                plane.sample_count,
                plane.samples.is_null()
            ),
            (0, 0, 0, 0, 0, 0, true)
        );
        let mut data = [7u8].as_ptr();
        let mut length = 1;
        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_camf(ptr::null(), &mut data, &mut length) },
            1
        );
        assert_eq!((data.is_null(), length), (true, 0));
        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_exif(ptr::null(), &mut data, &mut length) },
            1
        );
        assert_eq!((data.is_null(), length), (true, 0));
        length = 1;
        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_camf(image, ptr::null_mut(), &mut length) },
            1
        );
        assert_eq!(length, 0);
        data = [7u8].as_ptr();
        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_exif(image, &mut data, ptr::null_mut()) },
            1
        );
        assert!(data.is_null());
        unsafe {
            rawdinal_sensor_v1_free(image);
        }
    }

    #[test]
    fn sensor_decode_invalid_input_clears_output_and_terminates_error() {
        let bytes = [0u8; 44];
        let mut image = ptr::NonNull::<SensorImage>::dangling().as_ptr();
        let mut error = [b'x' as c_char; 5];
        assert_eq!(
            unsafe {
                rawdinal_sensor_v1_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    &mut image,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            1
        );
        assert_eq!((image.is_null(), error[4]), (true, 0));
        assert_eq!(
            unsafe {
                rawdinal_sensor_v1_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    ptr::null_mut(),
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            1
        );
        image = ptr::NonNull::<SensorImage>::dangling().as_ptr();
        assert_eq!(
            unsafe {
                rawdinal_sensor_v1_decode(
                    ptr::null(),
                    bytes.len(),
                    &mut image,
                    error.as_mut_ptr(),
                    error.len(),
                )
            },
            1
        );
        assert!(image.is_null());
    }

    #[test]
    fn sensor_handle_allows_empty_exif() {
        let image = sensor_handle(Vec::new());
        let mut exif = [1u8].as_ptr();
        let mut exif_length = 1;

        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_exif(image, &mut exif, &mut exif_length) },
            0
        );
        assert_eq!((exif.is_null(), exif_length), (true, 0));
        unsafe {
            rawdinal_sensor_v1_free(image);
        }
    }

    #[test]
    fn sensor_handle_keeps_copied_exif() {
        let image = sensor_handle(b"II*\0\x08\0\0\0".to_vec());
        let mut exif = ptr::null();
        let mut exif_length = 0;

        assert_eq!(
            unsafe { rawdinal_sensor_v1_get_exif(image, &mut exif, &mut exif_length) },
            0
        );
        assert_eq!(
            unsafe { slice::from_raw_parts(exif, exif_length) },
            b"II*\0\x08\0\0\0"
        );
        unsafe {
            rawdinal_sensor_v1_free(image);
        }
    }

    #[test]
    fn preview_failure_clears_borrowed_outputs() {
        let input = [0u8; 44];
        let mut preview = input.as_ptr();
        let mut length = 44;
        let result =
            unsafe { rawdinal_preview(input.as_ptr(), input.len(), &mut preview, &mut length) };
        assert_eq!((result, preview.is_null(), length), (1, true, 0));
    }

    #[test]
    #[ignore = "requires X3F_SAMPLE pointing to a full-resolution sd Quattro file"]
    fn sample_ffi_output_survives_input_release() {
        let bytes = std::fs::read(std::env::var_os("X3F_SAMPLE").expect("X3F_SAMPLE")).unwrap();
        let mut preview = ptr::null();
        let mut preview_size = 0;
        let status = unsafe {
            rawdinal_preview(bytes.as_ptr(), bytes.len(), &mut preview, &mut preview_size)
        };
        assert_eq!(status, 0);
        assert!(preview_size > 2 && preview_size < bytes.len());
        assert_eq!(unsafe { slice::from_raw_parts(preview, 2) }, [255, 216]);
        let mut image = ptr::null_mut();
        let mut info = ImageInfo::default();
        let mut error = [0 as c_char; 256];
        let status = unsafe {
            rawdinal_decode(
                bytes.as_ptr(),
                bytes.len(),
                &mut image,
                &mut info,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        assert_eq!(status, 0);
        drop(bytes);
        assert_eq!((info.width, info.height), (5424, 3616));
        assert!(info.exif_size > 8);
        assert_eq!(unsafe { slice::from_raw_parts(info.exif, 2) }, b"II");
        let mut rgba = vec![0.0; info.width as usize * info.height as usize * 4];
        assert_eq!(
            unsafe { rawdinal_copy_rgba(image, rgba.as_mut_ptr(), rgba.len()) },
            0
        );
        unsafe {
            rawdinal_free(image);
        }
        assert!(
            rgba.chunks_exact(4)
                .all(|pixel| pixel.iter().all(|value| value.is_finite()) && pixel[3] == 1.0)
        );
    }

    #[test]
    #[ignore = "requires X3F_SAMPLE_DIR containing 164 supported X3F files"]
    fn sensor_corpus_outputs_survive_input_release() {
        let directory =
            std::path::PathBuf::from(std::env::var_os("X3F_SAMPLE_DIR").expect("X3F_SAMPLE_DIR"));
        let mut paths = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
            .map(|entry| {
                entry
                    .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
                    .path()
            })
            .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("x3f"))
            .collect::<Vec<_>>();
        paths.sort();

        for path in &paths {
            let bytes =
                std::fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let mut image = ptr::null_mut();
            let mut error = [0 as c_char; 256];
            let status = unsafe {
                rawdinal_sensor_v1_decode(
                    bytes.as_ptr(),
                    bytes.len(),
                    &mut image,
                    error.as_mut_ptr(),
                    error.len(),
                )
            };
            assert_eq!(status, 0, "{}: {}", path.display(), error_text(&error));
            drop(bytes);

            for identity in 0..3 {
                let mut plane = SensorPlane::default();
                assert_eq!(
                    unsafe { rawdinal_sensor_v1_get_plane(image, identity, &mut plane) },
                    0,
                    "{}: unable to retrieve plane {identity}",
                    path.display()
                );
                assert!(
                    !plane.samples.is_null() && plane.width > 0 && plane.height > 0,
                    "{}: plane {identity} has empty data",
                    path.display()
                );
                assert_eq!(
                    (
                        plane.identity,
                        plane.reserved,
                        plane.stride_samples,
                        plane.sample_count
                    ),
                    (identity, 0, plane.width, plane.width * plane.height),
                    "{}: inconsistent plane {identity} layout",
                    path.display()
                );
                let samples = unsafe { slice::from_raw_parts(plane.samples, plane.sample_count) };
                assert!(!samples.is_empty());
            }

            let mut camf = ptr::null();
            let mut camf_length = 0;
            assert_eq!(
                unsafe { rawdinal_sensor_v1_get_camf(image, &mut camf, &mut camf_length) },
                0,
                "{}: unable to retrieve CAMF",
                path.display()
            );
            assert!(
                !camf.is_null() && camf_length > 0,
                "{}: empty CAMF data",
                path.display()
            );
            let camf = unsafe { slice::from_raw_parts(camf, camf_length) };
            assert!(!camf.is_empty());

            let mut exif = ptr::null();
            let mut exif_length = 0;
            assert_eq!(
                unsafe { rawdinal_sensor_v1_get_exif(image, &mut exif, &mut exif_length) },
                0,
                "{}: unable to retrieve EXIF",
                path.display()
            );
            assert_eq!(
                exif.is_null(),
                exif_length == 0,
                "{}: inconsistent EXIF data",
                path.display()
            );
            if exif_length > 0 {
                assert!(!unsafe { slice::from_raw_parts(exif, exif_length) }.is_empty());
            }

            unsafe {
                rawdinal_sensor_v1_free(image);
            }
        }
        assert_eq!(
            paths.len(),
            164,
            "{}: unexpected X3F file count",
            directory.display()
        );
    }

    #[test]
    fn invalid_input_clears_output_and_terminates_error_buffer() {
        let mut image = ptr::null_mut();
        let mut info = ImageInfo {
            width: 99,
            height: 99,
            ..ImageInfo::default()
        };
        let mut error = [b'x' as c_char; 5];
        let bytes = [0u8; 44];
        let result = unsafe {
            rawdinal_decode(
                bytes.as_ptr(),
                bytes.len(),
                &mut image,
                &mut info,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        assert_eq!(
            (result, image.is_null(), info.width, info.height, error[4]),
            (1, true, 0, 0, 0)
        );
    }

    #[test]
    fn copy_rejects_short_buffers_without_writing() {
        let image = Image {
            pixels: LinearImage {
                width: 1,
                height: 1,
                rgb: vec![[-0.1, 2.0, 0.3]],
            },
            exif: Vec::new(),
        };
        let mut output = [7.0; 3];
        let result = unsafe { rawdinal_copy_rgba(&image, output.as_mut_ptr(), output.len()) };
        assert_eq!((result, output), (1, [7.0; 3]));
    }

    #[test]
    fn rgba_copy_preserves_unbounded_samples_and_initializes_alpha() {
        let image = Image {
            pixels: LinearImage {
                width: 1,
                height: 1,
                rgb: vec![[-0.1, 2.0, 0.3]],
            },
            exif: Vec::new(),
        };
        let mut output = [0.0; 4];
        let result = unsafe { rawdinal_copy_rgba(&image, output.as_mut_ptr(), output.len()) };
        assert_eq!((result, output), (0, [-0.1, 2.0, 0.3, 1.0]));
    }
}
