//! Robust RSSI calibration shared by all desktop frontends.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationProfile {
    pub median_dbm: i16,
    pub p10_dbm: i16,
    pub p90_dbm: i16,
    pub away_threshold_dbm: i16,
    pub samples: usize,
}

/// Builds a proximity baseline from real BLE RSSI observations.
/// Values outside the Bluetooth RSSI range are discarded.
pub fn calibrate(samples: &[i16], away_margin_db: i16) -> Option<CalibrationProfile> {
    let mut values: Vec<i16> = samples.iter().copied()
        .filter(|v| (-127..=0).contains(v))
        .collect();
    if values.len() < 30 { return None; }
    values.sort_unstable();
    let median = percentile(&values, 0.50);
    let p10 = percentile(&values, 0.10);
    let p90 = percentile(&values, 0.90);
    Some(CalibrationProfile {
        median_dbm: median,
        p10_dbm: p10,
        p90_dbm: p90,
        away_threshold_dbm: median.saturating_sub(away_margin_db.max(1)),
        samples: values.len(),
    })
}

fn percentile(values: &[i16], q: f64) -> i16 {
    let pos = q.clamp(0.0, 1.0) * (values.len() - 1) as f64;
    values[pos.round() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_real_sample_count() {
        assert!(calibrate(&vec![-50; 29], 15).is_none());
    }

    #[test]
    fn produces_stable_profile() {
        let samples: Vec<i16> = (0..60).map(|i| -60 + (i % 8) as i16).collect();
        let p = calibrate(&samples, 15).unwrap();
        assert_eq!(p.samples, 60);
        assert!(p.p10_dbm <= p.median_dbm);
        assert!(p.median_dbm <= p.p90_dbm);
        assert_eq!(p.away_threshold_dbm, p.median_dbm - 15);
    }
}
