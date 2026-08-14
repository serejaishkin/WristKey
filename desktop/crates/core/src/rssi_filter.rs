//! RSSI smoothing filters — reduces jitter and false lock/unlock triggers.

/// Exponential Moving Average (EMA) — lightweight, no state history needed.
/// Alpha = 0.3 (30% new value, 70% history) — good balance of responsiveness vs smoothness.
pub struct EmaFilter {
    alpha: f32,
    value: Option<f32>,
}

impl EmaFilter {
    pub fn new(alpha: f32) -> Self {
        Self { alpha: alpha.clamp(0.01, 0.99), value: None }
    }

    pub fn update(&mut self, raw: i16) -> i16 {
        let raw_f = raw as f32;
        let smoothed = match self.value {
            Some(prev) => prev * (1.0 - self.alpha) + raw_f * self.alpha,
            None => raw_f,
        };
        self.value = Some(smoothed);
        smoothed.round() as i16
    }

    pub fn reset(&mut self) {
        self.value = None;
    }

    pub fn current(&self) -> Option<i16> {
        self.value.map(|v| v.round() as i16)
    }
}

/// Simple Kalman filter for RSSI — models signal as noisy measurement of true distance.
/// Process noise (Q) = 0.01 — signal changes slowly
/// Measurement noise (R) = 4.0 — RSSI variance ~ ±6 dBm
pub struct KalmanFilter {
    q: f32,  // process noise
    r: f32,  // measurement noise
    x: f32,  // estimated value
    p: f32,  // error covariance
    initialized: bool,
}

impl KalmanFilter {
    pub fn new() -> Self {
        Self {
            q: 0.01,
            r: 4.0,
            x: 0.0,
            p: 1.0,
            initialized: false,
        }
    }

    pub fn with_noise(q: f32, r: f32) -> Self {
        Self {
            q: q.clamp(0.001, 1.0),
            r: r.clamp(0.1, 100.0),
            x: 0.0,
            p: 1.0,
            initialized: false,
        }
    }

    pub fn update(&mut self, measurement: i16) -> i16 {
        let z = measurement as f32;
        if !self.initialized {
            self.x = z;
            self.initialized = true;
            return measurement;
        }

        // Prediction
        self.p += self.q;

        // Update
        let k = self.p / (self.p + self.r);  // Kalman gain
        self.x += k * (z - self.x);
        self.p *= (1.0 - k);

        self.x.round() as i16
    }

    pub fn reset(&mut self) {
        self.initialized = false;
        self.p = 1.0;
    }

    pub fn current(&self) -> Option<i16> {
        if self.initialized {
            Some(self.x.round() as i16)
        } else {
            None
        }
    }
}

/// Hysteresis gate — prevents rapid lock/unlock toggling.
/// Lock when RSSI < threshold - margin
/// Unlock when RSSI > threshold + margin
/// Margin = 3 dBm (typical RSSI jitter)
pub struct HysteresisGate {
    threshold: i16,
    margin: i16,
    state: bool,  // true = unlocked, false = locked
}

impl HysteresisGate {
    pub fn new(threshold: i16, margin: i16) -> Self {
        Self {
            threshold,
            margin: margin.max(1),
            state: false,  // start locked (safe default)
        }
    }

    /// Returns (new_state, changed)
    pub fn update(&mut self, rssi: i16) -> (bool, bool) {
        let lock_threshold = self.threshold - self.margin;
        let unlock_threshold = self.threshold + self.margin;

        let new_state = if self.state {
            // Currently unlocked — lock only if clearly below threshold
            rssi > lock_threshold
        } else {
            // Currently locked — unlock only if clearly above threshold
            rssi > unlock_threshold
        };

        let changed = new_state != self.state;
        self.state = new_state;
        (new_state, changed)
    }

    pub fn state(&self) -> bool {
        self.state
    }

    pub fn reset(&mut self) {
        self.state = false;
    }
}

/// Combined filter: Kalman → Hysteresis → action
/// Usage: call update() with raw RSSI, get (should_unlock, changed)
pub struct RssiSmoother {
    kalman: KalmanFilter,
    hysteresis: HysteresisGate,
}

impl RssiSmoother {
    pub fn new(threshold: i16) -> Self {
        Self {
            kalman: KalmanFilter::new(),
            hysteresis: HysteresisGate::new(threshold, 3),
        }
    }

    pub fn with_params(threshold: i16, margin: i16, q: f32, r: f32) -> Self {
        Self {
            kalman: KalmanFilter::with_noise(q, r),
            hysteresis: HysteresisGate::new(threshold, margin),
        }
    }

    pub fn update(&mut self, raw_rssi: i16) -> (bool, bool) {
        let smoothed = self.kalman.update(raw_rssi);
        self.hysteresis.update(smoothed)
    }

    pub fn reset(&mut self) {
        self.kalman.reset();
        self.hysteresis.reset();
    }

    pub fn current_rssi(&self) -> Option<i16> {
        self.kalman.current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ema() {
        let mut f = EmaFilter::new(0.3);
        assert_eq!(f.update(-50), -50);
        assert_eq!(f.update(-60), -53);  // 70% * -50 + 30% * -60 = -53
    }

    #[test]
    fn test_kalman() {
        let mut k = KalmanFilter::new();
        let v1 = k.update(-50);
        let v2 = k.update(-52);
        let v3 = k.update(-48);
        // Kalman should smooth these close values
        assert!(v1.abs_diff(v2) < 5);
        assert!(v2.abs_diff(v3) < 5);
    }

    #[test]
    fn test_hysteresis() {
        let mut h = HysteresisGate::new(-60, 3);
        // Start locked
        assert_eq!(h.update(-55), (true, true));   // unlock (>-57)
        assert_eq!(h.update(-58), (true, false));  // stay unlocked (>-63)
        assert_eq!(h.update(-65), (false, true));  // lock (<-63)
        assert_eq!(h.update(-61), (false, false)); // stay locked (<-57)
    }
}
