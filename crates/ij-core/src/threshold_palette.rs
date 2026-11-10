use crate::ThresholdLevels;
use serde::{Deserialize, Serialize};

/// ImageJ 1.54u `ThresholdAdjuster` default constants.
/// `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` line 29
pub const DEFAULT_MIN_THRESHOLD: f64 = 0.0;
/// `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` line 30
pub const DEFAULT_MAX_THRESHOLD: f64 = 255.0;

/// `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` line 46
pub const SLIDER_RANGE: f64 = 256.0;
pub const THRESHOLD_SLIDER_RANGE: f64 = SLIDER_RANGE;

/// Default min slider position = sliderRange / 3 = 256 / 3 = 85.
/// `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` line 128
pub const DEFAULT_MIN_SLIDER: f64 = 85.0;

/// Default max slider position = sliderRange * 2 / 3 = 512 / 3 = 170.
/// `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` line 158 and line 1045
pub const DEFAULT_MAX_SLIDER: f64 = 170.0;

/// Value used by ImageJ for no threshold in the histogram plot.
/// `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` line 596
pub const NO_THRESHOLD_PLOT_BIN: i32 = -1;

/// -1e30 limit used for 32-bit float threshold lower bound.
/// `original_imagej/ij/process/ImageProcessor.java` line 760
pub const FLOAT_NEG_LIMIT: f64 = -1e30;

/// 1e30 limit used for 32-bit float threshold upper bound.
/// `original_imagej/ij/process/ImageProcessor.java` line 767
pub const FLOAT_POS_LIMIT: f64 = 1e30;

/// Represents slider positions in the 0..=255 coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ThresholdSliders {
    pub min_slider: f64,
    pub max_slider: f64,
}

impl Default for ThresholdSliders {
    fn default() -> Self {
        Self {
            min_slider: DEFAULT_MIN_SLIDER,
            max_slider: DEFAULT_MAX_SLIDER,
        }
    }
}

impl ThresholdSliders {
    pub fn new(min_slider: f64, max_slider: f64) -> Self {
        Self {
            min_slider,
            max_slider,
        }
    }
}

/// Scales threshold levels in the slider range 0-255 to actual pixel values.
/// Parity reference: `original_imagej/ij/process/ImageProcessor.java` lines 752-774
/// and `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` line 563 (`scaleUpAndSet`).
pub fn scale_up_and_set(
    bit_depth: u8,
    display_min: f64,
    display_max: f64,
    lower_slider: f64,
    upper_slider: f64,
) -> ThresholdLevels {
    if bit_depth == 8 {
        // ImageProcessor.java lines 753-754: bitDepth==8 thresholds are unscaled.
        // ByteProcessor.java lines 476-483: clamped to [0.0, 255.0].
        return ThresholdLevels {
            lower: lower_slider.clamp(0.0, 255.0),
            upper: upper_slider.clamp(0.0, 255.0),
        };
    }

    if display_max > display_min {
        let lower = if lower_slider == 0.0 {
            if bit_depth == 32 {
                // ImageProcessor.java line 760: Math.min(min, -1e30)
                display_min.min(FLOAT_NEG_LIMIT)
            } else {
                // ImageProcessor.java lines 758-762: leaves lower unchanged at 0.0 for 16-bit
                0.0
            }
        } else {
            display_min + (lower_slider / 255.0) * (display_max - display_min)
        };

        let upper = if upper_slider == 255.0 {
            if bit_depth == 16 {
                // ImageProcessor.java line 765: full-range limit of 16-bit
                65535.0
            } else if bit_depth == 32 {
                // ImageProcessor.java line 767: Math.max(max, 1e30)
                display_max.max(FLOAT_POS_LIMIT)
            } else {
                display_min + (upper_slider / 255.0) * (display_max - display_min)
            }
        } else {
            display_min + (upper_slider / 255.0) * (display_max - display_min)
        };

        ThresholdLevels { lower, upper }
    } else {
        // ImageProcessor.java line 771: lower = upper = min;
        ThresholdLevels {
            lower: display_min,
            upper: display_min,
        }
    }
}

/// Scales an actual threshold level down to the 0-255 slider range.
/// Parity reference: `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` lines 568-580 (`scaleDown`).
pub fn scale_down(
    bit_depth: u8,
    display_min: f64,
    display_max: f64,
    threshold: f64,
) -> Option<f64> {
    if bit_depth == 8 {
        return Some(threshold.clamp(0.0, 255.0));
    }
    if display_max > display_min {
        let scaled = ((threshold - display_min) / (display_max - display_min)) * 255.0;
        Some(scaled.clamp(0.0, 255.0))
    } else {
        None
    }
}

/// Scales a slider position in 0-255 to the actual pixel value without full-range snaps.
/// Parity reference: `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` lines 583-590 (`scaleUp`).
pub fn scale_up(
    _bit_depth: u8,
    display_min: f64,
    display_max: f64,
    slider_val: f64,
) -> Option<f64> {
    if display_max > display_min {
        Some(display_min + (slider_val / 255.0) * (display_max - display_min))
    } else {
        None
    }
}

/// Converts active `ThresholdLevels` (or `None`) into slider coordinates.
/// Parity reference: `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` line 515-516 and line 466-467.
pub fn threshold_to_sliders(
    levels: Option<ThresholdLevels>,
    bit_depth: u8,
    display_min: f64,
    display_max: f64,
) -> ThresholdSliders {
    match levels {
        Some(levels) => {
            let min_s = scale_down(bit_depth, display_min, display_max, levels.lower)
                .unwrap_or(DEFAULT_MIN_SLIDER);
            let max_s = scale_down(bit_depth, display_min, display_max, levels.upper)
                .unwrap_or(DEFAULT_MAX_SLIDER);
            ThresholdSliders {
                min_slider: min_s,
                max_slider: max_s,
            }
        }
        None => ThresholdSliders::default(),
    }
}

/// Updates min slider value and enforces slider constraints.
/// When `scroll_together` is true (Shift or Alt key pressed in ImageJ), width is preserved.
/// Parity reference: `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` lines 693-714 (`adjustMinThreshold`).
pub fn adjust_min_threshold(
    current: ThresholdSliders,
    new_min: f64,
    scroll_together: bool,
) -> ThresholdSliders {
    let new_min = new_min.clamp(0.0, 255.0);
    if scroll_together {
        let mut width = current.max_slider - current.min_slider;
        if width < 1.0 {
            width = 1.0;
        }
        let mut min = new_min;
        let mut max = min + width;
        if min + width > 255.0 {
            min = 255.0 - width;
            max = min + width;
        }
        ThresholdSliders {
            min_slider: min.clamp(0.0, 255.0),
            max_slider: max.clamp(0.0, 255.0),
        }
    } else {
        let min = new_min;
        let max = if current.max_slider < min {
            min
        } else {
            current.max_slider
        };
        ThresholdSliders {
            min_slider: min,
            max_slider: max,
        }
    }
}

/// Updates max slider value and enforces slider constraints.
/// Parity reference: `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` lines 716-729 (`adjustMaxThreshold`).
pub fn adjust_max_threshold(
    current: ThresholdSliders,
    new_max: f64,
) -> ThresholdSliders {
    let new_max = new_max.clamp(0.0, 255.0);
    let max = new_max;
    let mut min = current.min_slider;
    if min > max {
        min = max;
    }
    if min < 0.0 {
        min = 0.0;
    }
    ThresholdSliders {
        min_slider: min,
        max_slider: max,
    }
}

/// Computes the histogram bins corresponding to the current sliders.
/// Parity reference: `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` lines 592-604 (`updatePlot`)
/// and lines 1214-1217 (`setThreshold`).
pub fn plot_threshold_bins(
    has_threshold: bool,
    min_slider: f64,
    max_slider: f64,
) -> (Option<usize>, usize) {
    let max = (max_slider.round() as i32).clamp(0, 255) as usize;
    if !has_threshold {
        (None, max)
    } else {
        let min = (min_slider.round() as i32).clamp(0, 255) as usize;
        (Some(min), max)
    }
}

/// Formats a threshold number for min/max labels matching ImageJ 1.54u.
/// Parity reference: `original_imagej/ij/plugin/frame/ThresholdAdjuster.java` lines 646-679 (`updateLabels` & `d2s`).
pub fn format_threshold_label(value: f64, is_signed_16bit_or_uncalibrated: bool) -> String {
    if value <= FLOAT_NEG_LIMIT {
        "-1e30".to_string()
    } else if value >= FLOAT_POS_LIMIT {
        "1e30".to_string()
    } else if (value == (value as i64) as f64 && value.abs() < 1e6) || is_signed_16bit_or_uncalibrated {
        format!("{:.0}", value.round())
    } else {
        let abs_x = value.abs();
        let mut digits = if abs_x < 10.0 {
            4
        } else if abs_x < 100.0 {
            3
        } else {
            2
        };
        if value < 0.0 && digits == 4 {
            digits = 3;
        }
        format!("{:.prec$}", value, prec = digits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_test_scale_up_and_set_8bit() {
        // ImageProcessor.java lines 753-754: 8-bit images do not scale with display range.
        // ByteProcessor.java lines 476-483: clamped to [0.0, 255.0].
        let t1 = scale_up_and_set(8, 0.0, 255.0, 0.0, 255.0);
        assert_eq!(t1.lower, 0.0);
        assert_eq!(t1.upper, 255.0);

        let t2 = scale_up_and_set(8, 50.0, 200.0, 85.0, 170.0);
        assert_eq!(t2.lower, 85.0);
        assert_eq!(t2.upper, 170.0);

        let t3 = scale_up_and_set(8, 0.0, 255.0, -10.0, 300.0);
        assert_eq!(t3.lower, 0.0);
        assert_eq!(t3.upper, 255.0);
    }

    #[test]
    fn table_test_scale_up_and_set_16bit() {
        // ImageProcessor.java lines 752-774:
        // lower == 0.0 -> lower = 0.0 (full lower range limit for 16-bit)
        // upper == 255.0 -> upper = 65535.0 (full upper range limit for 16-bit)
        // otherwise lower = min + (lower/255.0)*(max - min), upper = min + (upper/255.0)*(max - min)
        // Display range: [1000.0, 5000.0] -> range = 4000.0
        let t1 = scale_up_and_set(16, 1000.0, 5000.0, 0.0, 255.0);
        assert_eq!(t1.lower, 0.0);
        assert_eq!(t1.upper, 65535.0);

        // lower = 127.5 -> 1000 + 0.5 * 4000 = 3000.0
        // upper = 255.0 -> snaps to 65535.0
        let t2 = scale_up_and_set(16, 1000.0, 5000.0, 127.5, 255.0);
        assert_eq!(t2.lower, 3000.0);
        assert_eq!(t2.upper, 65535.0);

        // lower = 51.0 (0.2) -> 1000 + 0.2 * 4000 = 1800.0
        // upper = 204.0 (0.8) -> 1000 + 0.8 * 4000 = 4200.0
        let t3 = scale_up_and_set(16, 1000.0, 5000.0, 51.0, 204.0);
        assert_eq!(t3.lower, 1800.0);
        assert_eq!(t3.upper, 4200.0);

        // Degenerate max <= min (ImageProcessor.java line 771: lower = upper = min)
        let t4 = scale_up_and_set(16, 2000.0, 2000.0, 50.0, 200.0);
        assert_eq!(t4.lower, 2000.0);
        assert_eq!(t4.upper, 2000.0);
    }

    #[test]
    fn table_test_scale_up_and_set_32bit() {
        // ImageProcessor.java lines 758-767:
        // lower == 0.0 -> Math.min(min, -1e30)
        // upper == 255.0 -> Math.max(max, 1e30)
        // Display range: [0.0, 10.0] -> range = 10.0
        let t1 = scale_up_and_set(32, 0.0, 10.0, 0.0, 255.0);
        assert_eq!(t1.lower, -1e30);
        assert_eq!(t1.upper, 1e30);

        // lower = 51.0 (0.2) -> 0.0 + 0.2 * 10.0 = 2.0
        // upper = 204.0 (0.8) -> 0.0 + 0.8 * 10.0 = 8.0
        let t2 = scale_up_and_set(32, 0.0, 10.0, 51.0, 204.0);
        assert_eq!(t2.lower, 2.0);
        assert_eq!(t2.upper, 8.0);

        // Negative display min: [-2e30, 5.0]
        // lower == 0.0 -> Math.min(-2e30, -1e30) = -2e30
        let t3 = scale_up_and_set(32, -2e30, 5.0, 0.0, 127.5);
        assert_eq!(t3.lower, -2e30);

        // Degenerate max <= min: [5.0, 5.0] -> lower = upper = 5.0
        let t4 = scale_up_and_set(32, 5.0, 5.0, 50.0, 200.0);
        assert_eq!(t4.lower, 5.0);
        assert_eq!(t4.upper, 5.0);
    }

    #[test]
    fn table_test_scale_down_and_scale_up() {
        // ThresholdAdjuster.java lines 568-580 (scaleDown):
        // 8-bit: threshold directly
        assert_eq!(scale_down(8, 0.0, 255.0, 128.0), Some(128.0));

        // 16-bit: ((threshold - min)/(max - min)) * 255.0, clamped [0, 255]
        // range [1000.0, 5000.0]
        assert_eq!(scale_down(16, 1000.0, 5000.0, 0.0), Some(0.0));
        assert_eq!(scale_down(16, 1000.0, 5000.0, 1000.0), Some(0.0));
        assert_eq!(scale_down(16, 1000.0, 5000.0, 3000.0), Some(127.5));
        assert_eq!(scale_down(16, 1000.0, 5000.0, 4200.0), Some(204.0));
        assert_eq!(scale_down(16, 1000.0, 5000.0, 65535.0), Some(255.0));

        // 32-bit:
        assert_eq!(scale_down(32, 0.0, 10.0, -1e30), Some(0.0));
        assert_eq!(scale_down(32, 0.0, 10.0, 2.0), Some(51.0));
        assert_eq!(scale_down(32, 0.0, 10.0, 8.0), Some(204.0));
        assert_eq!(scale_down(32, 0.0, 10.0, 1e30), Some(255.0));

        // Degenerate max <= min returns None (NO_THRESHOLD in Java)
        assert_eq!(scale_down(16, 1000.0, 1000.0, 500.0), None);

        // ThresholdAdjuster.java lines 583-590 (scaleUp):
        // Does not snap 0 / 255 to full range limits
        assert_eq!(scale_up(8, 0.0, 255.0, 128.0), Some(128.0));
        // ThresholdAdjuster.java lines 583-590 also scale ByteProcessor values
        // against a non-default B&C display range.
        assert_eq!(scale_up(8, 20.0, 200.0, 0.0), Some(20.0));
        assert_eq!(scale_up(8, 20.0, 200.0, 255.0), Some(200.0));
        assert_eq!(scale_up(16, 1000.0, 5000.0, 0.0), Some(1000.0));
        assert_eq!(scale_up(16, 1000.0, 5000.0, 255.0), Some(5000.0));
        assert_eq!(scale_up(32, 0.0, 10.0, 0.0), Some(0.0));
        assert_eq!(scale_up(32, 0.0, 10.0, 255.0), Some(10.0));
        assert_eq!(scale_up(16, 1000.0, 1000.0, 128.0), None);
    }

    #[test]
    fn table_test_adjust_sliders() {
        let current = ThresholdSliders::new(85.0, 170.0);

        // Normal adjust_min (ThresholdAdjuster.java lines 708-713)
        let s1 = adjust_min_threshold(current, 100.0, false);
        assert_eq!(s1.min_slider, 100.0);
        assert_eq!(s1.max_slider, 170.0);

        // Min pushed past max (line 709: if (maxThreshold < minThreshold) maxThreshold = minThreshold)
        let s2 = adjust_min_threshold(current, 200.0, false);
        assert_eq!(s2.min_slider, 200.0);
        assert_eq!(s2.max_slider, 200.0);

        // Linked/scroll_together adjust_min (Shift/Alt key, lines 694-706)
        // width = 170 - 85 = 85.0
        // min = 100.0 -> max = 100.0 + 85.0 = 185.0
        let s3 = adjust_min_threshold(current, 100.0, true);
        assert_eq!(s3.min_slider, 100.0);
        assert_eq!(s3.max_slider, 185.0);

        // Linked hitting upper ceiling (lines 699-703: if (minThreshold+width)>255)
        // new_min = 200.0, width = 85.0 -> 285 > 255
        // min = 255 - 85 = 170.0, max = 170 + 85 = 255.0
        let s4 = adjust_min_threshold(current, 200.0, true);
        assert_eq!(s4.min_slider, 170.0);
        assert_eq!(s4.max_slider, 255.0);

        // Normal adjust_max (ThresholdAdjuster.java lines 716-729)
        let s5 = adjust_max_threshold(current, 150.0);
        assert_eq!(s5.min_slider, 85.0);
        assert_eq!(s5.max_slider, 150.0);

        // Max pulled below min (lines 718-720: if (minThreshold > maxThreshold) minThreshold = maxThreshold)
        let s6 = adjust_max_threshold(current, 50.0);
        assert_eq!(s6.min_slider, 50.0);
        assert_eq!(s6.max_slider, 50.0);

        // Negative minThreshold removal (lines 722-724: if (minThreshold < 0) minThreshold = 0)
        let negative_min = ThresholdSliders::new(-1.0, 170.0);
        let s7 = adjust_max_threshold(negative_min, 150.0);
        assert_eq!(s7.min_slider, 0.0);
        assert_eq!(s7.max_slider, 150.0);
    }

    #[test]
    fn table_test_plot_threshold_bins() {
        // ThresholdAdjuster.java lines 592-604 & 1214-1217:
        // When no threshold active, min bin is NO_THRESHOLD_PLOT_BIN (-1 in Java, None in Option)
        let (p_min_none, p_max_none) = plot_threshold_bins(false, 85.2, 170.1);
        assert_eq!(p_min_none, None);
        assert_eq!(p_max_none, 170);

        let (p_min_some, p_max_some) = plot_threshold_bins(true, 85.2, 170.1);
        assert_eq!(p_min_some, Some(85));
        assert_eq!(p_max_some, 170);
    }

    #[test]
    fn table_test_formatting_and_defaults() {
        // ThresholdAdjuster.java lines 661-668 & 673-679:
        assert_eq!(format_threshold_label(-1e30, false), "-1e30");
        assert_eq!(format_threshold_label(1e30, false), "1e30");
        assert_eq!(format_threshold_label(100.0, false), "100");
        assert_eq!(format_threshold_label(42.0, true), "42");
        // ResultsTable.java line 962 sets HALF_UP for zero-decimal formatting.
        assert_eq!(format_threshold_label(2.5, true), "3");
        assert_eq!(format_threshold_label(3.14159, false), "3.1416");
        assert_eq!(format_threshold_label(-3.14159, false), "-3.142");
        assert_eq!(format_threshold_label(42.5678, false), "42.568");
        assert_eq!(format_threshold_label(123.456, false), "123.46");

        // Defaults
        assert_eq!(DEFAULT_MIN_THRESHOLD, 0.0);
        assert_eq!(DEFAULT_MAX_THRESHOLD, 255.0);
        assert_eq!(DEFAULT_MIN_SLIDER, 85.0);
        assert_eq!(DEFAULT_MAX_SLIDER, 170.0);
    }

    #[test]
    fn table_test_threshold_to_sliders() {
        // 8-bit
        let s_none = threshold_to_sliders(None, 8, 0.0, 255.0);
        assert_eq!(s_none, ThresholdSliders::default());

        let s_8 = threshold_to_sliders(Some(ThresholdLevels { lower: 50.0, upper: 180.0 }), 8, 0.0, 255.0);
        assert_eq!(s_8.min_slider, 50.0);
        assert_eq!(s_8.max_slider, 180.0);

        // 16-bit
        let s_16 = threshold_to_sliders(
            Some(ThresholdLevels { lower: 3000.0, upper: 65535.0 }),
            16,
            1000.0,
            5000.0,
        );
        assert_eq!(s_16.min_slider, 127.5);
        assert_eq!(s_16.max_slider, 255.0);
    }
}
