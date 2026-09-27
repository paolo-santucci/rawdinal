// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! C ownership boundary for sensor-domain X3F decoding and experimental rendering.
//! All parsing and processing live in the safe Rust crate; each exported call contains
//! its own unwind boundary.

use rawdinal::{
    SensorImage as DecodedSensorImage, X3f,
    experimental::{LinearImage, Reconstruction, render},
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
