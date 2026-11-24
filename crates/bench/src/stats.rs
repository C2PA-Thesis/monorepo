//! Summary statistics of a set of samples.

use serde::{Deserialize, Serialize};

/// Above this, the middle half of the samples spans too much of the median to
/// compare runs by the median alone.
const NOISY_SPREAD: f64 = 0.10;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub count: usize,
    pub median: f64,
    pub p25: f64,
    pub p75: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub stddev: f64,
}

impl Stats {
    pub fn of(values: &[f64]) -> Option<Stats> {
        if values.is_empty() {
            return None;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        let count = sorted.len();
        let mean = sorted.iter().sum::<f64>() / count as f64;
        let variance = match count {
            1 => 0.0,
            _ => sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (count - 1) as f64,
        };
        Some(Stats {
            count,
            median: quantile(&sorted, 0.5),
            p25: quantile(&sorted, 0.25),
            p75: quantile(&sorted, 0.75),
            min: sorted[0],
            max: sorted[count - 1],
            mean,
            stddev: variance.sqrt(),
        })
    }

    pub fn is_noisy(&self) -> bool {
        self.median > 0.0 && (self.p75 - self.p25) / self.median > NOISY_SPREAD
    }
}

/// Linear interpolation between closest ranks, on sorted values.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    let rank = q * (sorted.len() - 1) as f64;
    let (low, high) = (rank.floor() as usize, rank.ceil() as usize);
    sorted[low] + (sorted[high] - sorted[low]) * (rank - low as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_has_no_statistics() {
        assert_eq!(Stats::of(&[]), None);
    }

    #[test]
    fn one_sample_is_its_own_summary() {
        let stats = Stats::of(&[2.0]).unwrap();
        assert_eq!((stats.median, stats.p25, stats.p75), (2.0, 2.0, 2.0));
        assert_eq!(stats.stddev, 0.0);
    }

    #[test]
    fn quantiles_interpolate_and_ignore_input_order() {
        let stats = Stats::of(&[4.0, 1.0, 3.0, 2.0]).unwrap();
        assert_eq!(stats.median, 2.5);
        assert_eq!(stats.p25, 1.75);
        assert_eq!(stats.p75, 3.25);
        assert_eq!((stats.min, stats.max), (1.0, 4.0));
    }

    #[test]
    fn a_wide_middle_half_is_noisy() {
        assert!(Stats::of(&[1.0, 1.0, 2.0, 2.0]).unwrap().is_noisy());
        assert!(!Stats::of(&[10.0, 10.1, 10.2, 10.3]).unwrap().is_noisy());
    }
}
