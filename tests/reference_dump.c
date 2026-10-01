/* SPDX-License-Identifier: GPL-3.0-only */
/* Copyright (C) 2026 Paolo SANTUCCI */

#include "x3f_io.h"
#include <stdint.h>
#include <stdio.h>

#ifndef X3F_REFERENCE_DECODER
#define X3F_REFERENCE_DECODER "unknown"
#endif

typedef struct
{
  x3f_area16_t *area;
  uint32_t component;
  const char *source;
} plane_source_t;

static int fail(const char *message)
{
  fprintf(stderr, "reference_dump: %s\n", message);
  return 1;
}

static int output_path(char *path, size_t capacity, const char *directory, const char *name)
{
  const int count = snprintf(path, capacity, "%s/%s", directory, name);
  return count >= 0 && (size_t)count < capacity;
}

static int valid_plane(const plane_source_t *plane)
{
  if(plane->area == NULL || plane->area->data == NULL || plane->area->rows == 0
     || plane->area->columns == 0 || plane->area->channels <= plane->component)
    return 0;
  const uint64_t row_samples = (uint64_t)plane->area->columns * plane->area->channels;
  return row_samples <= plane->area->row_stride;
}

static int write_plane(const char *directory, uint32_t identity, const plane_source_t *plane)
{
  char name[32];
  const int count = snprintf(name, sizeof(name), "layer-%u.u16le", identity);
  if(count < 0 || (size_t)count >= sizeof(name)) return fail("plane filename is too long");
  char path[1024];
  if(!output_path(path, sizeof(path), directory, name)) return fail("plane output path is too long");
  FILE *output = fopen(path, "wbx");
  if(output == NULL) return fail("unable to create plane output");

  for(uint32_t row = 0; row < plane->area->rows; row++)
    for(uint32_t column = 0; column < plane->area->columns; column++)
    {
      const size_t offset = (size_t)row * plane->area->row_stride
                            + (size_t)column * plane->area->channels + plane->component;
      const uint16_t value = plane->area->data[offset];
      if(fputc(value & 255, output) == EOF || fputc(value >> 8, output) == EOF)
      {
        fclose(output);
        return fail("unable to write plane output");
      }
    }
  if(fclose(output) != 0) return fail("unable to close plane output");
  return 0;
}

static int write_calibration(const char *directory, const x3f_camf_t *calibration)
{
  char path[1024];
  if(!output_path(path, sizeof(path), directory, "calibration.bin"))
    return fail("calibration output path is too long");
  FILE *output = fopen(path, "wbx");
  if(output == NULL) return fail("unable to create calibration output");
  if(fwrite(calibration->decoded_data, 1, calibration->decoded_data_size, output)
     != calibration->decoded_data_size)
  {
    fclose(output);
    return fail("unable to write calibration output");
  }
  if(fclose(output) != 0) return fail("unable to close calibration output");
  return 0;
}

static int write_manifest(const char *directory, const x3f_t *file, const x3f_image_data_t *image,
                          const x3f_camf_t *calibration, const plane_source_t planes[TRUE_PLANES])
{
  char path[1024];
  if(!output_path(path, sizeof(path), directory, "reference.txt"))
    return fail("manifest output path is too long");
  FILE *output = fopen(path, "wbx");
  if(output == NULL) return fail("unable to create manifest output");
  if(fprintf(output,
             "schema=1\n"
             "decoder=%s\n"
             "x3f_version=0x%08x\n"
             "raw_type_format=0x%08x\n"
             "camf_type=%u\n"
             "camf_bytes=%u\n",
             X3F_REFERENCE_DECODER, file->header.version, image->type_format, calibration->type,
             calibration->decoded_data_size)
     < 0)
  {
    fclose(output);
    return fail("unable to write manifest");
  }
  for(uint32_t identity = 0; identity < TRUE_PLANES; identity++)
    if(fprintf(output,
               "layer_%u_width=%u\n"
               "layer_%u_height=%u\n"
               "layer_%u_source=%s\n"
               "layer_%u_component=%u\n",
               identity, planes[identity].area->columns, identity, planes[identity].area->rows,
               identity, planes[identity].source, identity, planes[identity].component)
       < 0)
    {
      fclose(output);
      return fail("unable to write manifest");
    }
  if(fclose(output) != 0) return fail("unable to close manifest output");
  return 0;
}

static int select_planes(x3f_image_data_t *image, plane_source_t planes[TRUE_PLANES],
                         uint32_t *expected_camf_type)
{
  if(image->tru == NULL) return fail("missing TRUE decoder output");
  switch(image->type_format)
  {
    case X3F_IMAGE_RAW_MERRILL:
      *expected_camf_type = 4;
      for(uint32_t identity = 0; identity < TRUE_PLANES; identity++)
      {
        planes[identity].area = &image->tru->x3rgb16;
        planes[identity].component = identity;
        planes[identity].source = "x3rgb16";
      }
      break;
    case X3F_IMAGE_RAW_QUATTRO:
    case X3F_IMAGE_RAW_SDQ:
    case X3F_IMAGE_RAW_SDQH:
      if(image->quattro == NULL || !image->quattro->quattro_layout)
        return fail("missing split-resolution Quattro decoder output");
      *expected_camf_type = 5;
      planes[0] = (plane_source_t){ &image->tru->x3rgb16, 0, "x3rgb16" };
      planes[1] = (plane_source_t){ &image->tru->x3rgb16, 1, "x3rgb16" };
      planes[2] = (plane_source_t){ &image->quattro->top16, 0, "top16" };
      break;
    default:
      return fail("unsupported X3F RAW format");
  }
  for(uint32_t identity = 0; identity < TRUE_PLANES; identity++)
    if(!valid_plane(&planes[identity])) return fail("invalid decoded plane layout");
  return 0;
}

/** Generate independent physical-plane and CAMF fixtures with a pinned X3F Tools decoder. */
int main(int argc, char **argv)
{
  if(argc != 3) return fail("usage: reference_dump INPUT.x3f EXISTING_OUTPUT_DIRECTORY");
  FILE *input = fopen(argv[1], "rb");
  if(input == NULL) return fail("unable to open input");
  x3f_t *file = x3f_new_from_file(input);
  if(file == NULL)
  {
    fclose(input);
    return fail("unable to parse X3F container");
  }
  x3f_directory_entry_t *raw = x3f_get_raw(file);
  x3f_directory_entry_t *camf = x3f_get_camf(file);
  if(raw == NULL || camf == NULL)
  {
    x3f_delete(file);
    fclose(input);
    return fail("missing RAW or CAMF section");
  }
  if(x3f_load_data(file, raw) != X3F_OK || x3f_load_data(file, camf) != X3F_OK)
  {
    x3f_delete(file);
    fclose(input);
    return fail("X3F Tools reported a decode failure");
  }

  x3f_image_data_t *image = &raw->header.data_subsection.image_data;
  x3f_camf_t *calibration = &camf->header.data_subsection.camf;
  plane_source_t planes[TRUE_PLANES] = { 0 };
  uint32_t expected_camf_type = 0;
  int status = select_planes(image, planes, &expected_camf_type);
  if(status == 0
     && (calibration->type != expected_camf_type || calibration->decoded_data == NULL
         || calibration->decoded_data_size == 0))
    status = fail("invalid decoded CAMF layout");
  for(uint32_t identity = 0; status == 0 && identity < TRUE_PLANES; identity++)
    status = write_plane(argv[2], identity, &planes[identity]);
  if(status == 0) status = write_calibration(argv[2], calibration);
  if(status == 0) status = write_manifest(argv[2], file, image, calibration, planes);

  if(x3f_delete(file) != X3F_OK && status == 0) status = fail("unable to release X3F decoder");
  if(fclose(input) != 0 && status == 0) status = fail("unable to close input");
  return status;
}
