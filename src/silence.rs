//! Silence detection through the ffmpeg `silencedetect` filter.

use std::path::Path;

use crate::error::{Error, Result};
use crate::exec::{self, FFMPEG};
use crate::interval::Interval;

/// Runs `silencedetect` on a single audio stream and returns its silent ranges.
///
/// `audio_index` is audio relative (`a:N`), never an OBS track number.
pub fn detect_silence(
    path: &Path,
    audio_index: usize,
    threshold_db: f64,
    min_duration: f64,
    media_duration: f64,
) -> Result<Vec<Interval>> {
    // Both values are validated numbers, never user supplied text, so nothing
    // can be injected into the filter graph description.
    let filter = format!("silencedetect=noise={threshold_db}dB:d={min_duration}");
    let map = format!("0:a:{audio_index}");

    let output = exec::run_tool(
        FFMPEG,
        [
            "-hide_banner".as_ref(),
            "-nostdin".as_ref(),
            "-nostats".as_ref(),
            "-loglevel".as_ref(),
            "info".as_ref(),
            "-i".as_ref(),
            path.as_os_str(),
            "-map".as_ref(),
            map.as_ref(),
            "-af".as_ref(),
            filter.as_ref(),
            "-f".as_ref(),
            "null".as_ref(),
            "-".as_ref(),
        ],
    )?;

    let log = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(Error::ToolFailed {
            tool: FFMPEG,
            status: output.status.to_string(),
            stderr: log.into_owned(),
        });
    }

    Ok(parse_silence_log(&log, media_duration))
}

/// Extracts `silence_start` / `silence_end` pairs from an ffmpeg log.
///
/// A trailing `silence_start` without a matching end closes at `media_duration`.
pub fn parse_silence_log(log: &str, media_duration: f64) -> Vec<Interval> {
    let mut intervals = Vec::new();
    let mut pending_start: Option<f64> = None;

    for line in log.lines() {
        if let Some(start) = parse_value_after(line, "silence_start:") {
            // An unclosed start means ffmpeg restarted the filter; keep the
            // earliest boundary rather than silently moving the cut point.
            pending_start.get_or_insert(start.max(0.0));
            continue;
        }
        if let Some(end) = parse_value_after(line, "silence_end:")
            && let Some(start) = pending_start.take()
        {
            push_interval(&mut intervals, start, end, media_duration);
        }
    }

    if let Some(start) = pending_start {
        push_interval(&mut intervals, start, media_duration, media_duration);
    }

    intervals
}

fn push_interval(intervals: &mut Vec<Interval>, start: f64, end: f64, media_duration: f64) {
    let interval = Interval::new(start.max(0.0), end.min(media_duration));
    if !interval.is_empty() {
        intervals.push(interval);
    }
}

/// Reads the first whitespace separated number following `key` on a log line.
fn parse_value_after(line: &str, key: &str) -> Option<f64> {
    let position = line.find(key)?;
    let value: f64 = line[position + key.len()..]
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
[silencedetect @ 0x5620d0] silence_start: 5.00002
[silencedetect @ 0x5620d0] silence_end: 12.0001 | silence_duration: 7.00008
[silencedetect @ 0x5620d0] silence_start: 15.25
[silencedetect @ 0x5620d0] silence_end: 16.75 | silence_duration: 1.5
size=N/A time=00:00:20.00 bitrate=N/A speed=  42x
";

    fn pairs(intervals: &[Interval]) -> Vec<(f64, f64)> {
        intervals.iter().map(|iv| (iv.start, iv.end)).collect()
    }

    #[test]
    fn parse_silence_log_reads_complete_pairs() {
        assert_eq!(
            pairs(&parse_silence_log(LOG, 20.0)),
            [(5.00002, 12.0001), (15.25, 16.75)]
        );
    }

    #[test]
    fn parse_silence_log_closes_trailing_start_at_duration() {
        let log = "[silencedetect @ 0x1] silence_start: 18.4\n";
        assert_eq!(pairs(&parse_silence_log(log, 20.0)), [(18.4, 20.0)]);
    }

    #[test]
    fn parse_silence_log_clamps_boundaries_to_media() {
        let log = "[silencedetect @ 0x1] silence_start: -0.001\n\
                   [silencedetect @ 0x1] silence_end: 25.0 | silence_duration: 25.0\n";
        assert_eq!(pairs(&parse_silence_log(log, 20.0)), [(0.0, 20.0)]);
    }

    #[test]
    fn parse_silence_log_ignores_unmatched_end() {
        let log = "[silencedetect @ 0x1] silence_end: 3.0 | silence_duration: 3.0\n\
                   [silencedetect @ 0x1] silence_start: 5.0\n\
                   [silencedetect @ 0x1] silence_end: 6.0 | silence_duration: 1.0\n";
        assert_eq!(pairs(&parse_silence_log(log, 20.0)), [(5.0, 6.0)]);
    }

    #[test]
    fn parse_silence_log_keeps_earliest_of_repeated_starts() {
        let log = "[silencedetect @ 0x1] silence_start: 5.0\n\
                   [silencedetect @ 0x1] silence_start: 7.0\n\
                   [silencedetect @ 0x1] silence_end: 9.0 | silence_duration: 4.0\n";
        assert_eq!(pairs(&parse_silence_log(log, 20.0)), [(5.0, 9.0)]);
    }

    #[test]
    fn parse_silence_log_drops_zero_length_ranges() {
        let log = "[silencedetect @ 0x1] silence_start: 5.0\n\
                   [silencedetect @ 0x1] silence_end: 5.0 | silence_duration: 0\n";
        assert!(parse_silence_log(log, 20.0).is_empty());
    }

    #[test]
    fn parse_silence_log_ignores_unrelated_output() {
        let log = "Input #0, matroska,webm, from 'a.mkv':\n  Duration: 00:00:20.00\n";
        assert!(parse_silence_log(log, 20.0).is_empty());
    }

    #[test]
    fn parse_silence_log_ignores_malformed_values() {
        let log = "[silencedetect @ 0x1] silence_start: NaN\n\
                   [silencedetect @ 0x1] silence_start: abc\n";
        assert!(parse_silence_log(log, 20.0).is_empty());
    }
}
