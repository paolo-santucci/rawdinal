#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "rawdinal.h"

_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->version) == sizeof(uint32_t), "version uses uint32_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->width) == sizeof(uint32_t), "width uses uint32_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->stride_samples) == sizeof(size_t), "stride uses size_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->samples) == sizeof(const float *), "samples uses float pointer");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->component_ids) == sizeof(const uint8_t *), "components use byte pointer");
_Static_assert(offsetof(rawdinal_raw_v1_info, version) == 0, "version leads descriptor");
_Static_assert(offsetof(rawdinal_raw_v1_info, reserved) == sizeof(uint32_t), "reserved follows version");
_Static_assert(offsetof(rawdinal_raw_v1_info, width) == 2 * sizeof(uint32_t), "dimensions follow header");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->dng_version) == 4, "DNG version has four bytes");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->dng_backward_version) == 4, "backward version has four bytes");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->tiff_byte_order) == sizeof(uint32_t), "byte order uses uint32_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->make) == sizeof(const uint8_t *), "make uses byte pointer");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->model) == sizeof(const uint8_t *), "model uses byte pointer");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->profile_gain_table_map) == sizeof(const uint8_t *), "gain map uses byte pointer");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->white_balance) == sizeof(uint32_t), "WB state uses uint32_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->color_conversion) == sizeof(uint32_t), "color state uses uint32_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->default_crop) == sizeof(uint32_t), "crop state uses uint32_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->orientation_processing) == sizeof(uint32_t), "orientation state uses uint32_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->baseline_exposure) == sizeof(uint32_t), "baseline state uses uint32_t");
_Static_assert(sizeof(((rawdinal_raw_v1_info *)0)->profile_tone_curve) == sizeof(uint32_t), "tone state uses uint32_t");
_Static_assert(offsetof(rawdinal_raw_v1_info, samples) < offsetof(rawdinal_raw_v1_info, component_ids), "sample order is stable");
_Static_assert(offsetof(rawdinal_raw_v1_info, component_ids) < offsetof(rawdinal_raw_v1_info, dng_version), "metadata order is stable");
_Static_assert(offsetof(rawdinal_raw_v1_info, dng_version) < offsetof(rawdinal_raw_v1_info, tiff_byte_order), "byte order follows versions");
_Static_assert(offsetof(rawdinal_raw_v1_info, white_normalization) < offsetof(rawdinal_raw_v1_info, white_balance), "operation order is stable");
_Static_assert(offsetof(rawdinal_raw_v1_info, profile_tone_curve) < offsetof(rawdinal_raw_v1_info, demosaic), "operation order is stable");
_Static_assert(offsetof(rawdinal_raw_v1_info, semantic_masks) < offsetof(rawdinal_raw_v1_info, profile_gain_table_map), "gain map follows states");
_Static_assert(offsetof(rawdinal_raw_v1_info, profile_gain_table_map) < offsetof(rawdinal_raw_v1_info, scene_linear), "classification follows gain map");
_Static_assert(sizeof(rawdinal_raw_v1_probe_info) == 8 * sizeof(uint32_t), "probe descriptor is fixed uint32 layout");
_Static_assert(offsetof(rawdinal_raw_v1_probe_info, version) == 0, "probe version leads");
_Static_assert(offsetof(rawdinal_raw_v1_probe_info, classification) == 2 * sizeof(uint32_t), "probe class follows header");
_Static_assert(sizeof(rawdinal_raw_v1_capabilities) == 4 * sizeof(uint32_t), "capabilities descriptor is fixed uint32 layout");
_Static_assert(offsetof(rawdinal_raw_v1_capabilities, codec_bits) == 2 * sizeof(uint32_t), "codec bits follow header");

#define CHECK(expression) do { if (!(expression)) return 1; } while (0)

static uint64_t checksum(const float *samples, size_t count)
{
  uint64_t value = UINT64_C(14695981039346656037);
  size_t index;
  for (index = 0; index < count; ++index) {
    uint32_t bits;
    memcpy(&bits, samples + index, sizeof(bits));
    value ^= bits;
    value *= UINT64_C(1099511628211);
  }
  return value;
}

int main(void)
{
  rawdinal_raw_v1_image *image = (rawdinal_raw_v1_image *)(void *)(uintptr_t)1;
  rawdinal_raw_v1_info info = {0};
  rawdinal_raw_v1_probe_info probe = {0};
  rawdinal_raw_v1_capabilities capabilities = {0};
  char error[8] = {'x', 'x', 'x', 'x', 'x', 'x', 'x', 'x'};
  const char *path = getenv("PRORAW_SAMPLE");

  CHECK(rawdinal_raw_v1_decode(NULL, 0, &image, error, sizeof(error)) == RAWDINAL_STATUS_INVALID_INPUT);
  CHECK(image == NULL && error[sizeof(error) - 1] == '\0');
  info.version = 9;
  info.samples = (const float *)(const void *)(uintptr_t)1;
  CHECK(rawdinal_raw_v1_get_info(NULL, &info) == RAWDINAL_STATUS_INVALID_INPUT);
  CHECK(info.version == 0 && info.samples == NULL);
  CHECK(rawdinal_raw_v1_probe((const uint8_t *)"random", 6, &probe, error, sizeof(error)) == RAWDINAL_STATUS_OK);
  CHECK(probe.version == RAWDINAL_RAW_V1_PROBE_INFO_VERSION && probe.classification == RAWDINAL_RAW_V1_PROBE_NOT_RECOGNIZED);
  CHECK(rawdinal_raw_v1_decode((const uint8_t *)"random", 6, &image, error, sizeof(error)) == RAWDINAL_STATUS_NOT_RECOGNIZED);
  CHECK(image == NULL);
  CHECK(rawdinal_raw_v1_probe((const uint8_t *)"FOVb", 4, &probe, error, sizeof(error)) == RAWDINAL_STATUS_OK);
  CHECK(probe.classification == RAWDINAL_RAW_V1_PROBE_RECOGNIZED_UNSUPPORTED && probe.container == RAWDINAL_RAW_V1_CONTAINER_X3F);
  CHECK(rawdinal_raw_v1_probe((const uint8_t *)"II+\0", 4, &probe, error, sizeof(error)) == RAWDINAL_STATUS_OK);
  CHECK(probe.classification == RAWDINAL_RAW_V1_PROBE_NOT_RECOGNIZED);
  CHECK(rawdinal_raw_v1_probe((const uint8_t *)"II*\0\xff\xff\xff\xff", 8, &probe, error, sizeof(error)) == RAWDINAL_STATUS_OK);
  CHECK(probe.classification == RAWDINAL_RAW_V1_PROBE_NOT_RECOGNIZED);
  CHECK(rawdinal_raw_v1_decode((const uint8_t *)"II*\0\xff\xff\xff\xff", 8, &image, error, sizeof(error)) == RAWDINAL_STATUS_NOT_RECOGNIZED);
  CHECK(image == NULL);
  CHECK(rawdinal_raw_v1_probe((const uint8_t *)"II)\0", 4, &probe, error, sizeof(error)) == RAWDINAL_STATUS_INVALID_INPUT);
  CHECK(probe.version == 0 && error[sizeof(error) - 1] == '\0');
  CHECK(rawdinal_raw_v1_get_capabilities(&capabilities) == RAWDINAL_STATUS_OK);
  CHECK(capabilities.version == RAWDINAL_RAW_V1_CAPABILITIES_VERSION);
  CHECK(capabilities.codec_bits == RAWDINAL_RAW_V1_CODEC_BIT_LOSSLESS_JPEG);
  rawdinal_raw_v1_free(NULL);

  if (path != NULL) {
    FILE *file = fopen(path, "rb");
    long length;
    uint8_t *input;
    uint64_t value;
    CHECK(file != NULL);
    CHECK(fseek(file, 0, SEEK_END) == 0);
    length = ftell(file);
    CHECK(length > 0 && fseek(file, 0, SEEK_SET) == 0);
    input = malloc((size_t)length);
    CHECK(input != NULL && fread(input, 1, (size_t)length, file) == (size_t)length);
    fclose(file);
    image = NULL;
    CHECK(rawdinal_raw_v1_probe(input, (size_t)length, &probe, error, sizeof(error)) == RAWDINAL_STATUS_OK);
    CHECK(probe.classification == RAWDINAL_RAW_V1_PROBE_SUPPORTED && probe.container == RAWDINAL_RAW_V1_CONTAINER_DNG);
    CHECK(probe.codec == RAWDINAL_RAW_V1_CODEC_LOSSLESS_JPEG && probe.width == 4032 && probe.height == 3024 && probe.channels == 3);
    CHECK(rawdinal_raw_v1_decode(input, (size_t)length, &image, error, sizeof(error)) == RAWDINAL_STATUS_OK);
    free(input);
    CHECK(rawdinal_raw_v1_get_info(image, &info) == RAWDINAL_STATUS_OK);
    CHECK(info.version == RAWDINAL_RAW_V1_INFO_VERSION);
    CHECK(info.width == 4032 && info.height == 3024 && info.channels == 3);
    CHECK(info.stride_samples == 12096 && info.sample_count == 36578304 && info.samples != NULL);
    CHECK(info.component_ids != NULL && info.component_id_count == 3);
    CHECK(info.tiff_byte_order == RAWDINAL_RAW_V1_TIFF_BYTE_ORDER_LITTLE_ENDIAN || info.tiff_byte_order == RAWDINAL_RAW_V1_TIFF_BYTE_ORDER_BIG_ENDIAN);
    CHECK(info.linearization == RAWDINAL_RAW_V1_PROCESSING_APPLIED);
    CHECK(info.black_subtraction == RAWDINAL_RAW_V1_PROCESSING_APPLIED);
    CHECK(info.white_normalization == RAWDINAL_RAW_V1_PROCESSING_APPLIED);
    CHECK(info.white_balance == RAWDINAL_RAW_V1_PROCESSING_UNAPPLIED);
    CHECK(info.color_conversion == RAWDINAL_RAW_V1_PROCESSING_UNAPPLIED);
    CHECK(info.default_crop == RAWDINAL_RAW_V1_PROCESSING_UNAPPLIED);
    CHECK(info.orientation_processing == RAWDINAL_RAW_V1_PROCESSING_NOT_PRESENT);
    CHECK(info.baseline_exposure == RAWDINAL_RAW_V1_PROCESSING_UNAPPLIED);
    CHECK(info.profile_tone_curve == RAWDINAL_RAW_V1_PROCESSING_NOT_PRESENT);
    CHECK(info.demosaic == RAWDINAL_RAW_V1_PROCESSING_NOT_PRESENT);
    CHECK(info.opcode_list_1 == RAWDINAL_RAW_V1_PROCESSING_NOT_PRESENT);
    CHECK(info.opcode_list_2 == RAWDINAL_RAW_V1_PROCESSING_NOT_PRESENT);
    CHECK(info.opcode_list_3 == RAWDINAL_RAW_V1_PROCESSING_NOT_PRESENT);
    CHECK(info.profile_gain_table_map_processing == RAWDINAL_RAW_V1_PROCESSING_UNAPPLIED);
    CHECK(info.semantic_masks == RAWDINAL_RAW_V1_PROCESSING_NOT_PRESENT);
    CHECK(info.scene_linear == RAWDINAL_RAW_V1_TRUE && info.camera_native == RAWDINAL_RAW_V1_TRUE);
    CHECK(info.already_demosaiced == RAWDINAL_RAW_V1_TRUE);
    value = checksum(info.samples, info.sample_count);
    CHECK(value == UINT64_C(0xdefdc0442475559c));
    rawdinal_raw_v1_free(image);
  }
  return 0;
}
