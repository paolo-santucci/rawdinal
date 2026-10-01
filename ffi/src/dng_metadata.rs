use super::{RAW_V1_INVALID_INPUT, RAW_V1_OK, RAW_V1_PANIC, RawV1Image, raw_v1_status};
use rawdinal::{DecodeError, DngDecodedImage};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
};

#[repr(C)]
pub struct RawMetadataV1Info {
    version: u32,
    root_offset: u32,
    raw_offset: u32,
    valid_area: [u32; 4],
    crop_origin_present: u32,
    crop_size_present: u32,
    crop_origin: [f64; 2],
    crop_size: [f64; 2],
    directory_count: usize,
    encoded_samples: *const u16,
    encoded_sample_count: usize,
    sample_flags: *const u8,
    sample_flag_count: usize,
    mask_count: usize,
}

impl Default for RawMetadataV1Info {
    fn default() -> Self {
        Self {
            version: 0,
            root_offset: 0,
            raw_offset: 0,
            valid_area: [0; 4],
            crop_origin_present: 0,
            crop_size_present: 0,
            crop_origin: [0.0; 2],
            crop_size: [0.0; 2],
            directory_count: 0,
            encoded_samples: ptr::null(),
            encoded_sample_count: 0,
            sample_flags: ptr::null(),
            sample_flag_count: 0,
            mask_count: 0,
        }
    }
}

#[repr(C)]
#[derive(Default)]
pub struct RawDirectoryV1Info {
    offset: u32,
    parent_offset: u32,
    tag_count: usize,
}

#[repr(C)]
pub struct RawTagV1Info {
    id: u16,
    field_type: u16,
    count: u32,
    data: *const u8,
    byte_count: usize,
}

impl Default for RawTagV1Info {
    fn default() -> Self {
        Self {
            id: 0,
            field_type: 0,
            count: 0,
            data: ptr::null(),
            byte_count: 0,
        }
    }
}

#[repr(C)]
pub struct RawMaskV1Info {
    ifd_offset: u32,
    width: u32,
    height: u32,
    sub_area_present: u32,
    sub_area: [u32; 4],
    name: *const u8,
    name_length: usize,
    instance_id: *const u8,
    instance_id_length: usize,
    samples: *const u8,
    sample_count: usize,
}

impl Default for RawMaskV1Info {
    fn default() -> Self {
        Self {
            ifd_offset: 0,
            width: 0,
            height: 0,
            sub_area_present: 0,
            sub_area: [0; 4],
            name: ptr::null(),
            name_length: 0,
            instance_id: ptr::null(),
            instance_id_length: 0,
            samples: ptr::null(),
            sample_count: 0,
        }
    }
}

unsafe fn descriptor<T: Default>(
    image: *const RawV1Image,
    output: *mut T,
    read: impl FnOnce(&DngDecodedImage) -> Result<T, DecodeError>,
) -> i32 {
    if !output.is_null() {
        unsafe { output.write(T::default()) };
    }
    if image.is_null() || output.is_null() {
        return RAW_V1_INVALID_INPUT;
    }
    catch_unwind(AssertUnwindSafe(|| {
        match read(unsafe { &(*image).image }) {
            Ok(value) => {
                unsafe { output.write(value) };
                RAW_V1_OK
            }
            Err(error) => super::raw_v1_status(error),
        }
    }))
    .unwrap_or(RAW_V1_PANIC)
}

fn borrowed<T>(values: Option<&[T]>) -> (*const T, usize) {
    values
        .filter(|values| !values.is_empty())
        .map_or((ptr::null(), 0), |values| (values.as_ptr(), values.len()))
}

/// Borrows owned metadata, exact geometry and optional diagnostic buffers.
///
/// # Safety
/// `image` is live; `output` is writable and disjoint from handle storage. Borrowed arrays expire
/// with the handle. Do not access or free the handle concurrently. Output is cleared on error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_get_metadata(
    image: *const RawV1Image,
    output: *mut RawMetadataV1Info,
) -> i32 {
    unsafe {
        descriptor(image, output, |image| {
            let (encoded_samples, encoded_sample_count) =
                borrowed(image.encoded_samples.as_deref());
            let (sample_flags, sample_flag_count) = borrowed(image.sample_flags.as_deref());
            Ok(RawMetadataV1Info {
                version: 1,
                root_offset: image.metadata.root_offset,
                raw_offset: image.metadata.raw_offset,
                valid_area: image.valid_area,
                crop_origin_present: u32::from(image.default_crop_origin_exact.is_some()),
                crop_size_present: u32::from(image.default_crop_size_exact.is_some()),
                crop_origin: image.default_crop_origin_exact.unwrap_or([0.0; 2]),
                crop_size: image.default_crop_size_exact.unwrap_or([0.0; 2]),
                directory_count: image.metadata.directories.len(),
                encoded_samples,
                encoded_sample_count,
                sample_flags,
                sample_flag_count,
                mask_count: image.semantic_masks.len(),
            })
        })
    }
}

/// Borrows an IFD descriptor by enumeration index.
///
/// # Safety
/// Same lifetime, disjoint-storage and concurrency requirements as `rawdinal_raw_v1_get_metadata`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_get_directory(
    image: *const RawV1Image,
    index: usize,
    output: *mut RawDirectoryV1Info,
) -> i32 {
    unsafe {
        descriptor(image, output, |image| {
            let directory = image
                .metadata
                .directories
                .get(index)
                .ok_or(DecodeError::InvalidTag)?;
            Ok(RawDirectoryV1Info {
                offset: directory.offset,
                parent_offset: directory.parent_offset.unwrap_or(0),
                tag_count: directory.tags.len(),
            })
        })
    }
}

/// Borrows a tag by directory and tag enumeration indices. Payload uses TIFF byte order.
///
/// # Safety
/// Same lifetime, disjoint-storage and concurrency requirements as `rawdinal_raw_v1_get_metadata`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_get_tag(
    image: *const RawV1Image,
    directory: usize,
    index: usize,
    output: *mut RawTagV1Info,
) -> i32 {
    unsafe {
        descriptor(image, output, |image| {
            let tag = image
                .metadata
                .directories
                .get(directory)
                .and_then(|directory| directory.tags.get(index))
                .ok_or(DecodeError::InvalidTag)?;
            Ok(RawTagV1Info {
                id: tag.id,
                field_type: tag.field_type,
                count: tag.count,
                data: tag.data.as_ptr(),
                byte_count: tag.data.len(),
            })
        })
    }
}

/// Converts a numeric tag to caller-owned doubles; count must exactly equal the TIFF count.
///
/// # Safety
/// `image` is live and `output` points to `count` writable doubles disjoint from the handle.
/// Do not access or release the handle concurrently. Output remains untouched on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_copy_tag_numbers(
    image: *const RawV1Image,
    directory: usize,
    index: usize,
    output: *mut f64,
    count: usize,
) -> i32 {
    if image.is_null() || output.is_null() {
        return RAW_V1_INVALID_INPUT;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let image = unsafe { &(*image).image };
        let Some(tag) = image
            .metadata
            .directories
            .get(directory)
            .and_then(|directory| directory.tags.get(index))
        else {
            return RAW_V1_INVALID_INPUT;
        };
        if count != tag.count as usize || count > isize::MAX as usize / size_of::<f64>() {
            return RAW_V1_INVALID_INPUT;
        }
        match tag.numbers(image.metadata.byte_order) {
            Ok(values) => {
                unsafe { slice::from_raw_parts_mut(output, count) }.copy_from_slice(&values);
                RAW_V1_OK
            }
            Err(error) => raw_v1_status(error),
        }
    }))
    .unwrap_or(RAW_V1_PANIC)
}

/// Borrows a decoded semantic mask, requested through the extended decode flags.
///
/// # Safety
/// Same lifetime, disjoint-storage and concurrency requirements as `rawdinal_raw_v1_get_metadata`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rawdinal_raw_v1_get_mask(
    image: *const RawV1Image,
    index: usize,
    output: *mut RawMaskV1Info,
) -> i32 {
    unsafe {
        descriptor(image, output, |image| {
            let mask = image
                .semantic_masks
                .get(index)
                .ok_or(DecodeError::InvalidTag)?;
            let (name, name_length) = borrowed(mask.name.as_ref().map(|text| text.as_bytes()));
            let (instance_id, instance_id_length) =
                borrowed(mask.instance_id.as_ref().map(|text| text.as_bytes()));
            Ok(RawMaskV1Info {
                ifd_offset: mask.ifd_offset,
                width: mask.width,
                height: mask.height,
                sub_area_present: u32::from(mask.sub_area.is_some()),
                sub_area: mask.sub_area.unwrap_or([0; 4]),
                name,
                name_length,
                instance_id,
                instance_id_length,
                samples: mask.samples.as_ptr(),
                sample_count: mask.samples.len(),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RawDirectoryV1Info, RawMaskV1Info, RawMetadataV1Info, RawTagV1Info,
        rawdinal_raw_v1_copy_tag_numbers, rawdinal_raw_v1_get_directory, rawdinal_raw_v1_get_mask,
        rawdinal_raw_v1_get_metadata, rawdinal_raw_v1_get_tag,
    };
    use crate::{RAW_V1_INVALID_INPUT, RAW_V1_OK};
    use std::ptr;

    #[test]
    fn metadata_accessors_borrow_owned_data_and_convert_rationals() {
        let image = crate::tests::raw_v1_handle();
        let metadata = unsafe { &mut (*image).image.metadata };
        metadata.directories[0].tags.push(rawdinal::DngTag {
            id: 50728,
            field_type: 5,
            count: 1,
            data: [1u32.to_le_bytes(), 2u32.to_le_bytes()].concat(),
        });
        let mut metadata = RawMetadataV1Info::default();
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_metadata(image, &mut metadata) },
            RAW_V1_OK
        );
        assert_eq!(
            (
                metadata.version,
                metadata.directory_count,
                metadata.raw_offset
            ),
            (1, 1, 8)
        );
        assert_eq!(metadata.crop_size, [2.0, 1.0]);
        assert!(metadata.encoded_samples.is_null());
        let mut directory = RawDirectoryV1Info::default();
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_directory(image, 0, &mut directory) },
            RAW_V1_OK
        );
        assert_eq!(directory.tag_count, 1);
        let mut tag = RawTagV1Info::default();
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_tag(image, 0, 0, &mut tag) },
            RAW_V1_OK
        );
        assert_eq!((tag.id, tag.count, tag.byte_count), (50728, 1, 8));
        let mut value = -1.0;
        assert_eq!(
            unsafe { rawdinal_raw_v1_copy_tag_numbers(image, 0, 0, &mut value, 1) },
            RAW_V1_OK
        );
        assert_eq!(value, 0.5);
        assert_eq!(
            unsafe { rawdinal_raw_v1_copy_tag_numbers(image, 0, 0, &mut value, 2) },
            RAW_V1_INVALID_INPUT
        );
        assert_eq!(value, 0.5);
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_tag(image, 0, 1, &mut tag) },
            RAW_V1_INVALID_INPUT
        );
        assert!(tag.data.is_null());
        assert_eq!(tag.byte_count, 0);
        unsafe { crate::rawdinal_raw_v1_free(image) };
    }

    #[test]
    fn metadata_null_and_bad_index_paths_clear_outputs() {
        let image = crate::tests::raw_v1_handle();
        let mut mask = RawMaskV1Info {
            width: 10,
            ..Default::default()
        };
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_mask(image, 0, &mut mask) },
            RAW_V1_INVALID_INPUT
        );
        assert_eq!(mask.width, 0);
        let mut metadata = RawMetadataV1Info {
            version: 10,
            ..Default::default()
        };
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_metadata(ptr::null(), &mut metadata) },
            RAW_V1_INVALID_INPUT
        );
        assert_eq!(metadata.version, 0);
        assert_eq!(
            unsafe { rawdinal_raw_v1_get_metadata(image, ptr::null_mut()) },
            RAW_V1_INVALID_INPUT
        );
        unsafe { crate::rawdinal_raw_v1_free(image) };
    }
}
