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

#define RAWDINAL_STATUS_OK INT32_C(0)
#define RAWDINAL_STATUS_ERROR INT32_C(1)
#define RAWDINAL_STATUS_PANIC INT32_C(2)

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

/** Borrow the camera JPEG for metadata or thumbnails. The returned storage belongs
 * to the input mapping and expires with it. Returns zero on success. */
int32_t rawdinal_preview(const uint8_t *data, size_t length, const uint8_t **preview, size_t *preview_length);

/** Decode to experimental linear sRGB. Returns zero on success. Input storage is
 * borrowed only during the call. The caller releases *output with rawdinal_free.
 * All output pointers must refer to writable, nonoverlapping storage. The EXIF
 * pointer in info is borrowed from the handle and expires when it is released. */
int32_t rawdinal_decode(const uint8_t *data, size_t length, rawdinal_image **output,
                       rawdinal_info *info, char *error, size_t error_capacity);

/** Copy to width*height*4 caller-owned floats without clipping. Alpha is one.
 * The image handle must remain live and the destination must not overlap it. */
int32_t rawdinal_copy_rgba(const rawdinal_image *image, float *destination, size_t float_count);

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

#ifdef __cplusplus
}
#endif

#endif
