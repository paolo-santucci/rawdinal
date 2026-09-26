// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! C ownership boundary for the experimental X3F renderer. All parsing and processing
//! live in the safe Rust crate; each exported call contains its own unwind boundary.

use rawdinal::{
    X3f,
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
#[derive(Default)]
pub struct ImageInfo {
    width: u32,
    height: u32,
    exif: *const u8,
    exif_size: usize,
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
