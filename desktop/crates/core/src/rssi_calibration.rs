//! RSSI calibration primitives shared by Tauri/daemon.

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CalibrationProfile {
    pub median_dbm: i16,
    pub p10_dbm: i16,
    pub p90_dbm: i16,
    pub away_threshold_dbm: i16,
    pub samples: usize,
}

pub fn calibrate(samples: &[i16], away_margin_db: i16) -> Option<CalibrationProfile> {
    if samples.len() < 10 { return None; }
    let mut values: Vec<i16> = samples.iter().copied().filter(|v| *v <= 0 && *v >= -127).collect();
    if values.len() < 10 { return None; }
    values.sort_unstable();
    let median = percentile(&values, 0.50);
    let p10 = percentile(&values, 0.10);
    let p90 = percentile(&values, 0.90);
    Some(CalibrationProfile { median_dbm: median, p10_dbm: p10, p90_dbm: p90, away_threshold_dbm: median.saturating_sub(away_margin_db.max(1)), samples: values.len() })
}

fn percentile(sorted: &[i16], q: f64) -> i16 {
    if sorted.len() == 1 { return sorted[0]; }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    sorted[pos.round() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn rejects_short_samples() { assert!(calibrate(&[-50; 9], 15).is_none()); }
    #[test] fn calculates_profile() { let samples: Vec<i16> = (-60..-40).collect(); let p = calibrate(&samples, 15).unwrap(); assert_eq!(p.samples, 20); assert!(p.p10_dbm <= p.median_dbm); assert!(p.p90_dbm >= p.median_dbm); }
}
