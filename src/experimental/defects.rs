// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! Original-sd-Quattro defect locations, retaining overlapping CAMF and AF causes.

use crate::{Calibration, Plane, Result, invalid, zeroed};

pub(super) const BAD_PIXEL: u8 = 1;
pub(super) const AF_PIXEL: u8 = 2;

pub(super) fn marked_pixels(
    plane: &Plane,
    calibration: &Calibration,
    channel: usize,
) -> Result<Vec<u8>> {
    let name = if channel == 2 {
        "BadPixelsLumaF23"
    } else {
        "BadPixelsChromaF23"
    };
    let mut marked = zeroed(plane.samples.len())?;
    mark_bad_pixels(
        &mut marked,
        plane.width,
        plane.height,
        &calibration.matrix(name)?.values,
    )?;
    mark_af_pixels(&mut marked, plane.width, channel);
    Ok(marked)
}

fn mark_bad_pixels(marked: &mut [u8], width: usize, height: usize, table: &[f64]) -> Result<()> {
    let mut position = 0;
    while position < table.len() {
        let row = table[position] as usize;
        if table[position].fract() != 0.0 || table[position] < 0.0 || row >= height {
            return Err(invalid("invalid bad-pixel row"));
        }
        position += 1;
        loop {
            let &column = table
                .get(position)
                .ok_or_else(|| invalid("truncated bad-pixel list"))?;
            position += 1;
            if column == 0.0 {
                break;
            }
            if column.fract() != 0.0
                || column < 0.0
                || column as usize >= width
                || position >= table.len()
            {
                return Err(invalid("invalid bad-pixel column"));
            }
            marked[row * width + column as usize] |= BAD_PIXEL;
            position += 1;
        }
    }
    Ok(())
}

/// The caller validates the original sd Quattro's full encoded geometry before marking AF sites.
fn mark_af_pixels(marked: &mut [u8], width: usize, channel: usize) {
    let (x0, x1, dx, y0, y1, dy, rows) = if channel == 2 {
        (217, 5641, 16, 464, 3312, 32, 2)
    } else {
        (108, 2820, 8, 232, 1656, 16, 1)
    };
    for y in (y0..=y1).step_by(dy) {
        for x in (x0..=x1).step_by(dx) {
            for row in y..y + rows {
                marked[row * width + x] |= AF_PIXEL;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_pixel_lists_preserve_existing_causes_and_skip_descriptor_values() {
        let mut marked = vec![0; 12];
        marked[5] = AF_PIXEL;
        mark_bad_pixels(
            &mut marked,
            4,
            3,
            &[1.0, 1.0, 99.0, 3.0, 98.0, 0.0, 2.0, 2.0, 97.0, 0.0],
        )
        .unwrap();
        assert_eq!(marked[5], BAD_PIXEL | AF_PIXEL);
        assert_eq!(marked[7], BAD_PIXEL);
        assert_eq!(marked[10], BAD_PIXEL);
        assert_eq!(marked.iter().filter(|&&flags| flags != 0).count(), 3);
    }

    #[test]
    fn bad_pixel_lists_reject_invalid_coordinates_and_missing_terminators() {
        for table in [
            &[-1.0, 0.0][..],
            &[3.0, 0.0],
            &[0.5, 0.0],
            &[1.0],
            &[1.0, -1.0],
            &[1.0, 4.0, 0.0, 0.0],
            &[1.0, 1.5, 0.0, 0.0],
            &[1.0, 1.0],
            &[1.0, 1.0, 99.0],
        ] {
            assert!(
                mark_bad_pixels(&mut [0; 12], 4, 3, table).is_err(),
                "{table:?}"
            );
        }
    }

    #[test]
    fn autofocus_grids_preserve_camf_overlap_at_both_spatial_resolutions() {
        for (channel, width, height, first, last, count) in [
            (0, 2944, 1888, (108, 232), (2820, 1656), 30600),
            (2, 5888, 3776, (217, 464), (5641, 3313), 61200),
        ] {
            let mut marked = vec![0; width * height];
            marked[first.1 * width + first.0] = BAD_PIXEL;
            mark_af_pixels(&mut marked, width, channel);
            assert_eq!(marked[first.1 * width + first.0], BAD_PIXEL | AF_PIXEL);
            assert_eq!(marked[last.1 * width + last.0], AF_PIXEL);
            assert_eq!(
                marked
                    .iter()
                    .filter(|&&flags| flags & AF_PIXEL != 0)
                    .count(),
                count
            );
            assert_eq!(marked[first.1 * width + first.0 + 1], 0);
        }
    }
}
