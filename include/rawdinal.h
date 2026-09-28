/* SPDX-License-Identifier: GPL-3.0-only */
/* Copyright (C) 2026 Paolo SANTUCCI */

#ifndef RAWDINAL_H
#define RAWDINAL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rawdinal_image rawdinal_image;
typedef struct rawdinal_sensor_v1_image rawdinal_sensor_v1_image;
typedef struct rawdinal_raw_v1_image rawdinal_raw_v1_image;

#define RAWDINAL_STATUS_OK INT32_C(0)
#define RAWDINAL_STATUS_ERROR INT32_C(1)
#define RAWDINAL_STATUS_PANIC INT32_C(2)
#define RAWDINAL_STATUS_UNSUPPORTED INT32_C(3)
#define RAWDINAL_STATUS_INVALID_INPUT INT32_C(4)
#define RAWDINAL_STATUS_RESOURCE_LIMIT INT32_C(5)
#define RAWDINAL_STATUS_ALLOCATION INT32_C(6)
#define RAWDINAL_STATUS_NOT_RECOGNIZED INT32_C(7)

#define RAWDINAL_RAW_V1_INFO_VERSION UINT32_C(1)
#define RAWDINAL_RAW_V1_PROBE_INFO_VERSION UINT32_C(1)
#define RAWDINAL_RAW_V1_CAPABILITIES_VERSION UINT32_C(1)
#define RAWDINAL_CLIPPING_V1_INFO_VERSION UINT32_C(1)
#define RAWDINAL_CLIPPING_V1_THRESHOLD_UNKNOWN UINT32_C(0)
#define RAWDINAL_CLIPPING_V1_THRESHOLD_ESTIMATED_ENCODED_MAXIMUM UINT32_C(1)
#define RAWDINAL_CLIPPING_V1_THRESHOLD_CALIBRATED UINT32_C(2)
#define RAWDINAL_RAW_V1_PROCESSING_NOT_PRESENT UINT32_C(0)
#define RAWDINAL_RAW_V1_PROCESSING_APPLIED UINT32_C(1)
#define RAWDINAL_RAW_V1_PROCESSING_UNAPPLIED UINT32_C(2)
#define RAWDINAL_RAW_V1_PROCESSING_UNKNOWN UINT32_C(3)
#define RAWDINAL_RAW_V1_PROCESSING_SKIPPED_OPTIONAL UINT32_C(4)
#define RAWDINAL_RAW_V1_PROBE_NOT_RECOGNIZED UINT32_C(0)
#define RAWDINAL_RAW_V1_PROBE_SUPPORTED UINT32_C(1)
#define RAWDINAL_RAW_V1_PROBE_RECOGNIZED_UNSUPPORTED UINT32_C(2)
#define RAWDINAL_RAW_V1_CONTAINER_UNKNOWN UINT32_C(0)
#define RAWDINAL_RAW_V1_CONTAINER_DNG UINT32_C(1)
#define RAWDINAL_RAW_V1_CONTAINER_X3F UINT32_C(2)
#define RAWDINAL_RAW_V1_CODEC_UNKNOWN UINT32_C(0)
#define RAWDINAL_RAW_V1_CODEC_LOSSLESS_JPEG UINT32_C(1)
#define RAWDINAL_RAW_V1_CODEC_JPEG_XL UINT32_C(2)
#define RAWDINAL_RAW_V1_CODEC_BIT_LOSSLESS_JPEG UINT32_C(1)
#define RAWDINAL_RAW_V1_CODEC_BIT_JPEG_XL UINT32_C(2)
#define RAWDINAL_RAW_V1_TIFF_BYTE_ORDER_UNKNOWN UINT32_C(0)
#define RAWDINAL_RAW_V1_TIFF_BYTE_ORDER_LITTLE_ENDIAN UINT32_C(1)
#define RAWDINAL_RAW_V1_TIFF_BYTE_ORDER_BIG_ENDIAN UINT32_C(2)
#define RAWDINAL_RAW_V1_COLORIMETRIC_REFERENCE_SCENE_REFERRED UINT32_C(0)
#define RAWDINAL_RAW_V1_COLORIMETRIC_REFERENCE_OUTPUT_REFERRED UINT32_C(1)
#define RAWDINAL_RAW_V1_FALSE UINT32_C(0)
#define RAWDINAL_RAW_V1_TRUE UINT32_C(1)

typedef struct rawdinal_raw_v1_info
{
  uint32_t version;
  uint32_t reserved;
  uint32_t width;
  uint32_t height;
  uint32_t channels;
  size_t stride_samples;
  size_t sample_count;
  const float *samples;
  const uint8_t *component_ids;
  size_t component_id_count;
  uint8_t dng_version[4];
  uint8_t dng_backward_version[4];
  uint32_t dng_backward_version_present;
  uint32_t tiff_byte_order;
  const uint8_t *make;
  size_t make_length;
  const uint8_t *model;
  size_t model_length;
  uint32_t active_area_top;
  uint32_t active_area_left;
  uint32_t active_area_bottom;
  uint32_t active_area_right;
  uint32_t orientation;
  uint32_t orientation_present;
  uint32_t default_crop_origin_x;
  uint32_t default_crop_origin_y;
  uint32_t default_crop_origin_present;
  uint32_t default_crop_size_width;
  uint32_t default_crop_size_height;
  uint32_t default_crop_size_present;
  uint32_t linearization;
  uint32_t black_subtraction;
  uint32_t white_normalization;
  uint32_t white_balance;
  uint32_t color_conversion;
  uint32_t default_crop;
  uint32_t orientation_processing;
  uint32_t baseline_exposure;
  uint32_t profile_tone_curve;
  uint32_t demosaic;
  uint32_t opcode_list_1;
  uint32_t opcode_list_2;
  uint32_t opcode_list_3;
  uint32_t profile_gain_table_map_processing;
  uint32_t semantic_masks;
  const uint8_t *profile_gain_table_map;
  size_t profile_gain_table_map_length;
  uint32_t scene_linear;
  uint32_t camera_native;
  uint32_t already_demosaiced;
  uint32_t colorimetric_reference;
  uint32_t colorimetric_reference_present;
} rawdinal_raw_v1_info;

typedef struct rawdinal_raw_v1_probe_info
{
  uint32_t version;
  uint32_t reserved;
  uint32_t classification;
  uint32_t container;
  uint32_t codec;
  uint32_t width;
  uint32_t height;
  uint32_t channels;
} rawdinal_raw_v1_probe_info;

typedef struct rawdinal_raw_v1_capabilities
{
  uint32_t version;
  uint32_t reserved;
  uint32_t codec_bits;
  uint32_t reserved2;
} rawdinal_raw_v1_capabilities;

typedef uint32_t rawdinal_sensor_v1_plane_id;

#define RAWDINAL_SENSOR_V1_BOTTOM UINT32_C(0)
#define RAWDINAL_SENSOR_V1_MIDDLE UINT32_C(1)
#define RAWDINAL_SENSOR_V1_TOP UINT32_C(2)

typedef struct rawdinal_sensor_v1_plane
{
  rawdinal_sensor_v1_plane_id identity;
  uint32_t reserved;
  size_t width;
  size_t height;
  size_t stride_samples;
  size_t sample_count;
  const uint16_t *samples;
} rawdinal_sensor_v1_plane;

typedef struct rawdinal_info
{
  uint32_t width;
  uint32_t height;
  const uint8_t *exif;
  size_t exif_size;
} rawdinal_info;

typedef struct rawdinal_clipping_v1_plane
{
  uint32_t identity;
  uint32_t threshold_code;
  uint32_t threshold_provenance;
  uint32_t reserved;
  size_t width;
  size_t height;
  size_t stride_bytes;
  size_t byte_count;
  const uint8_t *data;
} rawdinal_clipping_v1_plane;

typedef struct rawdinal_clipping_v1_info
{
  uint32_t version;
  uint32_t reserved;
  size_t width;
  size_t height;
  size_t stride_bytes;
  size_t byte_count;
  const uint8_t *data;
  rawdinal_clipping_v1_plane planes[3];
} rawdinal_clipping_v1_info;

/** Borrow the camera JPEG for metadata or thumbnails. The returned storage belongs
 * to the input mapping and expires with it. Returns zero on success. */
int32_t rawdinal_preview(const uint8_t *data, size_t length, const uint8_t **preview, size_t *preview_length);

/** Decode to experimental linear sRGB. Returns zero on success. Input storage is
 * borrowed only during the call. The caller releases *output with rawdinal_free.
 * All output pointers must refer to writable, nonoverlapping storage. The EXIF
 * pointer in info is borrowed from the handle and expires when it is released. */
int32_t rawdinal_decode(const uint8_t *data, size_t length, rawdinal_image **output,
                       rawdinal_info *info, char *error, size_t error_capacity);

/** Decode to experimental linear sRGB and retain version-1 clipping provenance. Ownership,
 * argument, error, and release rules are identical to rawdinal_decode. */
int32_t rawdinal_decode_with_clipping_v1(const uint8_t *data, size_t length,
                                        rawdinal_image **output, rawdinal_info *info,
                                        char *error, size_t error_capacity);

/** Copy to width*height*4 caller-owned floats without clipping. Alpha is one.
 * The image handle must remain live and the destination must not overlap it. */
int32_t rawdinal_copy_rgba(const rawdinal_image *image, float *destination, size_t float_count);

/** Borrow version-1 estimated encoded-maximum clipping provenance from a handle returned by
 * rawdinal_decode_with_clipping_v1. The combined output mask has
 * one byte per output pixel and native planes are bottom, middle, top. A nonzero byte means that
 * contributing source support met its layer's estimated encoded maximum; it does not indicate
 * calibrated physical saturation. All pointers expire with rawdinal_free. Output must be writable
 * and disjoint from the handle and its owned storage, is cleared on failure, and the handle must
 * not be accessed or released concurrently. */
int32_t rawdinal_get_clipping_v1(const rawdinal_image *image,
                                 rawdinal_clipping_v1_info *output);

/** Release exactly once after all reads. Null is accepted. */
void rawdinal_free(rawdinal_image *image);

/** Decode the narrow observed DP2 Merrill, dp3 Quattro and sd Quattro sensor layouts
 * without calibration, reconstruction, white balance or color conversion. Samples are
 * uncalibrated. Input storage is borrowed only during the call. Output must be writable,
 * disjoint from all other arguments and must not contain an unreleased handle. It is cleared
 * on failure. Error may be null; otherwise it must be writable and disjoint. Returns
 * RAWDINAL_STATUS_OK, RAWDINAL_STATUS_ERROR or RAWDINAL_STATUS_PANIC. The caller releases a
 * successful output with rawdinal_sensor_v1_free. */
int32_t rawdinal_sensor_v1_decode(const uint8_t *data, size_t length,
                                 rawdinal_sensor_v1_image **output,
                                 char *error, size_t error_capacity);

/** Borrow one decoded uint16 plane from the handle. Rows contain width samples and
 * stride_samples is expressed in uint16 samples. Dimensions preserve the full encoded
 * layout and can exceed nominal image dimensions. The pointer expires when the handle is
 * released. Output must be writable and disjoint from the handle and its owned storage; it is
 * cleared on failure. The handle must not be released or accessed concurrently. */
int32_t rawdinal_sensor_v1_get_plane(const rawdinal_sensor_v1_image *image,
                                    rawdinal_sensor_v1_plane_id identity,
                                    rawdinal_sensor_v1_plane *output);

/** Borrow decompressed, uninterpreted CAMF bytes from the handle. The pointer
 * expires when the handle is released. Data and length must be writable, disjoint from each
 * other and handle storage; both are cleared on failure. The handle must not be released or
 * accessed concurrently. */
int32_t rawdinal_sensor_v1_get_camf(const rawdinal_sensor_v1_image *image,
                                   const uint8_t **data, size_t *length);

/** Borrow TIFF/EXIF bytes copied from the JPEG preview. Empty output is valid
 * when no readable EXIF block exists. The pointer expires with the handle. Data and length
 * must be writable, disjoint from each other and handle storage; both are cleared on failure.
 * The handle must not be released or accessed concurrently. */
int32_t rawdinal_sensor_v1_get_exif(const rawdinal_sensor_v1_image *image,
                                   const uint8_t **data, size_t *length);

/** Release exactly once after all sensor, CAMF and EXIF reads. Null is accepted. */
void rawdinal_sensor_v1_free(rawdinal_sensor_v1_image *image);

/** Decode the supported Apple ProRAW DNG tiled linear-raw layout. Input is borrowed only during
 * this call. Output must be writable, must not contain a live handle, and must be disjoint from
 * input and error. Error may be null; otherwise its error_capacity bytes must be writable and
 * disjoint from input and output. On success, output owns decoded samples and metadata and must
 * be released with rawdinal_raw_v1_free. Output is cleared and error is NUL-terminated on
 * failure. Unsupported inputs return RAWDINAL_STATUS_UNSUPPORTED; malformed inputs, limits, and
 * allocation failures return their respective detailed status. Arbitrary or non-DNG input returns
 * RAWDINAL_STATUS_NOT_RECOGNIZED; recognized X3F and valid unsupported DNG return
 * RAWDINAL_STATUS_UNSUPPORTED. */
int32_t rawdinal_raw_v1_decode(const uint8_t *data, size_t length,
                               rawdinal_raw_v1_image **output,
                               char *error, size_t error_capacity);

/** Borrow a fixed version-1 descriptor from a live decoded handle. Width, height, ActiveArea,
 * and default crop values are pixels; ActiveArea is half-open stored coordinates and an absent
 * ActiveArea expands to the stored image. Crop origin is relative to ActiveArea. Orientation is
 * the DNG value 1 through 8. Stride and sample count are float samples in chunky, interleaved
 * component_ids and calibration-channel order. Make, model, component IDs, samples, and
 * ProfileGainTableMap are byte or float arrays and are not NUL-terminated unless their lengths
 * include such a byte. tiff_byte_order interprets ProfileGainTableMap bytes. Presence fields make
 * optional values meaningful; absent colorimetric_reference defaults to SCENE_REFERRED.
 * Processing values use RAWDINAL_RAW_V1_PROCESSING_* and classification fields use
 * RAWDINAL_RAW_V1_TRUE/FALSE. Raw-v1 does not retain the full DNG metadata graph: callers retain
 * and reparse their original DNG bytes for WB, calibration, EXIF, and other metadata according to
 * processing flags. All borrowed pointers expire with the handle. The handle must not be accessed
 * or released concurrently, and output must be writable and disjoint from handle-owned storage.
 * Output is zeroed on failure. */
int32_t rawdinal_raw_v1_get_info(const rawdinal_raw_v1_image *image,
                                 rawdinal_raw_v1_info *output);

/** Probe bounded input without decoding samples. Output must be writable and disjoint from input
 * and error. Error may be null; otherwise it must be writable and disjoint. A successful call
 * returns a RAWDINAL_RAW_V1_PROBE_* classification; malformed input and resource failures clear
 * output and return their detailed statuses. */
int32_t rawdinal_raw_v1_probe(const uint8_t *data, size_t length,
                              rawdinal_raw_v1_probe_info *output,
                              char *error, size_t error_capacity);

/** Write immutable raw-v1 decoder capabilities. Output must be writable and is zeroed on failure. */
int32_t rawdinal_raw_v1_get_capabilities(rawdinal_raw_v1_capabilities *output);

/** Release a raw-v1 handle exactly once after all descriptor reads. Null is accepted. */
void rawdinal_raw_v1_free(rawdinal_raw_v1_image *image);

#ifdef __cplusplus
}
#endif

#endif
