use crate::DngMetadata;
use crate::dng_codec::{self, TileSpec};
use crate::dng_metadata::{allocate, own_metadata};
use crate::dng_normalize::Normalization;
use crate::dng_opcode::{OpcodeList, Stage};
use crate::probe::{ByteOrder, DecodeError, DecodeLimits, DngRawFacts, ProbeResult, dng_raw_facts};

const PHOTOMETRIC_LINEAR_RAW: u16 = 34_892;

/// A parsed, bounded DNG container supporting the narrow Apple ProRAW tiled layout.
#[derive(Debug)]
pub struct Dng<'a> {
    bytes: &'a [u8],
    facts: DngRawFacts,
    limits: DecodeLimits,
}

/// Whether a DNG processing operation has been applied to [`LinearRawImage`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearRawProcessing {
    NotPresent,
    Applied,
    Unapplied,
    Unknown,
    SkippedOptional,
    PartiallyApplied,
}

/// Optional diagnostic storage. Defaults avoid retaining another full-resolution sample buffer.
#[derive(Debug, Clone, Copy, Default)]
pub struct DngDecodeOptions {
    pub retain_encoded_samples: bool,
    pub retain_sample_flags: bool,
    pub decode_semantic_masks: bool,
}

/// One 8-bit semantic weight image, independent of the primary RGB calibration.
#[derive(Debug, Clone, PartialEq)]
pub struct DngSemanticMask {
    pub ifd_offset: u32,
    pub width: u32,
    pub height: u32,
    pub name: Option<String>,
    pub instance_id: Option<String>,
    /// `[top, left, full_width, full_height]`; absent means the stored mask fills its canvas.
    pub sub_area: Option<[u32; 4]>,
    pub samples: Vec<u8>,
}

/// Processing state of a decoded camera-native linear raw image.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinearRawProcessingState {
    pub linearization: LinearRawProcessing,
    pub black_subtraction: LinearRawProcessing,
    pub white_normalization: LinearRawProcessing,
    pub white_balance: LinearRawProcessing,
    pub color_conversion: LinearRawProcessing,
    pub default_crop: LinearRawProcessing,
    pub orientation: LinearRawProcessing,
    pub baseline_exposure: LinearRawProcessing,
    pub profile_tone_curve: LinearRawProcessing,
    pub demosaic: LinearRawProcessing,
    pub opcode_list_1: LinearRawProcessing,
    pub opcode_list_2: LinearRawProcessing,
    pub opcode_list_3: LinearRawProcessing,
    pub profile_gain_table_map: LinearRawProcessing,
    pub semantic_masks: LinearRawProcessing,
}

impl Default for LinearRawProcessingState {
    fn default() -> Self {
        Self {
            linearization: LinearRawProcessing::NotPresent,
            black_subtraction: LinearRawProcessing::NotPresent,
            white_normalization: LinearRawProcessing::NotPresent,
            white_balance: LinearRawProcessing::NotPresent,
            color_conversion: LinearRawProcessing::NotPresent,
            default_crop: LinearRawProcessing::NotPresent,
            orientation: LinearRawProcessing::NotPresent,
            baseline_exposure: LinearRawProcessing::NotPresent,
            profile_tone_curve: LinearRawProcessing::NotPresent,
            demosaic: LinearRawProcessing::NotPresent,
            opcode_list_1: LinearRawProcessing::NotPresent,
            opcode_list_2: LinearRawProcessing::NotPresent,
            opcode_list_3: LinearRawProcessing::NotPresent,
            profile_gain_table_map: LinearRawProcessing::NotPresent,
            semantic_masks: LinearRawProcessing::NotPresent,
        }
    }
}

/// A camera-native image after DNG linearization, black subtraction, and white normalization.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearRawImage {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub component_ids: Vec<u8>,
    pub dng_version: [u8; 4],
    pub dng_backward_version: Option<[u8; 4]>,
    pub tiff_byte_order: ByteOrder,
    pub make: String,
    pub model: Option<String>,
    pub colorimetric_reference: Option<u32>,
    /// Stored-image coordinates `[top, left, bottom, right]`; the image is not cropped.
    pub active_area: [u32; 4],
    pub orientation: Option<u16>,
    /// `[x, y]` relative to `active_area`; the image is not cropped.
    pub default_crop_origin: Option<[u32; 2]>,
    /// `[width, height]` within `active_area`; the image is not cropped.
    pub default_crop_size: Option<[u32; 2]>,
    pub profile_gain_table_map: Option<Vec<u8>>,
    pub samples: Vec<f32>,
    pub processing: LinearRawProcessingState,
}

/// Extended DNG result, retaining scoped metadata and optional diagnostics.
/// The legacy pixel descriptor remains available through `image` and dereferencing.
#[derive(Debug, Clone, PartialEq)]
pub struct DngDecodedImage {
    pub image: LinearRawImage,
    pub metadata: DngMetadata,
    /// Exact rational crop metadata, including fractional values omitted by legacy integer fields.
    pub default_crop_origin_exact: Option<[f64; 2]>,
    pub default_crop_size_exact: Option<[f64; 2]>,
    /// Region containing valid processed pixels after active-area and opcode trimming.
    /// Numerical values outside this region are unspecified and must be excluded from processing.
    pub valid_area: [u32; 4],
    /// Original decompressed, assembled codes before opcode list 1 and linearization.
    pub encoded_samples: Option<Vec<u16>>,
    /// Per-sample bits before opcodes: 1 encoded maximum, 2 `L(original_code) >= WhiteLevel`,
    /// 4 outside active area. Bit 8 is an image-wide indication that opcode processing occurred,
    /// not a per-sample modification mask. None implies measured photosite saturation.
    pub sample_flags: Option<Vec<u8>>,
    pub semantic_masks: Vec<DngSemanticMask>,
}

impl std::ops::Deref for DngDecodedImage {
    type Target = LinearRawImage;
    fn deref(&self) -> &Self::Target {
        &self.image
    }
}

impl std::ops::DerefMut for DngDecodedImage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.image
    }
}

impl<'a> Dng<'a> {
    /// Parses a DNG using default bounded decode limits.
    pub fn parse(bytes: &'a [u8]) -> ProbeResult<Self> {
        Self::parse_with_limits(bytes, DecodeLimits::default())
    }

    /// Parses a DNG using caller-provided bounded decode limits.
    pub fn parse_with_limits(bytes: &'a [u8], limits: DecodeLimits) -> ProbeResult<Self> {
        let mut facts = dng_raw_facts(bytes, limits)?;
        configure_storage(&mut facts)?;
        validate_layout(bytes, &facts, limits)?;
        Ok(Self {
            bytes,
            facts,
            limits,
        })
    }

    /// Decodes camera-native scene-referred samples with the legacy integer geometry descriptor.
    /// Fractional crop or opcode trimming requires [`Self::decode_with_options`].
    pub fn decode(&self) -> ProbeResult<LinearRawImage> {
        let decoded = self.decode_with_options(DngDecodeOptions::default())?;
        if decoded.valid_area != decoded.active_area
            || (decoded.default_crop_origin_exact.is_some()
                && decoded.default_crop_origin.is_none())
            || (decoded.default_crop_size_exact.is_some() && decoded.default_crop_size.is_none())
        {
            return Err(DecodeError::UnsupportedFeature);
        }
        Ok(decoded.image)
    }

    fn decode_codes(&self) -> ProbeResult<(Vec<f32>, Vec<u8>)> {
        let tile_width = usize::try_from(
            self.facts
                .tile_width
                .ok_or(DecodeError::UnsupportedFeature)?,
        )
        .map_err(|_| DecodeError::InvalidGeometry)?;
        let tile_length = usize::try_from(
            self.facts
                .tile_length
                .ok_or(DecodeError::UnsupportedFeature)?,
        )
        .map_err(|_| DecodeError::InvalidGeometry)?;
        let width = usize::try_from(self.facts.width).map_err(|_| DecodeError::InvalidGeometry)?;
        let height =
            usize::try_from(self.facts.height).map_err(|_| DecodeError::InvalidGeometry)?;
        let offsets = self
            .facts
            .tile_offsets
            .as_ref()
            .ok_or(DecodeError::UnsupportedFeature)?;
        let byte_counts = self
            .facts
            .tile_byte_counts
            .as_ref()
            .ok_or(DecodeError::UnsupportedFeature)?;
        let channels = usize::from(self.facts.samples_per_pixel);
        let count = width
            .checked_mul(height)
            .and_then(|value| value.checked_mul(channels))
            .ok_or(DecodeError::ResourceLimit)?;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(count)
            .map_err(|_| DecodeError::Allocation)?;
        samples.resize(count, 0.0);
        let columns = width.div_ceil(tile_width);
        let blocks_per_plane = columns * height.div_ceil(tile_length);
        let planar = self.facts.planar_configuration == 2;
        let frame_channels = if planar { 1 } else { channels };
        let mut component_ids = if planar { Some(vec![1, 2, 3]) } else { None };
        for (tile, (&offset, &byte_count)) in offsets.iter().zip(byte_counts).enumerate() {
            let plane = if planar { tile / blocks_per_plane } else { 0 };
            let tile = tile % blocks_per_plane;
            let payload = checked_payload(self.bytes, offset, byte_count)?;
            let block_height = if self.facts.has_strip_offsets {
                tile_length.min(height - tile * tile_length)
            } else {
                tile_length
            };
            let frame = dng_codec::decode(
                payload,
                TileSpec {
                    width: tile_width as u32,
                    height: block_height as u32,
                    channels: frame_channels as u16,
                    bits: self.facts.bits_per_sample[0],
                    compression: self.facts.compression,
                    order: self.facts.byte_order,
                },
                self.limits,
            )?;
            if !planar {
                if let Some(expected) = &component_ids {
                    if expected != &frame.component_ids {
                        return Err(DecodeError::UnsupportedFeature);
                    }
                } else {
                    component_ids = Some(frame.component_ids.clone());
                }
            }
            let tile_x = (tile % columns)
                .checked_mul(tile_width)
                .ok_or(DecodeError::ResourceLimit)?;
            let tile_y = (tile / columns)
                .checked_mul(tile_length)
                .ok_or(DecodeError::ResourceLimit)?;
            let copy_width = (width - tile_x).min(tile_width);
            let copy_height = (height - tile_y).min(tile_length);
            for y in 0..copy_height {
                let source = y * tile_width * frame_channels;
                let destination = ((tile_y + y) * width + tile_x) * channels + plane;
                for x in 0..copy_width {
                    for channel in 0..frame_channels {
                        samples[destination + x * channels + channel] =
                            f32::from(frame.samples[source + x * frame_channels + channel]);
                    }
                }
            }
        }
        reorder_interleaved_image(&mut samples, &self.facts)?;
        Ok((samples, component_ids.ok_or(DecodeError::InvalidContainer)?))
    }

    /// Decodes extended geometry, scoped metadata and optional diagnostic buffers.
    pub fn decode_with_options(&self, options: DngDecodeOptions) -> ProbeResult<DngDecodedImage> {
        let (mut samples, component_ids) = self.decode_codes()?;
        let count = samples.len();
        let normalization = Normalization::new(&self.facts)?;
        let encoded_samples = if options.retain_encoded_samples {
            let mut codes = allocate(count)?;
            codes.extend(samples.iter().map(|&value| value as u16));
            Some(codes)
        } else {
            None
        };
        let mut sample_flags = if options.retain_sample_flags {
            Some(normalization.flags(&samples)?)
        } else {
            None
        };
        let active = self
            .facts
            .active_area
            .unwrap_or([0, 0, self.facts.height, self.facts.width]);
        let lists = self.opcode_lists()?;
        let mut first = Stage {
            samples: &mut samples,
            width: self.facts.width,
            origin: [0, 0],
            bounds: [0, 0, self.facts.height, self.facts.width],
            number: 1,
        };
        if let Some(list) = &lists[0] {
            list.apply(&mut first)?;
        }
        if first.bounds[0] > active[0]
            || first.bounds[1] > active[1]
            || first.bounds[2] < active[2]
            || first.bounds[3] < active[3]
        {
            return Err(DecodeError::InvalidGeometry);
        }
        normalization.apply(&mut samples)?;
        let mut stage = Stage {
            samples: &mut samples,
            width: self.facts.width,
            origin: [active[0], active[1]],
            bounds: [0, 0, active[2] - active[0], active[3] - active[1]],
            number: 2,
        };
        for (index, list) in lists.iter().enumerate().skip(1) {
            stage.number = index as u8 + 1;
            if let Some(list) = list {
                list.apply(&mut stage)?;
            }
        }
        let valid_area = [
            stage.bounds[0] + active[0],
            stage.bounds[1] + active[1],
            stage.bounds[2] + active[0],
            stage.bounds[3] + active[1],
        ];
        let opcode_processing = lists.each_ref().map(|list| {
            list.as_ref()
                .map_or(LinearRawProcessing::NotPresent, OpcodeList::processing)
        });
        if opcode_processing.iter().any(|state| {
            matches!(
                state,
                LinearRawProcessing::Applied | LinearRawProcessing::PartiallyApplied
            )
        }) {
            if let Some(flags) = &mut sample_flags {
                for flag in flags {
                    *flag |= 8;
                }
            }
        }
        let profile_gain_table_map = match &self.facts.profile_gain_table_map {
            Some(range) => {
                let mut bytes = Vec::new();
                bytes
                    .try_reserve_exact(range.len())
                    .map_err(|_| DecodeError::Allocation)?;
                bytes.extend_from_slice(&self.bytes[range.clone()]);
                Some(bytes)
            }
            None => None,
        };
        let metadata = own_metadata(self.bytes, &self.facts)?;
        let has_gain_table =
            self.facts.profile_gain_table_map.is_some() || metadata.root().tag(52544).is_some();
        Ok(DngDecodedImage {
            image: LinearRawImage {
                width: self.facts.width,
                height: self.facts.height,
                channels: self.facts.samples_per_pixel as u8,
                component_ids,
                dng_version: self.facts.dng_version,
                dng_backward_version: self.facts.dng_backward_version,
                tiff_byte_order: self.facts.byte_order,
                make: self
                    .facts
                    .make
                    .clone()
                    .ok_or(DecodeError::UnsupportedFeature)?,
                model: self.facts.model.clone(),
                colorimetric_reference: self.facts.colorimetric_reference,
                active_area: self.facts.active_area.unwrap_or([
                    0,
                    0,
                    self.facts.height,
                    self.facts.width,
                ]),
                orientation: self
                    .facts
                    .orientation
                    .map(|value| u16::try_from(value).map_err(|_| DecodeError::UnsupportedFeature))
                    .transpose()?,
                default_crop_origin: self.facts.default_crop_origin,
                default_crop_size: self.facts.default_crop_size,
                profile_gain_table_map,
                samples,
                processing: LinearRawProcessingState {
                    linearization: if self.facts.linearization_table.is_some() {
                        LinearRawProcessing::Applied
                    } else {
                        LinearRawProcessing::NotPresent
                    },
                    black_subtraction: LinearRawProcessing::Applied,
                    white_normalization: LinearRawProcessing::Applied,
                    white_balance: LinearRawProcessing::Unapplied,
                    color_conversion: LinearRawProcessing::Unapplied,
                    default_crop: if self.facts.active_area.is_some()
                        || self.facts.default_crop_origin_exact.is_some()
                        || self.facts.default_crop_size_exact.is_some()
                    {
                        LinearRawProcessing::Unapplied
                    } else {
                        LinearRawProcessing::NotPresent
                    },
                    orientation: if self.facts.orientation.is_some_and(|value| value != 1) {
                        LinearRawProcessing::Unapplied
                    } else {
                        LinearRawProcessing::NotPresent
                    },
                    baseline_exposure: if self.facts.has_baseline_exposure {
                        LinearRawProcessing::Unapplied
                    } else {
                        LinearRawProcessing::NotPresent
                    },
                    profile_tone_curve: if self.facts.has_profile_tone_curve {
                        LinearRawProcessing::Unapplied
                    } else {
                        LinearRawProcessing::NotPresent
                    },
                    demosaic: LinearRawProcessing::NotPresent,
                    opcode_list_1: opcode_processing[0],
                    opcode_list_2: opcode_processing[1],
                    opcode_list_3: opcode_processing[2],
                    profile_gain_table_map: if has_gain_table {
                        LinearRawProcessing::Unapplied
                    } else {
                        LinearRawProcessing::NotPresent
                    },
                    semantic_masks: if self.facts.has_semantic_masks {
                        LinearRawProcessing::Unapplied
                    } else {
                        LinearRawProcessing::NotPresent
                    },
                },
            },
            metadata,
            default_crop_origin_exact: self.facts.default_crop_origin_exact,
            default_crop_size_exact: self.facts.default_crop_size_exact,
            valid_area,
            encoded_samples,
            sample_flags,
            semantic_masks: if options.decode_semantic_masks {
                self.decode_semantic_masks()?
            } else {
                Vec::new()
            },
        })
    }

    fn opcode_lists(&self) -> ProbeResult<[Option<OpcodeList>; 3]> {
        let parse = |index: usize| {
            self.facts.opcode_lists[index]
                .as_ref()
                .map(|range| OpcodeList::parse(&self.bytes[range.clone()]))
                .transpose()
        };
        Ok([parse(0)?, parse(1)?, parse(2)?])
    }

    /// Decodes semantic weights without applying RGB calibration, crop, orientation or resampling.
    pub fn decode_semantic_masks(&self) -> ProbeResult<Vec<DngSemanticMask>> {
        crate::dng_mask::decode(
            self.bytes,
            &own_metadata(self.bytes, &self.facts)?,
            self.limits,
        )
    }

    /// Borrows the first complete JPEG preview from a reduced-resolution image IFD.
    /// The JPEG is not decoded or color transformed and the returned bytes borrow this DNG's input.
    pub fn jpeg_preview(&self) -> ProbeResult<Option<&'a [u8]>> {
        for directory in &self.facts.directories {
            let scalar = |id| -> ProbeResult<Option<u32>> {
                let Some(tag) = directory.tags.iter().find(|tag| tag.id == id) else {
                    return Ok(None);
                };
                if tag.count != 1 || !matches!(tag.field_type, 3 | 4) {
                    return Ok(None);
                }
                Ok(Some(crate::dng_metadata::number(
                    &self.bytes[tag.range.clone()],
                    tag.field_type,
                    0,
                    self.facts.byte_order,
                )? as u32))
            };
            if scalar(254)?.unwrap_or(0) & 1 == 0 {
                continue;
            }
            let pair = if let (Some(offset), Some(length)) = (scalar(513)?, scalar(514)?) {
                Some((offset, length))
            } else if matches!(scalar(259)?, Some(6 | 7)) && matches!(scalar(262)?, Some(2 | 6)) {
                scalar(273)?.zip(scalar(279)?)
            } else {
                None
            };
            if let Some((offset, length)) = pair {
                let payload = checked_payload(self.bytes, offset, length)?;
                if payload.starts_with(&[0xff, 0xd8]) && payload.ends_with(&[0xff, 0xd9]) {
                    return Ok(Some(payload));
                }
            }
        }
        Ok(None)
    }
}

fn configure_storage(facts: &mut DngRawFacts) -> ProbeResult<()> {
    if facts.has_strip_offsets {
        if facts.tile_offsets.is_some() || facts.tile_byte_counts.is_some() {
            return Err(DecodeError::UnsupportedFeature);
        }
        facts.tile_width = Some(facts.width);
        facts.tile_length = Some(
            facts
                .rows_per_strip
                .unwrap_or(facts.height)
                .min(facts.height),
        );
        facts.tile_offsets = facts.strip_offsets.take();
        facts.tile_byte_counts = facts.strip_byte_counts.take();
    }
    Ok(())
}

fn validate_layout(bytes: &[u8], facts: &DngRawFacts, limits: DecodeLimits) -> ProbeResult<()> {
    let directory = facts
        .directories
        .iter()
        .find(|directory| directory.offset == facts.raw_offset)
        .ok_or(DecodeError::InvalidContainer)?;
    for id in [266, 317] {
        if let Some(tag) = directory.tags.iter().find(|tag| tag.id == id) {
            if tag.count != 1 || tag.field_type != 3 {
                return Err(DecodeError::InvalidTag);
            }
            if crate::dng_metadata::number(
                &bytes[tag.range.clone()],
                tag.field_type,
                0,
                facts.byte_order,
            )? != 1.0
            {
                return Err(DecodeError::UnsupportedFeature);
            }
        }
    }
    if facts
        .orientation
        .is_some_and(|value| !(1..=8).contains(&value))
    {
        return Err(DecodeError::InvalidTag);
    }
    if facts.photometric != PHOTOMETRIC_LINEAR_RAW
        || !matches!(facts.compression, 1 | 7 | 34892 | 52546)
        || facts.dng_version < [1, 0, 0, 0]
        || facts.dng_version > [1, 7, 1, 0]
        || facts.dng_backward_version.is_some_and(|version| {
            !([1, 0, 0, 0]..=[1, 7, 1, 0]).contains(&version) || version > facts.dng_version
        })
        || facts.make.as_deref() != Some("Apple")
        || facts.colorimetric_reference.is_some_and(|value| value != 0)
        || facts
            .sub_tile_block_size
            .is_some_and(|value| value != [1, 1])
        || facts
            .row_interleave_factor
            .is_some_and(|value| value == 0 || value > facts.height)
        || facts
            .column_interleave_factor
            .is_some_and(|value| value == 0 || value > facts.width)
        || facts.samples_per_pixel != 3
        || facts.sample_format != [1; 3]
        || facts.bits_per_sample.len() != 3
        || facts
            .bits_per_sample
            .iter()
            .any(|&bits| bits != facts.bits_per_sample[0])
    {
        return Err(DecodeError::UnsupportedFeature);
    }
    let bits = facts.bits_per_sample[0];
    if !(8..=16).contains(&bits) || (facts.compression == 34892 && bits != 8) {
        return Err(DecodeError::UnsupportedFeature);
    }
    validate_geometry_metadata(facts)?;
    Normalization::new(facts)?;
    for range in facts.opcode_lists.iter().flatten() {
        OpcodeList::parse(&bytes[range.clone()])?;
    }
    let tile_width = facts.tile_width.ok_or(DecodeError::UnsupportedFeature)?;
    let tile_length = facts.tile_length.ok_or(DecodeError::UnsupportedFeature)?;
    if tile_width == 0
        || tile_length == 0
        || (matches!(facts.compression, 7 | 34892)
            && (tile_width > u32::from(u16::MAX) || tile_length > u32::from(u16::MAX)))
    {
        return Err(DecodeError::UnsupportedFeature);
    }
    let columns = facts.width.div_ceil(tile_width);
    let rows = facts.height.div_ceil(tile_length);
    let planes = if facts.planar_configuration == 2 {
        3
    } else {
        1
    };
    let expected_tiles = usize::try_from(
        u64::from(columns)
            .checked_mul(u64::from(rows))
            .and_then(|value| value.checked_mul(planes))
            .ok_or(DecodeError::ResourceLimit)?,
    )
    .map_err(|_| DecodeError::ResourceLimit)?;
    if facts.tile_offsets.as_ref().map(Vec::len) != Some(expected_tiles)
        || facts.tile_byte_counts.as_ref().map(Vec::len) != Some(expected_tiles)
    {
        return Err(DecodeError::UnsupportedFeature);
    }
    let decoded = u64::from(tile_width)
        .checked_mul(u64::from(tile_length))
        .and_then(|value| value.checked_mul(u64::from(facts.samples_per_pixel) / planes))
        .and_then(|value| value.checked_mul(u64::try_from(expected_tiles).ok()?))
        .ok_or(DecodeError::ResourceLimit)?;
    if decoded > limits.max_decoded_samples {
        return Err(DecodeError::ResourceLimit);
    }
    validate_tile_ranges(
        bytes,
        facts
            .tile_offsets
            .as_ref()
            .ok_or(DecodeError::UnsupportedFeature)?,
        facts
            .tile_byte_counts
            .as_ref()
            .ok_or(DecodeError::UnsupportedFeature)?,
        limits,
    )?;
    Ok(())
}

fn reorder_interleaved_image(samples: &mut Vec<f32>, facts: &DngRawFacts) -> ProbeResult<()> {
    let rows = facts.row_interleave_factor.unwrap_or(1) as usize;
    let columns = facts.column_interleave_factor.unwrap_or(1) as usize;
    if rows == 1 && columns == 1 {
        return Ok(());
    }
    let width = facts.width as usize;
    let height = facts.height as usize;
    let row_order = interleaved_order(height, rows)?;
    let column_order = interleaved_order(width, columns)?;
    let mut output = allocate(samples.len())?;
    output.resize(samples.len(), 0.0);
    for (row, &destination_row) in row_order.iter().enumerate() {
        for (column, &destination_column) in column_order.iter().enumerate() {
            let source = (row * width + column) * 3;
            let destination = (destination_row * width + destination_column) * 3;
            output[destination..destination + 3].copy_from_slice(&samples[source..source + 3]);
        }
    }
    *samples = output;
    Ok(())
}

fn interleaved_order(length: usize, factor: usize) -> ProbeResult<Vec<usize>> {
    let mut order = allocate(length)?;
    for phase in 0..factor {
        order.extend((phase..length).step_by(factor));
    }
    Ok(order)
}

fn validate_geometry_metadata(facts: &DngRawFacts) -> ProbeResult<()> {
    let active_area = facts
        .active_area
        .unwrap_or([0, 0, facts.height, facts.width]);
    let [top, left, bottom, right] = active_area;
    if top >= bottom || left >= right || bottom > facts.height || right > facts.width {
        return Err(DecodeError::InvalidGeometry);
    }
    let active_width = right - left;
    let active_height = bottom - top;
    if let Some([x, y]) = facts.default_crop_origin_exact {
        if x < 0.0 || y < 0.0 || x > f64::from(active_width) || y > f64::from(active_height) {
            return Err(DecodeError::InvalidGeometry);
        }
    }
    if let Some([width, height]) = facts.default_crop_size_exact {
        if width <= 0.0
            || height <= 0.0
            || width > f64::from(active_width)
            || height > f64::from(active_height)
        {
            return Err(DecodeError::InvalidGeometry);
        }
    }
    if let (Some([x, y]), Some([width, height])) = (
        facts.default_crop_origin_exact,
        facts.default_crop_size_exact,
    ) {
        if x + width > f64::from(active_width) || y + height > f64::from(active_height) {
            return Err(DecodeError::InvalidGeometry);
        }
    }
    Ok(())
}

fn validate_tile_ranges(
    bytes: &[u8],
    offsets: &[u32],
    counts: &[u32],
    limits: DecodeLimits,
) -> ProbeResult<()> {
    let mut ranges = Vec::new();
    ranges
        .try_reserve_exact(offsets.len())
        .map_err(|_| DecodeError::Allocation)?;
    let mut total = 0_u64;
    for (&offset, &count) in offsets.iter().zip(counts) {
        let start = usize::try_from(offset).map_err(|_| DecodeError::InvalidOffset)?;
        let count = usize::try_from(count).map_err(|_| DecodeError::InvalidOffset)?;
        let end = start.checked_add(count).ok_or(DecodeError::InvalidOffset)?;
        if end > bytes.len() {
            return Err(DecodeError::Truncated);
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| DecodeError::ResourceLimit)?)
            .ok_or(DecodeError::ResourceLimit)?;
        ranges.push((start, end));
    }
    if total > u64::try_from(limits.max_input_bytes).map_err(|_| DecodeError::ResourceLimit)? {
        return Err(DecodeError::ResourceLimit);
    }
    ranges.sort_unstable_by_key(|&(start, _)| start);
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(DecodeError::InvalidOffset);
    }
    Ok(())
}

fn checked_payload(bytes: &[u8], offset: u32, count: u32) -> ProbeResult<&[u8]> {
    let offset = usize::try_from(offset).map_err(|_| DecodeError::InvalidOffset)?;
    let count = usize::try_from(count).map_err(|_| DecodeError::InvalidOffset)?;
    let end = offset
        .checked_add(count)
        .ok_or(DecodeError::InvalidOffset)?;
    bytes.get(offset..end).ok_or(DecodeError::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IFD0: usize = 8;
    const RAW_IFD: usize = 100;
    const DATA: usize = 9_000;

    fn put16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }

    fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn entry(bytes: &mut [u8], offset: usize, tag: u16, kind: u16, count: u32, value: u32) {
        put16(bytes, offset, tag);
        put16(bytes, offset + 2, kind);
        put32(bytes, offset + 4, count);
        put32(
            bytes,
            offset + 8,
            if kind == 3 && count == 1 {
                value << 16
            } else if tag == 50_706 || tag == 50_707 {
                value.swap_bytes()
            } else {
                value
            },
        );
    }

    fn jpeg(differences: [i32; 3]) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xc3, 0, 17, 12, 0, 1, 0, 1, 3];
        bytes.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
        bytes.extend_from_slice(&[0xff, 0xc4, 0, 36, 0, 0, 0, 0, 0, 17]);
        bytes.extend_from_slice(&[0; 11]);
        bytes.extend(0..=16);
        bytes.extend_from_slice(&[0xff, 0xda, 0, 12, 3, 1, 0, 2, 0, 3, 0, 7, 0, 0]);
        let mut bits = Vec::new();
        for difference in differences {
            let category = if difference == 0 {
                0
            } else {
                32 - difference.unsigned_abs().leading_zeros()
            };
            bits.extend((0..5).rev().map(|shift| ((category >> shift) & 1) as u8));
            if category != 0 {
                let value = if difference < 0 {
                    difference + (1 << category) - 1
                } else {
                    difference
                };
                bits.extend(
                    (0..category)
                        .rev()
                        .map(|shift| ((value >> shift) & 1) as u8),
                );
            }
        }
        while bits.len() % 8 != 0 {
            bits.push(1);
        }
        for chunk in bits.chunks(8) {
            bytes.push(chunk.iter().fold(0, |value, bit| (value << 1) | bit));
        }
        bytes.extend_from_slice(&[0xff, 0xd9]);
        bytes
    }

    fn fixture() -> Vec<u8> {
        let payloads = [[0, 0, 0]; 4].map(jpeg);
        let offsets = [DATA, DATA + payloads[0].len(), 0, 0];
        let offsets = [
            offsets[0],
            offsets[1],
            offsets[1] + payloads[1].len(),
            offsets[1] + payloads[1].len() + payloads[2].len(),
        ];
        let counts = payloads.each_ref().map(|payload| payload.len());
        let array = 400;
        let lut = array + 32;
        let black = lut + 8192;
        let white = black + 6;
        let samples = white + 6;
        let mut bytes = vec![0; DATA + payloads.iter().map(Vec::len).sum::<usize>()];
        bytes[..2].copy_from_slice(b"MM");
        put16(&mut bytes, 2, 42);
        put32(&mut bytes, 4, IFD0 as u32);
        put16(&mut bytes, IFD0, 6);
        entry(&mut bytes, IFD0 + 2, 50_706, 1, 4, 0x0000_0601);
        entry(&mut bytes, IFD0 + 14, 50_707, 1, 4, 0x0000_0301);
        entry(&mut bytes, IFD0 + 26, 330, 4, 1, RAW_IFD as u32);
        entry(&mut bytes, IFD0 + 38, 254, 4, 1, 1);
        entry(&mut bytes, IFD0 + 50, 271, 2, 6, 392);
        entry(&mut bytes, IFD0 + 62, 274, 3, 1, 1);
        bytes[392..398].copy_from_slice(b"Apple\0");
        put16(&mut bytes, RAW_IFD, 20);
        let tags = [
            (254, 4, 1, 0),
            (256, 4, 1, 2),
            (257, 4, 1, 2),
            (258, 3, 3, samples as u32),
            (259, 3, 1, 7),
            (262, 3, 1, 34_892),
            (277, 3, 1, 3),
            (284, 3, 1, 1),
            (322, 4, 1, 1),
            (323, 4, 1, 1),
            (324, 4, 4, array as u32),
            (325, 4, 4, (array + 16) as u32),
            (339, 3, 3, (samples + 6) as u32),
            (50_712, 3, 4096, lut as u32),
            (50_714, 3, 3, black as u32),
            (50_717, 3, 3, white as u32),
            (50_713, 3, 2, 0x0001_0001),
            (50_829, 4, 4, 360),
            (50_719, 4, 2, 376),
            (50_720, 4, 2, 384),
        ];
        for (index, &(tag, kind, count, value)) in tags.iter().enumerate() {
            entry(
                &mut bytes,
                RAW_IFD + 2 + index * 12,
                tag,
                kind,
                count,
                value,
            );
        }
        for (index, (&offset, &count)) in offsets.iter().zip(counts.iter()).enumerate() {
            put32(&mut bytes, array + index * 4, offset as u32);
            put32(&mut bytes, array + 16 + index * 4, count as u32);
        }
        for index in 0..4096 {
            put16(&mut bytes, lut + index * 2, index as u16);
        }
        for index in 0..3 {
            put16(&mut bytes, black + index * 2, 2048);
            put16(&mut bytes, white + index * 2, 2050);
            put16(&mut bytes, samples + index * 2, 12);
            put16(&mut bytes, samples + 6 + index * 2, 1);
        }
        for (offset, value) in [
            (360, 0),
            (364, 0),
            (368, 2),
            (372, 2),
            (376, 0),
            (380, 0),
            (384, 2),
            (388, 2),
        ] {
            put32(&mut bytes, offset, value);
        }
        let mut cursor = DATA;
        for payload in payloads {
            bytes[cursor..cursor + payload.len()].copy_from_slice(&payload);
            cursor += payload.len();
        }
        bytes
    }

    #[test]
    fn selects_tiled_raw_subifd_and_normalizes_linearized_samples() {
        let mut bytes = fixture();
        put16(&mut bytes, 432 + 2048 * 2, 2049);
        let image = Dng::parse(&bytes).unwrap().decode().unwrap();
        assert_eq!((image.width, image.height, image.channels), (2, 2, 3));
        assert_eq!(image.component_ids, [1, 2, 3]);
        assert_eq!(image.tiff_byte_order, ByteOrder::BigEndian);
        assert_eq!(image.make, "Apple");
        assert_eq!(image.active_area, [0, 0, 2, 2]);
        assert_eq!(image.orientation, Some(1));
        assert_eq!(image.default_crop_origin, Some([0, 0]));
        assert_eq!(image.default_crop_size, Some([2, 2]));
        assert_eq!(
            image.processing.semantic_masks,
            LinearRawProcessing::NotPresent
        );
        assert_eq!(image.samples, [0.5; 12]);
        assert_eq!(
            image.processing.white_normalization,
            LinearRawProcessing::Applied
        );
        assert_eq!(
            image.processing.white_balance,
            LinearRawProcessing::Unapplied
        );
        assert_eq!(
            image.processing.color_conversion,
            LinearRawProcessing::Unapplied
        );
        assert_eq!(
            image.processing.default_crop,
            LinearRawProcessing::Unapplied
        );
        assert_eq!(
            image.processing.orientation,
            LinearRawProcessing::NotPresent
        );
    }

    #[test]
    fn rejects_bad_tiling_and_supported_layout_deviations() {
        let mut bad_count = fixture();
        put32(&mut bad_count, RAW_IFD + 2 + 10 * 12 + 4, 3);
        assert!(matches!(
            Dng::parse(&bad_count),
            Err(DecodeError::UnsupportedFeature)
        ));
        let mut strip = fixture();
        entry(&mut strip, RAW_IFD + 2 + 10 * 12, 273, 4, 1, DATA as u32);
        assert!(matches!(
            Dng::parse(&strip),
            Err(DecodeError::UnsupportedFeature)
        ));
        let mut cfa = fixture();
        entry(&mut cfa, RAW_IFD + 2 + 5 * 12, 262, 3, 1, 32_803);
        assert!(matches!(
            Dng::parse(&cfa),
            Err(DecodeError::UnsupportedFeature)
        ));
    }

    #[test]
    fn rejects_invalid_tiles_without_panicking() {
        let valid = fixture();
        for end in 0..=valid.len() {
            assert!(std::panic::catch_unwind(|| Dng::parse(&valid[..end])).is_ok());
        }
        let mut offset = valid;
        put32(&mut offset, 400, u32::MAX);
        assert!(matches!(Dng::parse(&offset), Err(DecodeError::Truncated)));
    }

    #[test]
    fn rejects_incompatible_sof_and_resource_limits() {
        for (offset, value) in [(DATA + 6, 11), (DATA + 10, 2), (DATA + 11, 2)] {
            let mut bytes = fixture();
            bytes[offset] = value;
            assert!(Dng::parse(&bytes).unwrap().decode().is_err());
        }
        assert!(matches!(
            Dng::parse_with_limits(
                &fixture(),
                DecodeLimits::default().with_max_value_bytes(8191)
            ),
            Err(DecodeError::ResourceLimit)
        ));
        assert!(matches!(
            Dng::parse_with_limits(
                &fixture(),
                DecodeLimits::default().with_max_decoded_samples(11)
            ),
            Err(DecodeError::ResourceLimit)
        ));
    }

    fn extra_tag(bytes: &mut [u8], tag: u16, kind: u16, count: u32, value: u32) {
        put16(bytes, RAW_IFD, 21);
        entry(bytes, RAW_IFD + 2 + 20 * 12, tag, kind, count, value);
    }

    fn ifd0_extra_tag(bytes: &mut [u8], tag: u16, kind: u16, count: u32, value: u32) {
        put16(bytes, IFD0, 7);
        entry(bytes, IFD0 + 2 + 6 * 12, tag, kind, count, value);
    }

    fn baseline_exposure(bytes: &mut [u8], offset: usize, denominator: u32) {
        put32(bytes, offset, 0);
        put32(bytes, offset + 4, denominator);
    }

    fn profile_tone_curve(bytes: &mut [u8], offset: usize, values: [f32; 4]) {
        for (index, value) in values.into_iter().enumerate() {
            put32(bytes, offset + index * 4, value.to_bits());
        }
    }

    fn add_preview_subifd(bytes: &mut [u8], tag: u16, kind: u16, count: u32, value: u32) {
        entry(bytes, IFD0 + 26, 330, 4, 2, 8_624);
        put32(bytes, 8_624, RAW_IFD as u32);
        put32(bytes, 8_628, 8_700);
        put16(bytes, 8_700, 1);
        entry(bytes, 8_702, tag, kind, count, value);
        put32(bytes, 8_714, 0);
    }

    #[test]
    fn rejects_malformed_processing_and_output_referred_features() {
        for (tag, kind, count, value, expected) in [
            (50_715, 7, 1, 0, DecodeError::InvalidTag),
            (50_716, 7, 1, 0, DecodeError::InvalidTag),
            (50_879, 3, 1, 1, DecodeError::UnsupportedFeature),
            (51_008, 7, 1, 0, DecodeError::Truncated),
            (51_009, 7, 1, 0, DecodeError::Truncated),
            (51_022, 7, 1, 0, DecodeError::Truncated),
        ] {
            let mut bytes = fixture();
            extra_tag(&mut bytes, tag, kind, count, value);
            assert_eq!(Dng::parse(&bytes).unwrap_err(), expected);
        }
        let mut repeated_black = fixture();
        extra_tag(&mut repeated_black, 50_713, 3, 2, 0x0002_0001);
        assert!(matches!(
            Dng::parse(&repeated_black),
            Err(DecodeError::InvalidTag)
        ));
        let mut malformed_profile_gain_table_map = fixture();
        extra_tag(&mut malformed_profile_gain_table_map, 52_525, 3, 1, 0);
        assert!(matches!(
            Dng::parse(&malformed_profile_gain_table_map),
            Err(DecodeError::UnsupportedFeature)
        ));
    }

    #[test]
    fn preserves_shared_colorimetric_reference_for_raw_subifd() {
        assert_eq!(
            Dng::parse(&fixture())
                .unwrap()
                .decode()
                .unwrap()
                .colorimetric_reference,
            None
        );
        let mut scene_referred = fixture();
        ifd0_extra_tag(&mut scene_referred, 50_879, 3, 1, 0);
        assert_eq!(
            Dng::parse(&scene_referred)
                .unwrap()
                .decode()
                .unwrap()
                .colorimetric_reference,
            Some(0)
        );
        let mut output_referred = fixture();
        ifd0_extra_tag(&mut output_referred, 50_879, 3, 1, 1);
        assert!(matches!(
            Dng::parse(&output_referred),
            Err(DecodeError::UnsupportedFeature)
        ));
        let mut wrong_scope = fixture();
        ifd0_extra_tag(&mut wrong_scope, 50_879, 3, 1, 0);
        extra_tag(&mut wrong_scope, 50_879, 3, 1, 0);
        assert!(matches!(
            Dng::parse(&wrong_scope),
            Err(DecodeError::UnsupportedFeature)
        ));
    }

    #[test]
    fn rejects_duplicate_scope_sensitive_tags() {
        for (tag, kind, count, value) in [
            (50_879, 3, 1, 0),
            (50_974, 4, 2, 8_624),
            (50_975, 4, 1, 1),
            (50_730, 10, 1, 8_632),
            (50_940, 11, 4, 8_640),
        ] {
            let mut bytes = fixture();
            entry(&mut bytes, RAW_IFD + 2, tag, kind, count, value);
            extra_tag(&mut bytes, tag, kind, count, value);
            match tag {
                50_974 => {
                    put32(&mut bytes, 8_624, 1);
                    put32(&mut bytes, 8_628, 1);
                }
                50_730 => baseline_exposure(&mut bytes, 8_632, 1),
                50_940 => profile_tone_curve(&mut bytes, 8_640, [0.0, 0.0, 1.0, 1.0]),
                _ => {}
            }
            assert!(matches!(Dng::parse(&bytes), Err(DecodeError::InvalidTag)));
        }
    }

    #[test]
    fn rejects_non_default_spatial_layouts() {
        assert!(Dng::parse(&fixture()).is_ok());
        for (tag, kind, count, value) in [(50_974, 4, 2, 8_624), (50_975, 4, 1, 3)] {
            let mut bytes = fixture();
            extra_tag(&mut bytes, tag, kind, count, value);
            if tag == 50_974 {
                put32(&mut bytes, 8_624, 2);
                put32(&mut bytes, 8_628, 1);
            }
            assert!(matches!(
                Dng::parse(&bytes),
                Err(DecodeError::UnsupportedFeature)
            ));
        }
        let mut defaults = fixture();
        extra_tag(&mut defaults, 50_974, 4, 2, 8_624);
        put32(&mut defaults, 8_624, 1);
        put32(&mut defaults, 8_628, 1);
        assert!(Dng::parse(&defaults).is_ok());
        let mut default_row_interleave = fixture();
        extra_tag(&mut default_row_interleave, 50_975, 4, 1, 1);
        assert!(Dng::parse(&default_row_interleave).is_ok());
    }

    #[test]
    fn reports_profile_gain_table_map_only_when_present() {
        assert_eq!(
            Dng::parse(&fixture())
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .profile_gain_table_map,
            LinearRawProcessing::NotPresent
        );
        let mut bytes = fixture();
        extra_tag(&mut bytes, 52_525, 7, 1, 0);
        assert_eq!(
            Dng::parse(&bytes)
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .profile_gain_table_map,
            LinearRawProcessing::Unapplied
        );
        assert_eq!(
            Dng::parse(&bytes)
                .unwrap()
                .decode()
                .unwrap()
                .profile_gain_table_map,
            Some(vec![0])
        );
    }

    #[test]
    fn preserves_large_profile_gain_table_with_a_separate_bound() {
        let mut bytes = fixture();
        let offset = bytes.len();
        let payload = vec![0x5a; 3_158_080];
        bytes.extend_from_slice(&payload);
        extra_tag(&mut bytes, 52_525, 7, payload.len() as u32, offset as u32);
        let image = Dng::parse(&bytes).unwrap().decode().unwrap();
        assert_eq!(image.profile_gain_table_map.as_ref(), Some(&payload));
        assert_eq!(
            image.processing.profile_gain_table_map,
            LinearRawProcessing::Unapplied
        );
        assert_eq!(
            image.samples,
            Dng::parse(&fixture()).unwrap().decode().unwrap().samples
        );
        assert!(matches!(
            Dng::parse_with_limits(
                &bytes,
                DecodeLimits::default().with_max_profile_gain_table_bytes(payload.len() - 1)
            ),
            Err(DecodeError::ResourceLimit)
        ));
        bytes.pop();
        assert!(matches!(Dng::parse(&bytes), Err(DecodeError::Truncated)));
    }

    #[test]
    fn does_not_mistake_tag_52509_for_a_profile_gain_table() {
        let mut bytes = fixture();
        extra_tag(&mut bytes, 52_509, 7, 1, 0);
        let image = Dng::parse(&bytes).unwrap().decode().unwrap();
        assert_eq!(
            image.processing.profile_gain_table_map,
            LinearRawProcessing::NotPresent
        );
        assert_eq!(image.profile_gain_table_map, None);
    }

    #[test]
    fn tracks_ifd0_preview_baseline_exposure_and_profile_tone_curve() {
        let mut baseline = fixture();
        ifd0_extra_tag(&mut baseline, 50_730, 10, 1, 8_800);
        baseline_exposure(&mut baseline, 8_800, 1);
        assert_eq!(
            Dng::parse(&baseline)
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .baseline_exposure,
            LinearRawProcessing::Unapplied
        );
        let mut tone_curve = fixture();
        ifd0_extra_tag(&mut tone_curve, 50_940, 11, 4, 8_800);
        profile_tone_curve(&mut tone_curve, 8_800, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            Dng::parse(&tone_curve)
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .profile_tone_curve,
            LinearRawProcessing::Unapplied
        );
    }

    #[test]
    fn ignores_deferred_processing_tags_from_the_selected_raw_ifd() {
        let mut baseline = fixture();
        extra_tag(&mut baseline, 50_730, 10, 1, 8_800);
        baseline_exposure(&mut baseline, 8_800, 1);
        assert_eq!(
            Dng::parse(&baseline)
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .baseline_exposure,
            LinearRawProcessing::NotPresent
        );
        let mut tone_curve = fixture();
        extra_tag(&mut tone_curve, 50_940, 11, 4, 8_800);
        profile_tone_curve(&mut tone_curve, 8_800, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            Dng::parse(&tone_curve)
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .profile_tone_curve,
            LinearRawProcessing::NotPresent
        );
    }

    #[test]
    fn ignores_deferred_processing_tags_from_separate_preview_subifds() {
        let mut baseline = fixture();
        add_preview_subifd(&mut baseline, 50_730, 10, 1, 8_800);
        baseline_exposure(&mut baseline, 8_800, 1);
        assert_eq!(
            Dng::parse(&baseline)
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .baseline_exposure,
            LinearRawProcessing::NotPresent
        );
        let mut tone_curve = fixture();
        add_preview_subifd(&mut tone_curve, 50_940, 11, 4, 8_800);
        profile_tone_curve(&mut tone_curve, 8_800, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            Dng::parse(&tone_curve)
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .profile_tone_curve,
            LinearRawProcessing::NotPresent
        );
    }

    #[test]
    fn rejects_malformed_deferred_processing_tags() {
        let mut baseline = fixture();
        ifd0_extra_tag(&mut baseline, 50_730, 5, 1, 8_800);
        assert!(matches!(
            Dng::parse(&baseline),
            Err(DecodeError::InvalidTag)
        ));
        let mut zero_denominator = fixture();
        extra_tag(&mut zero_denominator, 50_730, 10, 1, 8_800);
        baseline_exposure(&mut zero_denominator, 8_800, 0);
        assert!(matches!(
            Dng::parse(&zero_denominator),
            Err(DecodeError::InvalidTag)
        ));
        let mut tone_curve = fixture();
        ifd0_extra_tag(&mut tone_curve, 50_940, 11, 3, 8_800);
        assert!(matches!(
            Dng::parse(&tone_curve),
            Err(DecodeError::InvalidTag)
        ));
        let mut non_finite = fixture();
        extra_tag(&mut non_finite, 50_940, 11, 4, 8_800);
        profile_tone_curve(&mut non_finite, 8_800, [0.0, f32::NAN, 1.0, 1.0]);
        assert!(matches!(
            Dng::parse(&non_finite),
            Err(DecodeError::InvalidTag)
        ));
    }

    #[test]
    fn ignores_preview_layout_metadata() {
        let mut bytes = fixture();
        ifd0_extra_tag(&mut bytes, 50_974, 4, 2, 8_624);
        put32(&mut bytes, 8_624, 2);
        put32(&mut bytes, 8_628, 1);
        assert!(Dng::parse(&bytes).is_ok());
    }

    #[test]
    fn preserves_ifd0_orientation_and_tracks_semantic_mask_ifds() {
        let mut bytes = fixture();
        entry(&mut bytes, IFD0 + 62, 274, 3, 1, 3);
        assert_eq!(
            Dng::parse(&bytes).unwrap().decode().unwrap().orientation,
            Some(3)
        );
        put32(&mut bytes, 82, 8_700);
        put16(&mut bytes, 8_700, 1);
        entry(&mut bytes, 8_702, 254, 4, 1, 65_540);
        assert_eq!(
            Dng::parse(&bytes)
                .unwrap()
                .decode()
                .unwrap()
                .processing
                .semantic_masks,
            LinearRawProcessing::Unapplied
        );
    }

    #[test]
    fn rejects_overlapping_tile_ranges() {
        let mut bytes = fixture();
        put32(&mut bytes, 404, DATA as u32);
        assert!(matches!(
            Dng::parse(&bytes),
            Err(DecodeError::InvalidOffset)
        ));
    }

    #[test]
    fn rejects_invalid_active_area_and_default_crop_geometry() {
        let mut active_area = fixture();
        put32(&mut active_area, 368, 3);
        assert!(matches!(
            Dng::parse(&active_area),
            Err(DecodeError::InvalidGeometry)
        ));
        let mut default_crop = fixture();
        put32(&mut default_crop, 384, 3);
        assert!(matches!(
            Dng::parse(&default_crop),
            Err(DecodeError::InvalidGeometry)
        ));
    }

    #[test]
    fn rejects_profiles_outside_the_supported_apple_contract() {
        let mut future_version = fixture();
        entry(&mut future_version, IFD0 + 2, 50_706, 1, 4, 0x0000_0801);
        assert!(matches!(
            Dng::parse(&future_version),
            Err(DecodeError::UnsupportedFeature)
        ));
        let mut incompatible_backward_version = fixture();
        entry(
            &mut incompatible_backward_version,
            IFD0 + 14,
            50_707,
            1,
            4,
            0x0000_0801,
        );
        assert!(matches!(
            Dng::parse(&incompatible_backward_version),
            Err(DecodeError::UnsupportedFeature)
        ));
        let mut other_make = fixture();
        other_make[392..398].copy_from_slice(b"Nikon\0");
        assert!(matches!(
            Dng::parse(&other_make),
            Err(DecodeError::UnsupportedFeature)
        ));
        let mut wide_black_level = fixture();
        entry(
            &mut wide_black_level,
            RAW_IFD + 2 + 14 * 12,
            50_714,
            4,
            3,
            8_800,
        );
        assert!(Dng::parse(&wide_black_level).is_ok());
    }
}
