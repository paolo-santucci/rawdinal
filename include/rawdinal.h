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

#ifdef __cplusplus
}
#endif

#endif
