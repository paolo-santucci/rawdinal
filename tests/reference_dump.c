/* SPDX-License-Identifier: GPL-3.0-only */
/* Copyright (C) 2026 Paolo SANTUCCI */

#include "x3f_io.h"
#include <stdint.h>
#include <stdio.h>

#define IS_NULL_PTR(pointer) ((pointer) == NULL)

/** Generate independent fixtures from trusted sd Quattro samples using X3F Tools. */
int main(int argc, char **argv)
{
  if(argc != 3) return 1;
  FILE *input = fopen(argv[1], "rb");
  if(IS_NULL_PTR(input)) return 1;
  x3f_t *file = x3f_new_from_file(input);
  if(IS_NULL_PTR(file)) return 1;
  x3f_directory_entry_t *raw = x3f_get_raw(file);
  x3f_directory_entry_t *camf = x3f_get_camf(file);
  if(IS_NULL_PTR(raw) || IS_NULL_PTR(camf)) return 1;
  x3f_load_data(file, raw);
  x3f_load_data(file, camf);
  x3f_image_data_t *image = &raw->header.data_subsection.image_data;
  for(int channel = 0; channel < 3; channel++)
  {
    char path[1024];
    const int count = snprintf(path, sizeof(path), "%s/layer-%d.u16le", argv[2], channel);
    if(count < 0 || (size_t)count >= sizeof(path)) return 1;
    FILE *output = fopen(path, "wbx");
    if(IS_NULL_PTR(output)) return 1;
    x3f_area16_t *area = channel == 2 ? &image->quattro->top16 : &image->tru->x3rgb16;
    for(uint32_t row = 0; row < area->rows; row++)
      for(uint32_t col = 0; col < area->columns; col++)
      {
        const uint16_t value = area->data[row * area->row_stride + col * area->channels
                                          + (channel == 2 ? 0 : channel)];
        if(fputc(value & 255, output) == EOF || fputc(value >> 8, output) == EOF) return 1;
      }
    if(fclose(output)) return 1;
  }
  char path[1024];
  const int count = snprintf(path, sizeof(path), "%s/calibration.bin", argv[2]);
  if(count < 0 || (size_t)count >= sizeof(path)) return 1;
  FILE *output = fopen(path, "wbx");
  if(IS_NULL_PTR(output)) return 1;
  x3f_camf_t *calibration = &camf->header.data_subsection.camf;
  if(fwrite(calibration->decoded_data, 1, calibration->decoded_data_size, output)
     != calibration->decoded_data_size) return 1;
  if(fclose(output)) return 1;
  x3f_delete(file);
  fclose(input);
  return 0;
}
