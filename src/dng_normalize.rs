use crate::probe::DngRawFacts;
use crate::{DecodeError, ProbeResult};

pub(crate) struct Normalization<'a> {
    facts: &'a DngRawFacts,
    active: [u32; 4],
    repeat: [u32; 2],
    denominator: [f64; 3],
    white: [f64; 3],
}

impl<'a> Normalization<'a> {
    pub fn new(facts: &'a DngRawFacts) -> ProbeResult<Self> {
        let active = facts
            .active_area
            .unwrap_or([0, 0, facts.height, facts.width]);
        let repeat = facts.black_level_repeat_dim.unwrap_or([1, 1]);
        let count = u64::from(repeat[0]) * u64::from(repeat[1]) * 3;
        if repeat.contains(&0)
            || repeat[0] > active[2] - active[0]
            || repeat[1] > active[3] - active[1]
            || facts
                .black_level
                .as_ref()
                .is_some_and(|values| values.len() as u64 != count)
            || facts
                .white_level
                .as_ref()
                .is_some_and(|values| values.len() != 3)
            || facts
                .black_level_delta_h
                .as_ref()
                .is_some_and(|values| values.len() != (active[3] - active[1]) as usize)
            || facts
                .black_level_delta_v
                .as_ref()
                .is_some_and(|values| values.len() != (active[2] - active[0]) as usize)
            || facts
                .linearization_table
                .as_ref()
                .is_some_and(Vec::is_empty)
        {
            return Err(DecodeError::InvalidTag);
        }
        let default_white = ((1u32 << facts.bits_per_sample[0]) - 1) as f64;
        let mut model = Self {
            facts,
            active,
            repeat,
            denominator: [0.0; 3],
            white: [default_white; 3],
        };
        if let Some(values) = &facts.white_level {
            model.white.copy_from_slice(values);
        }
        for channel in 0..3 {
            let mut max_black = f64::NEG_INFINITY;
            for y in 0..repeat[0] as usize {
                for x in 0..repeat[1] as usize {
                    let black = model.base_black(y, x, channel)
                        + maximum_phase(
                            facts.black_level_delta_h.as_deref(),
                            x,
                            repeat[1] as usize,
                        )
                        + maximum_phase(
                            facts.black_level_delta_v.as_deref(),
                            y,
                            repeat[0] as usize,
                        );
                    max_black = max_black.max(black);
                }
            }
            model.denominator[channel] = model.white[channel] - max_black;
            if !model.denominator[channel].is_finite() || model.denominator[channel] <= 0.0 {
                return Err(DecodeError::InvalidTag);
            }
        }
        Ok(model)
    }

    fn base_black(&self, row: usize, column: usize, channel: usize) -> f64 {
        self.facts.black_level.as_ref().map_or(0.0, |levels| {
            levels[((row % self.repeat[0] as usize) * self.repeat[1] as usize
                + column % self.repeat[1] as usize)
                * 3
                + channel]
        })
    }

    pub fn flags(&self, samples: &[f32]) -> ProbeResult<Vec<u8>> {
        let mut flags = crate::dng_metadata::allocate(samples.len())?;
        let [top, left, bottom, right] = self.active;
        let encoded_max = ((1u32 << self.facts.bits_per_sample[0]) - 1) as f32;
        for (index, &sample) in samples.iter().enumerate() {
            let row = (index / 3) / self.facts.width as usize;
            let column = (index / 3) % self.facts.width as usize;
            let linear = self.linearize(sample);
            flags.push(
                u8::from(sample >= encoded_max)
                    | (u8::from(linear >= self.white[index % 3]) << 1)
                    | (u8::from(
                        row < top as usize
                            || row >= bottom as usize
                            || column < left as usize
                            || column >= right as usize,
                    ) << 2),
            );
        }
        Ok(flags)
    }

    fn linearize(&self, sample: f32) -> f64 {
        self.facts
            .linearization_table
            .as_ref()
            .map_or(f64::from(sample), |table| {
                f64::from(table[((sample.max(0.0).round()) as usize).min(table.len() - 1)])
            })
    }

    pub fn apply(&self, samples: &mut [f32]) -> ProbeResult<()> {
        let width = self.facts.width as usize;
        let [top, left, bottom, right] = self.active;
        for (index, sample) in samples.iter_mut().enumerate() {
            let channel = index % 3;
            let pixel = index / 3;
            let y = pixel / width;
            let x = pixel % width;
            let row = (y as u32).clamp(top, bottom - 1) - top;
            let column = (x as u32).clamp(left, right - 1) - left;
            let black = self.base_black(row as usize, column as usize, channel)
                + self
                    .facts
                    .black_level_delta_h
                    .as_ref()
                    .map_or(0.0, |values| values[column as usize])
                + self
                    .facts
                    .black_level_delta_v
                    .as_ref()
                    .map_or(0.0, |values| values[row as usize]);
            let linear = self.linearize(*sample);
            *sample = ((linear - black) / self.denominator[channel]) as f32;
            if !sample.is_finite() {
                return Err(DecodeError::InvalidTag);
            }
        }
        Ok(())
    }
}

fn maximum_phase(values: Option<&[f64]>, start: usize, step: usize) -> f64 {
    values.map_or(0.0, |values| {
        values
            .iter()
            .skip(start)
            .step_by(step)
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
    })
}
