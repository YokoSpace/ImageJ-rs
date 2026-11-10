use serde::{Deserialize, Serialize};

/// Fixed slider range used by ImageJ 1.54u `ContrastAdjuster`
/// (`original_imagej/ij/plugin/frame/ContrastAdjuster.java` line 33).
pub const SLIDER_RANGE: f64 = 256.0;

/// State of Brightness & Contrast display range adjustments, matching ImageJ 1.54u
/// `ContrastAdjuster` (`original_imagej/ij/plugin/frame/ContrastAdjuster.java`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ContrastState {
    pub default_min: f64,
    pub default_max: f64,
    pub min: f64,
    pub max: f64,
}

impl ContrastState {
    /// Creates a new `ContrastState` with explicit default and current display ranges.
    pub fn new(default_min: f64, default_max: f64, min: f64, max: f64) -> Self {
        Self {
            default_min,
            default_max,
            min,
            max,
        }
    }

    /// Creates a new `ContrastState` where current range equals default range.
    pub fn with_defaults(default_min: f64, default_max: f64) -> Self {
        Self {
            default_min,
            default_max,
            min: default_min,
            max: default_max,
        }
    }

    /// Sets the current display range directly (`ContrastAdjuster.java` line 432: `setMinAndMax`).
    pub fn set_min_and_max(&mut self, min: f64, max: f64) {
        self.min = min;
        self.max = max;
    }

    /// Adjusts the minimum slider value (`ContrastAdjuster.java` lines 541-553: `adjustMin`).
    pub fn adjust_min(&mut self, minvalue: f64) {
        let range = self.default_max - self.default_min;
        self.min = self.default_min + minvalue * range / (SLIDER_RANGE - 1.0);
        if self.max > self.default_max {
            self.max = self.default_max;
        }
        if self.min > self.max {
            self.max = self.min;
        }
    }

    /// Adjusts the maximum slider value (`ContrastAdjuster.java` lines 555-568: `adjustMax`).
    pub fn adjust_max(&mut self, maxvalue: f64) {
        let range = self.default_max - self.default_min;
        self.max = self.default_min + maxvalue * range / (SLIDER_RANGE - 1.0);
        if self.min < self.default_min {
            self.min = self.default_min;
        }
        if self.max < self.min {
            self.min = self.max;
        }
    }

    /// Adjusts brightness slider value (`ContrastAdjuster.java` lines 579-589: `adjustBrightness`).
    pub fn adjust_brightness(&mut self, bvalue: f64) {
        let range = self.default_max - self.default_min;
        let center = self.default_min + range * ((SLIDER_RANGE - bvalue) / SLIDER_RANGE);
        let width = self.max - self.min;
        self.min = center - width / 2.0;
        self.max = center + width / 2.0;
    }

    /// Adjusts contrast slider value (`ContrastAdjuster.java` lines 591-607: `adjustContrast`).
    pub fn adjust_contrast(&mut self, cvalue: f64) {
        let center = self.min + (self.max - self.min) / 2.0;
        let range = self.default_max - self.default_min;
        let mid = SLIDER_RANGE / 2.0;
        let slope = if cvalue <= mid {
            cvalue / mid
        } else {
            mid / (SLIDER_RANGE - cvalue)
        };
        if slope > 0.0 {
            self.min = center - (0.5 * range) / slope;
            self.max = center + (0.5 * range) / slope;
        }
    }

    /// Resets current display range back to default (`ContrastAdjuster.java` lines 620-621: `reset`).
    pub fn reset(&mut self) {
        self.min = self.default_min;
        self.max = self.default_max;
    }

    /// Calculates scrollbar positions `[min, max, brightness, contrast]` matching
    /// `ContrastAdjuster.java` lines 485-526 (`updateScrollBars` and `scaleDown`).
    pub fn slider_positions(&self) -> [i32; 4] {
        let range = self.default_max - self.default_min;
        if range <= 0.0 {
            return [0, 0, (SLIDER_RANGE / 2.0) as i32, (SLIDER_RANGE / 2.0) as i32];
        }

        // ContrastAdjuster.java line 522-526: scaleDown
        let scale_down = |mut v: f64| -> i32 {
            if v < self.default_min {
                v = self.default_min;
            }
            if v > self.default_max {
                v = self.default_max;
            }
            ((v - self.default_min) * (SLIDER_RANGE - 1.0) / range) as i32
        };

        let min_pos = scale_down(self.min);
        let max_pos = scale_down(self.max);

        // ContrastAdjuster.java lines 500-502: brightness
        let level = self.min + (self.max - self.min) / 2.0;
        let normalized_level = 1.0 - (level - self.default_min) / range;
        let brightness_pos = (normalized_level * SLIDER_RANGE) as i32;

        // ContrastAdjuster.java lines 487-491: contrast
        let mid = SLIDER_RANGE / 2.0;
        let span = self.max - self.min;
        let contrast_pos = if span <= 0.0 {
            SLIDER_RANGE as i32
        } else {
            let mut c = (range / span) * mid;
            if c > mid {
                c = SLIDER_RANGE - (span / range) * mid;
            }
            c as i32
        };

        [min_pos, max_pos, brightness_pos, contrast_pos]
    }
}

/// Standalone function for setting min and max range.
pub fn set_min_and_max(state: &mut ContrastState, min: f64, max: f64) {
    state.set_min_and_max(min, max);
}

/// Standalone function for adjusting minimum slider value.
pub fn adjust_min(state: &mut ContrastState, minvalue: f64) {
    state.adjust_min(minvalue);
}

/// Standalone function for adjusting maximum slider value.
pub fn adjust_max(state: &mut ContrastState, maxvalue: f64) {
    state.adjust_max(maxvalue);
}

/// Standalone function for adjusting brightness slider value.
pub fn adjust_brightness(state: &mut ContrastState, bvalue: f64) {
    state.adjust_brightness(bvalue);
}

/// Standalone function for adjusting contrast slider value.
pub fn adjust_contrast(state: &mut ContrastState, cvalue: f64) {
    state.adjust_contrast(cvalue);
}

/// Standalone function for resetting display range.
pub fn reset(state: &mut ContrastState) {
    state.reset();
}

/// Standalone function for getting the 4 slider positions `[min, max, brightness, contrast]`.
pub fn slider_positions(state: &ContrastState) -> [i32; 4] {
    state.slider_positions()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_test_initial_default_state() {
        // ContrastAdjuster.java lines 487-526:
        // When min == defaultMin and max == defaultMax:
        // - minSlider = scaleDown(defaultMin) = 0
        // - maxSlider = scaleDown(defaultMax) = 255
        // - level = defaultMin + (defaultMax-defaultMin)/2.0
        //   normalizedLevel = 1.0 - 0.5 = 0.5
        //   brightness = (int)(0.5 * 256.0) = 128
        // - c = (range / range) * 128 = 128.0; c > mid is false; contrast = 128
        let state_8bit = ContrastState::with_defaults(0.0, 255.0);
        assert_eq!(slider_positions(&state_8bit), [0, 255, 128, 128]);

        let state_16bit = ContrastState::with_defaults(0.0, 4095.0);
        assert_eq!(slider_positions(&state_16bit), [0, 255, 128, 128]);
    }

    #[test]
    fn table_test_adjust_min() {
        // ContrastAdjuster.java lines 543-547:
        // min = defaultMin + minvalue * (defaultMax - defaultMin) / (sliderRange - 1.0)
        // if (max > defaultMax) max = defaultMax
        // if (min > max) max = min
        let mut state_8 = ContrastState::with_defaults(0.0, 255.0);
        adjust_min(&mut state_8, 64.0);
        assert_eq!(state_8.min, 64.0);
        assert_eq!(state_8.max, 255.0);

        // Pushing min above max pushes max (ContrastAdjuster.java line 546)
        adjust_min(&mut state_8, 260.0);
        assert_eq!(state_8.min, 260.0);
        assert_eq!(state_8.max, 260.0);

        // 16-bit table test: 0..4095, sliderRange=256 -> (4095 - 0) / 255 = 16.058823529411764
        let mut state_16 = ContrastState::with_defaults(0.0, 4095.0);
        adjust_min(&mut state_16, 64.0);
        let expected_min_16 = 64.0 * 4095.0 / 255.0; // 1027.764705882353
        assert!((state_16.min - expected_min_16).abs() < 1e-9);
        assert_eq!(state_16.max, 4095.0);
        // Slider positions check
        let pos16 = slider_positions(&state_16);
        assert_eq!(pos16[0], 64);
        assert_eq!(pos16[1], 255);
        assert_eq!(pos16[2], 95);
        assert_eq!(pos16[3], 160);
    }

    #[test]
    fn table_test_adjust_max() {
        // ContrastAdjuster.java lines 557-562:
        // max = defaultMin + maxvalue * (defaultMax - defaultMin) / (sliderRange - 1.0)
        // if (min < defaultMin) min = defaultMin
        // if (max < min) min = max
        let mut state_8 = ContrastState::new(0.0, 255.0, 100.0, 255.0);
        adjust_max(&mut state_8, 192.0);
        assert_eq!(state_8.min, 100.0);
        assert_eq!(state_8.max, 192.0);

        // Pushing max below min pushes min (ContrastAdjuster.java line 561)
        adjust_max(&mut state_8, 50.0);
        assert_eq!(state_8.min, 50.0);
        assert_eq!(state_8.max, 50.0);

        // 16-bit table test: 0..4095
        let mut state_16 = ContrastState::with_defaults(0.0, 4095.0);
        adjust_max(&mut state_16, 192.0);
        let expected_max_16 = 192.0 * 4095.0 / 255.0; // 3083.294117647059
        assert_eq!(state_16.min, 0.0);
        assert!((state_16.max - expected_max_16).abs() < 1e-9);
        let pos16 = slider_positions(&state_16);
        assert_eq!(pos16[0], 0);
        assert_eq!(pos16[1], 192);
        assert_eq!(pos16[2], 159);
        assert_eq!(pos16[3], 159);
    }

    #[test]
    fn table_test_adjust_brightness() {
        // ContrastAdjuster.java lines 580-583:
        // center = defaultMin + (defaultMax - defaultMin) * ((sliderRange - bvalue) / sliderRange)
        // width = max - min
        // min = center - width / 2.0
        // max = center + width / 2.0
        let mut state_8 = ContrastState::new(0.0, 255.0, 50.0, 150.0);
        adjust_brightness(&mut state_8, 96.0);
        // (256 - 96)/256 = 160/256 = 0.625
        // center = 255 * 0.625 = 159.375
        // width = 100.0 -> min = 109.375, max = 209.375
        assert_eq!(state_8.min, 109.375);
        assert_eq!(state_8.max, 209.375);

        let mut state_16 = ContrastState::new(0.0, 4095.0, 1000.0, 3000.0);
        adjust_brightness(&mut state_16, 64.0);
        // (256 - 64)/256 = 192/256 = 0.75
        // center = 4095.0 * 0.75 = 3071.25
        // width = 2000.0 -> min = 2071.25, max = 4071.25
        assert_eq!(state_16.min, 2071.25);
        assert_eq!(state_16.max, 4071.25);
    }

    #[test]
    fn table_test_adjust_contrast() {
        // ContrastAdjuster.java lines 592-604:
        // center = min + (max - min) / 2.0
        // range = defaultMax - defaultMin
        // mid = sliderRange / 2 = 128.0
        // slope = if cvalue <= mid { cvalue / mid } else { mid / (sliderRange - cvalue) }
        // if slope > 0.0 { min = center - (0.5 * range) / slope; max = center + (0.5 * range) / slope }

        // 8-bit low contrast: cvalue = 64 <= 128 -> slope = 0.5
        let mut state_8 = ContrastState::with_defaults(0.0, 255.0);
        adjust_contrast(&mut state_8, 64.0);
        assert_eq!(state_8.min, -127.5);
        assert_eq!(state_8.max, 382.5);

        // 8-bit high contrast: cvalue = 192 > 128 -> slope = 128 / (256 - 192) = 2.0
        let mut state_8h = ContrastState::with_defaults(0.0, 255.0);
        adjust_contrast(&mut state_8h, 192.0);
        assert_eq!(state_8h.min, 63.75);
        assert_eq!(state_8h.max, 191.25);

        // 16-bit contrast adjustment: cvalue = 160 > 128 -> slope = 128 / 96 = 4/3
        // center = 2000.0, range = 4095.0, (0.5 * range) / (4/3) = 1535.625
        let mut state_16 = ContrastState::new(0.0, 4095.0, 1000.0, 3000.0);
        adjust_contrast(&mut state_16, 160.0);
        assert_eq!(state_16.min, 464.375);
        assert_eq!(state_16.max, 3535.625);

        // Edge case: cvalue = 0 -> slope = 0.0 -> unchanged (ContrastAdjuster.java line 600)
        let before_min = state_16.min;
        let before_max = state_16.max;
        adjust_contrast(&mut state_16, 0.0);
        assert_eq!(state_16.min, before_min);
        assert_eq!(state_16.max, before_max);

        // Edge case: cvalue = 256 -> slope = inf -> min == max == center
        adjust_contrast(&mut state_16, 256.0);
        assert_eq!(state_16.min, 2000.0);
        assert_eq!(state_16.max, 2000.0);
    }

    #[test]
    fn table_test_reset_and_set_min_and_max() {
        // ContrastAdjuster.java line 432 (setMinAndMax) and line 620-621 (reset)
        let mut state = ContrastState::with_defaults(0.0, 255.0);
        set_min_and_max(&mut state, 40.0, 210.0);
        assert_eq!(state.min, 40.0);
        assert_eq!(state.max, 210.0);

        reset(&mut state);
        assert_eq!(state.min, 0.0);
        assert_eq!(state.max, 255.0);
    }
}
