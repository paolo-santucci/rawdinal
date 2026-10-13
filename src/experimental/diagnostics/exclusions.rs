// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Paolo SANTUCCI

//! Diagnostic-only cause bits. Exact combinations partition the population; marginal counts overlap.

pub(super) const ENCODED_MAXIMUM: u8 = 4;
pub(super) const REPAIR_CLIPPED_DONOR: u8 = 8;

/// Converts binary clipping provenance into causes while retaining the original defect locations.
/// Donor clipping is recorded only at repaired sites; a defective source code is not a clipping observation.
pub(super) fn attribute_sources(defects: &[u8], clipping: &[u8], influence: &mut [u8]) {
    for ((flags, &defect), &clipped) in influence.iter_mut().zip(defects).zip(clipping) {
        let donor_clipped = defect != 0 && *flags != 0;
        *flags = defect
            | if clipped != 0 { ENCODED_MAXIMUM } else { 0 }
            | if donor_clipped {
                REPAIR_CLIPPED_DONOR
            } else {
                0
            };
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Counts {
    pub(super) combinations: [usize; 16],
}

impl Counts {
    pub(super) fn record(&mut self, causes: u8) {
        self.combinations[usize::from(causes)] += 1;
    }

    pub(super) fn excluded(&self) -> usize {
        self.combinations[1..].iter().sum()
    }

    pub(super) fn containing(&self, cause: u8) -> usize {
        self.combinations
            .iter()
            .enumerate()
            .filter(|(mask, _)| mask & usize::from(cause) != 0)
            .map(|(_, count)| count)
            .sum()
    }

    pub(super) fn overlapping(&self) -> usize {
        self.combinations
            .iter()
            .enumerate()
            .filter(|(mask, _)| mask.count_ones() > 1)
            .map(|(_, count)| count)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experimental::defects::{AF_PIXEL, BAD_PIXEL};

    #[test]
    fn source_causes_separate_repair_donors_from_direct_clipping() {
        let defects = [0, BAD_PIXEL, AF_PIXEL, BAD_PIXEL | AF_PIXEL, 0];
        let clipping = [1, 0, 0, 0, 0];
        let mut influence = [1, 1, 0, 1, 0];
        attribute_sources(&defects, &clipping, &mut influence);
        assert_eq!(
            influence,
            [
                ENCODED_MAXIMUM,
                BAD_PIXEL | REPAIR_CLIPPED_DONOR,
                AF_PIXEL,
                BAD_PIXEL | AF_PIXEL | REPAIR_CLIPPED_DONOR,
                0
            ]
        );
        assert_eq!(
            influence.map(|flags| flags != 0),
            [true, true, true, true, false]
        );
    }

    #[test]
    fn exact_combinations_partition_exclusions_without_double_counting() {
        let mut counts = Counts::default();
        for causes in [
            0,
            AF_PIXEL,
            AF_PIXEL | BAD_PIXEL,
            BAD_PIXEL | ENCODED_MAXIMUM,
            BAD_PIXEL | ENCODED_MAXIMUM | REPAIR_CLIPPED_DONOR,
            0,
        ] {
            counts.record(causes);
        }
        assert_eq!(
            counts.combinations,
            [2, 0, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0]
        );
        assert_eq!(counts.excluded(), 4);
        assert_eq!(counts.containing(AF_PIXEL), 2);
        assert_eq!(counts.containing(BAD_PIXEL), 3);
        assert_eq!(counts.containing(ENCODED_MAXIMUM), 2);
        assert_eq!(counts.containing(REPAIR_CLIPPED_DONOR), 1);
        assert_eq!(counts.overlapping(), 3);
    }
}
