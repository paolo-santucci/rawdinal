#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "rawdinal.h"

_Static_assert(RAWDINAL_CLIPPING_V1_INFO_VERSION == UINT32_C(1), "clipping descriptor version is stable");
_Static_assert(RAWDINAL_CLIPPING_V1_THRESHOLD_UNKNOWN == UINT32_C(0), "unknown threshold provenance is stable");
_Static_assert(RAWDINAL_CLIPPING_V1_THRESHOLD_ESTIMATED_ENCODED_MAXIMUM == UINT32_C(1), "clipping threshold provenance is stable");
_Static_assert(RAWDINAL_CLIPPING_V1_THRESHOLD_CALIBRATED == UINT32_C(2), "calibrated threshold provenance is stable");
_Static_assert(offsetof(rawdinal_clipping_v1_plane, identity) == 0, "identity leads the plane");
_Static_assert(offsetof(rawdinal_clipping_v1_plane, threshold_code) == sizeof(uint32_t), "threshold follows identity");
_Static_assert(offsetof(rawdinal_clipping_v1_plane, width) == 4 * sizeof(uint32_t), "plane sizes follow identifiers");
_Static_assert(offsetof(rawdinal_clipping_v1_plane, data) == 4 * sizeof(uint32_t) + 4 * sizeof(size_t), "plane data follows sizes");
_Static_assert(sizeof(rawdinal_clipping_v1_plane) == 4 * sizeof(uint32_t) + 4 * sizeof(size_t) + sizeof(const uint8_t *), "plane has no undocumented storage");
_Static_assert(offsetof(rawdinal_clipping_v1_info, version) == 0, "version leads the descriptor");
_Static_assert(offsetof(rawdinal_clipping_v1_info, width) == 2 * sizeof(uint32_t), "output sizes follow version fields");
_Static_assert(offsetof(rawdinal_clipping_v1_info, data) == 2 * sizeof(uint32_t) + 4 * sizeof(size_t), "output data follows sizes");
_Static_assert(offsetof(rawdinal_clipping_v1_info, planes) == 2 * sizeof(uint32_t) + 4 * sizeof(size_t) + sizeof(const uint8_t *), "planes follow output data");
_Static_assert(sizeof(((rawdinal_clipping_v1_info *)0)->planes) == 3 * sizeof(rawdinal_clipping_v1_plane), "descriptor has three physical planes");
_Static_assert(sizeof(rawdinal_clipping_v1_info) == offsetof(rawdinal_clipping_v1_info, planes) + 3 * sizeof(rawdinal_clipping_v1_plane), "descriptor has no undocumented storage");

static int clipping_info_is_cleared(const rawdinal_clipping_v1_info *info)
{
  if (info->version != 0 || info->reserved != 0 || info->width != 0 || info->height != 0 ||
      info->stride_bytes != 0 || info->byte_count != 0 || info->data != NULL) {
    return 0;
  }
  for (size_t index = 0; index < 3; ++index) {
    const rawdinal_clipping_v1_plane *plane = &info->planes[index];
    if (plane->identity != 0 || plane->threshold_code != 0 ||
        plane->threshold_provenance != 0 || plane->reserved != 0 || plane->width != 0 ||
        plane->height != 0 || plane->stride_bytes != 0 || plane->byte_count != 0 ||
        plane->data != NULL) {
      return 0;
    }
  }
  return 1;
}

int main(void)
{
  rawdinal_image *image = (rawdinal_image *)(uintptr_t)1;
  rawdinal_info decoded = { .width = 99, .height = 99,
                           .exif = (const uint8_t *)(uintptr_t)1, .exif_size = 99 };
  char error[1] = { 'x' };
  rawdinal_clipping_v1_info info;

  if (rawdinal_decode_with_clipping_v1(NULL, 0, &image, &decoded, error, sizeof(error)) == RAWDINAL_STATUS_OK ||
      image != NULL || decoded.width != 0 || decoded.height != 0 || decoded.exif != NULL ||
      decoded.exif_size != 0 || error[0] != '\0') {
    return 1;
  }
  memset(&info, 0xff, sizeof(info));
  if (rawdinal_get_clipping_v1(NULL, &info) == RAWDINAL_STATUS_OK) {
    return 1;
  }
  return !clipping_info_is_cleared(&info);
}
