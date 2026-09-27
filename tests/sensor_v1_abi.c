#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "rawdinal.h"

_Static_assert(sizeof(((rawdinal_sensor_v1_plane *)0)->identity) == sizeof(uint32_t), "identity uses uint32_t");
_Static_assert(sizeof(((rawdinal_sensor_v1_plane *)0)->reserved) == sizeof(uint32_t), "reserved uses uint32_t");
_Static_assert(sizeof(((rawdinal_sensor_v1_plane *)0)->width) == sizeof(size_t), "width uses size_t");
_Static_assert(sizeof(((rawdinal_sensor_v1_plane *)0)->height) == sizeof(size_t), "height uses size_t");
_Static_assert(sizeof(((rawdinal_sensor_v1_plane *)0)->stride_samples) == sizeof(size_t), "stride uses size_t");
_Static_assert(sizeof(((rawdinal_sensor_v1_plane *)0)->sample_count) == sizeof(size_t), "count uses size_t");
_Static_assert(offsetof(rawdinal_sensor_v1_plane, identity) == 0, "identity leads the plane");
_Static_assert(offsetof(rawdinal_sensor_v1_plane, reserved) == sizeof(uint32_t), "reserved follows identity");
_Static_assert(offsetof(rawdinal_sensor_v1_plane, width) == 2 * sizeof(uint32_t), "dimensions follow identifiers");
_Static_assert(offsetof(rawdinal_sensor_v1_plane, samples) == 2 * sizeof(uint32_t) + 4 * sizeof(size_t), "samples follow sizes");
_Static_assert(sizeof(rawdinal_sensor_v1_plane) == offsetof(rawdinal_sensor_v1_plane, samples) + sizeof(const uint16_t *), "plane has no undocumented storage");

#define CHECK(expression) do { if (!(expression)) return 1; } while (0)

static void put_u32(uint8_t *bytes, size_t offset, uint32_t value)
{
  bytes[offset] = (uint8_t)value;
  bytes[offset + 1] = (uint8_t)(value >> 8);
  bytes[offset + 2] = (uint8_t)(value >> 16);
  bytes[offset + 3] = (uint8_t)(value >> 24);
}

static void put_u16(uint8_t *bytes, size_t offset, uint16_t value)
{
  bytes[offset] = (uint8_t)value;
  bytes[offset + 1] = (uint8_t)(value >> 8);
}

static void push_bits(uint8_t *bytes, size_t *bit_count, uint32_t value, uint32_t count)
{
  uint32_t shift;
  for (shift = count; shift > 0; --shift) {
    bytes[*bit_count / 8] |= (uint8_t)(((value >> (shift - 1)) & 1U) << (7 - (*bit_count % 8)));
    ++*bit_count;
  }
}

static size_t encode_bytes(const uint8_t *decoded, size_t decoded_length, uint8_t *stream)
{
  int32_t previous = 0;
  size_t bit_count = 0;
  size_t index;
  for (index = 0; index < decoded_length; ++index) {
    int32_t difference = (int32_t)decoded[index] - previous;
    uint32_t magnitude = (uint32_t)(difference < 0 ? -difference : difference);
    uint32_t length = 0;
    uint32_t encoded;
    while (magnitude > 0) {
      ++length;
      magnitude >>= 1;
    }
    encoded = difference < 0 ? (uint32_t)(difference + (1 << length) - 1) : (uint32_t)difference;
    push_bits(stream, &bit_count, length, 4);
    push_bits(stream, &bit_count, encoded, length);
    previous = decoded[index];
  }
  return (bit_count + 7) / 8;
}

static int sensor_fixture(uint8_t **input, size_t *input_length)
{
  uint8_t decoded[40] = {0};
  uint8_t stream[80] = {0};
  uint8_t raw[102] = {0};
  uint8_t *camf;
  uint8_t *file;
  uint8_t *directory;
  size_t stream_length;
  size_t camf_length;
  size_t directory_offset;
  size_t channel;

  memcpy(decoded, "CMbT", 4);
  put_u32(decoded, 8, 40);
  put_u32(decoded, 12, 20);
  put_u32(decoded, 16, 24);
  memcpy(decoded + 20, "Tag\0", 4);
  put_u32(decoded, 24, 12);
  memcpy(decoded + 28, "hello world\0", 12);
  stream_length = encode_bytes(decoded, sizeof(decoded), stream);
  camf_length = 60 + stream_length;
  camf = calloc(1, camf_length);
  file = calloc(1, 40 + camf_length + sizeof(raw) + 40);
  if (camf == NULL || file == NULL) {
    free(camf);
    free(file);
    return 0;
  }
  memcpy(camf, "SECc", 4);
  put_u32(camf, 4, 0x20000);
  put_u32(camf, 8, 5);
  put_u32(camf, 12, sizeof(decoded));
  for (channel = 0; channel < 9; ++channel) {
    camf[28 + 2 * channel] = 4;
    camf[29 + 2 * channel] = (uint8_t)(channel << 4);
  }
  put_u32(camf, 56, (uint32_t)stream_length);
  memcpy(camf + 60, stream, stream_length);
  memcpy(raw, "SECi", 4);
  put_u32(raw, 4, 0x20000);
  put_u32(raw, 8, 1);
  put_u32(raw, 12, 0x25);
  put_u32(raw, 16, 4);
  put_u32(raw, 20, 4);
  for (channel = 0; channel < 3; ++channel) {
    uint16_t size = channel == 2 ? 4 : 2;
    put_u16(raw, 28 + 4 * channel, size);
    put_u16(raw, 30 + 4 * channel, size);
    raw[40 + 2 * channel] = (uint8_t)(10 + channel);
  }
  raw[48] = 1;
  put_u32(raw, 56, 1);
  put_u32(raw, 60, 1);
  put_u32(raw, 64, 2);
  memcpy(file, "FOVb", 4);
  put_u32(file, 4, 0x40002);
  memcpy(file + 40, camf, camf_length);
  memcpy(file + 40 + camf_length, raw, sizeof(raw));
  directory_offset = 40 + camf_length + sizeof(raw);
  directory = file + directory_offset;
  memcpy(directory, "SECd", 4);
  put_u32(directory, 4, 0x20000);
  put_u32(directory, 8, 2);
  put_u32(directory, 12, 40);
  put_u32(directory, 16, (uint32_t)camf_length);
  memcpy(directory + 20, "CAMF", 4);
  put_u32(directory, 24, (uint32_t)(40 + camf_length));
  put_u32(directory, 28, sizeof(raw));
  memcpy(directory + 32, "IMA2", 4);
  put_u32(directory, 36, (uint32_t)directory_offset);
  free(camf);
  *input = file;
  *input_length = directory_offset + 40;
  return 1;
}

int main(void)
{
  static const uint8_t sentinel[] = {1};
  uint8_t *input = NULL;
  size_t input_length = 0;
  rawdinal_sensor_v1_image *image = (rawdinal_sensor_v1_image *)(void *)sentinel;
  rawdinal_sensor_v1_plane plane = {1, 1, 1, 1, 1, 1, (const uint16_t *)(const void *)sentinel};
  const uint8_t *camf = sentinel;
  const uint8_t *exif = sentinel;
  size_t camf_length = 1;
  size_t exif_length = 1;
  char error[32] = {0};

  CHECK(sensor_fixture(&input, &input_length));
  CHECK(rawdinal_sensor_v1_decode(input, input_length, &image, error, sizeof(error)) == RAWDINAL_STATUS_ERROR);
  CHECK(image == NULL && error[0] != '\0');
  free(input);
  CHECK(rawdinal_sensor_v1_get_plane(NULL, RAWDINAL_SENSOR_V1_BOTTOM, &plane) == RAWDINAL_STATUS_ERROR);
  CHECK(plane.identity == 0 && plane.reserved == 0 && plane.width == 0 && plane.height == 0 && plane.stride_samples == 0 && plane.sample_count == 0 && plane.samples == NULL);
  CHECK(rawdinal_sensor_v1_get_camf(NULL, &camf, &camf_length) == RAWDINAL_STATUS_ERROR);
  CHECK(camf == NULL && camf_length == 0);
  CHECK(rawdinal_sensor_v1_get_exif(NULL, &exif, &exif_length) == RAWDINAL_STATUS_ERROR);
  CHECK(exif == NULL && exif_length == 0);
  image = (rawdinal_sensor_v1_image *)(void *)sentinel;
  CHECK(rawdinal_sensor_v1_decode(NULL, input_length, &image, error, sizeof(error)) == RAWDINAL_STATUS_ERROR);
  CHECK(image == NULL);
  rawdinal_sensor_v1_free(NULL);
  return 0;
}
