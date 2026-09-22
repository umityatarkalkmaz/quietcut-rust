//! Frame grid handling: rational frame rates and snapping of cut points.

use crate::interval::Interval;

/// A rational frame rate such as `30000/1001`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRate {
    pub numerator: u32,
    pub denominator: u32,
}

/// A keep segment expressed in whole frames, half-open as `[start, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSegment {
    pub start_frame: i64,
    pub end_frame: i64,
}

impl FrameRate {
    pub fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            return None;
        }
        Some(Self {
            numerator,
            denominator,
        })
    }

    pub fn fps(&self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }

    /// Rounds a timestamp to the nearest frame boundary.
    ///
    /// Rounding to nearest is monotonic, so snapped cut points keep their
    /// original order and adjacent segments can never start overlapping.
    pub fn snap_to_frame(&self, seconds: f64) -> i64 {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 0;
        }
        (seconds * self.fps()).round() as i64
    }

    pub fn frame_to_seconds(&self, frames: i64) -> f64 {
        frames as f64 / self.fps()
    }
}

impl FrameSegment {
    pub fn frame_count(&self) -> i64 {
        (self.end_frame - self.start_frame).max(0)
    }

    pub fn start_seconds(&self, rate: FrameRate) -> f64 {
        rate.frame_to_seconds(self.start_frame)
    }

    pub fn end_seconds(&self, rate: FrameRate) -> f64 {
        rate.frame_to_seconds(self.end_frame)
    }

    pub fn duration_seconds(&self, rate: FrameRate) -> f64 {
        rate.frame_to_seconds(self.frame_count())
    }
}

/// Parses an ffprobe rate field such as `30000/1001`, `25/1` or `25`.
pub fn parse_frame_rate(text: &str) -> Option<FrameRate> {
    let text = text.trim();
    match text.split_once('/') {
        Some((numerator, denominator)) => FrameRate::new(
            numerator.trim().parse().ok()?,
            denominator.trim().parse().ok()?,
        ),
        None => FrameRate::new(text.parse().ok()?, 1),
    }
}

/// Snaps every interval onto the frame grid, dropping sub-frame leftovers.
pub fn snap_intervals_to_frames(intervals: &[Interval], rate: FrameRate) -> Vec<FrameSegment> {
    intervals
        .iter()
        .map(|interval| FrameSegment {
            start_frame: rate.snap_to_frame(interval.start),
            end_frame: rate.snap_to_frame(interval.end),
        })
        .filter(|segment| segment.frame_count() > 0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NTSC: FrameRate = FrameRate {
        numerator: 30000,
        denominator: 1001,
    };
    const PAL: FrameRate = FrameRate {
        numerator: 25,
        denominator: 1,
    };

    #[test]
    fn parse_frame_rate_reads_rational_values() {
        assert_eq!(parse_frame_rate("30000/1001"), Some(NTSC));
        assert_eq!(parse_frame_rate(" 25/1 "), Some(PAL));
        assert_eq!(parse_frame_rate("25"), Some(PAL));
    }

    #[test]
    fn parse_frame_rate_rejects_degenerate_values() {
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("30/0"), None);
        assert_eq!(parse_frame_rate("0/1"), None);
        assert_eq!(parse_frame_rate("N/A"), None);
        assert_eq!(parse_frame_rate(""), None);
        assert_eq!(parse_frame_rate("-30/1"), None);
    }

    #[test]
    fn snap_to_frame_rounds_to_nearest_boundary() {
        assert_eq!(PAL.snap_to_frame(1.0), 25);
        assert_eq!(PAL.snap_to_frame(1.019), 25);
        assert_eq!(PAL.snap_to_frame(1.021), 26);
        assert_eq!(NTSC.snap_to_frame(1.0), 30);
    }

    #[test]
    fn snap_to_frame_clamps_negative_and_invalid_input() {
        assert_eq!(PAL.snap_to_frame(-5.0), 0);
        assert_eq!(PAL.snap_to_frame(f64::NAN), 0);
        assert_eq!(PAL.snap_to_frame(f64::INFINITY), 0);
    }

    #[test]
    fn snap_intervals_to_frames_preserves_order_without_overlap() {
        let intervals = [Interval::new(0.0, 5.13), Interval::new(7.87, 20.0)];
        let segments = snap_intervals_to_frames(&intervals, PAL);
        assert_eq!(
            segments,
            [
                FrameSegment {
                    start_frame: 0,
                    end_frame: 128
                },
                FrameSegment {
                    start_frame: 197,
                    end_frame: 500
                }
            ]
        );
        assert!(segments[0].end_frame <= segments[1].start_frame);
    }

    #[test]
    fn snap_intervals_to_frames_drops_sub_frame_segments() {
        let intervals = [Interval::new(1.0, 1.005), Interval::new(2.0, 3.0)];
        let segments = snap_intervals_to_frames(&intervals, PAL);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].frame_count(), 25);
    }

    #[test]
    fn frame_segment_converts_back_to_seconds() {
        let segment = FrameSegment {
            start_frame: 50,
            end_frame: 100,
        };
        assert!((segment.start_seconds(PAL) - 2.0).abs() < 1e-9);
        assert!((segment.end_seconds(PAL) - 4.0).abs() < 1e-9);
        assert!((segment.duration_seconds(PAL) - 2.0).abs() < 1e-9);
    }
}
