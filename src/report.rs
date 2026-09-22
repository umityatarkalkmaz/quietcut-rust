//! Human readable rendering of an [`Analysis`].

use std::fmt::Write as _;

use crate::ffprobe::AudioStream;
use crate::interval::{self, Interval};
use crate::pipeline::{Analysis, DetectionConfig};

/// Formats a timestamp as `HH:MM:SS.mmm`.
pub fn format_timecode(seconds: f64) -> String {
    let total_ms = if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1000.0).round() as i64
    } else {
        0
    };
    let (milliseconds, total_seconds) = (total_ms % 1000, total_ms / 1000);
    let (hours, minutes, secs) = (
        total_seconds / 3600,
        (total_seconds / 60) % 60,
        total_seconds % 60,
    );
    format!("{hours:02}:{minutes:02}:{secs:02}.{milliseconds:03}")
}

/// Renders the full `analyze` report.
pub fn render_analysis(analysis: &Analysis, config: &DetectionConfig) -> String {
    let media = &analysis.media;
    let rate = media.frame_rate;
    let mut out = String::new();

    let _ = writeln!(out, "Input:      {}", media.path.display());
    let _ = writeln!(
        out,
        "Duration:   {} ({:.3} s)",
        format_timecode(media.duration),
        media.duration
    );
    let _ = writeln!(
        out,
        "Frame rate: {}/{} ({:.3} fps)",
        rate.numerator,
        rate.denominator,
        rate.fps()
    );

    let _ = writeln!(out, "\nAudio streams:");
    out.push_str(&render_stream_table(analysis));

    let _ = writeln!(out, "\nDetection:");
    let _ = writeln!(
        out,
        "  mic      {:<6} threshold {:>6} dB  ->  {} silence range(s)",
        format!("a:{}", analysis.mic.audio_index),
        config.mic_threshold_db,
        analysis.mic_silences.len()
    );
    match (&analysis.discord, &analysis.discord_silences) {
        (Some(stream), Some(silences)) => {
            let _ = writeln!(
                out,
                "  discord  {:<6} threshold {:>6} dB  ->  {} silence range(s)",
                format!("a:{}", stream.audio_index),
                config.discord_threshold_db,
                silences.len()
            );
        }
        _ => {
            let _ = writeln!(out, "  discord  not used (mic-only detection)");
        }
    }
    let _ = writeln!(
        out,
        "  combined min-silence {} s, padding {} s  ->  {} range(s)",
        config.min_silence,
        config.padding,
        analysis.silences.len()
    );

    let _ = writeln!(out, "\nSilence to cut ({}):", analysis.silences.len());
    if analysis.silences.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for (position, silence) in analysis.silences.iter().enumerate() {
        let _ = writeln!(
            out,
            "  {:>3}  {} -> {}  {:>9.3} s",
            position + 1,
            format_timecode(silence.start),
            format_timecode(silence.end),
            silence.duration()
        );
    }

    let _ = writeln!(out, "\nKeep segments ({}):", analysis.keep_segments.len());
    if analysis.keep_segments.is_empty() {
        let _ = writeln!(out, "  (none)");
    }
    for (position, segment) in analysis.keep_segments.iter().enumerate() {
        let _ = writeln!(
            out,
            "  {:>3}  {} -> {}  {:>9.3} s  frames {}..{}",
            position + 1,
            format_timecode(segment.start_seconds(rate)),
            format_timecode(segment.end_seconds(rate)),
            segment.duration_seconds(rate),
            segment.start_frame,
            segment.end_frame
        );
    }

    let kept: f64 = analysis
        .keep_segments
        .iter()
        .map(|segment| segment.duration_seconds(rate))
        .sum();
    let removed = (media.duration - kept).max(0.0);
    let ratio = if media.duration > 0.0 {
        kept / media.duration * 100.0
    } else {
        0.0
    };
    let _ = writeln!(
        out,
        "\nSummary: {:.3} s -> {:.3} s kept ({:.1}%), {:.3} s removed in {} cut(s)",
        media.duration,
        kept,
        ratio,
        removed,
        analysis.silences.len()
    );

    out
}

fn render_stream_table(analysis: &Analysis) -> String {
    let role_of = |stream: &AudioStream| -> &'static str {
        if stream.audio_index == analysis.mic.audio_index {
            "<- mic"
        } else if analysis
            .discord
            .as_ref()
            .is_some_and(|discord| discord.audio_index == stream.audio_index)
        {
            "<- discord"
        } else {
            ""
        }
    };

    let rows: Vec<(String, String, String)> = analysis
        .media
        .audio_streams
        .iter()
        .map(|stream| {
            (
                format!("a:{}", stream.audio_index),
                match &stream.title {
                    Some(title) => format!("\"{title}\""),
                    None => "(untitled)".to_string(),
                },
                format!(
                    "{} {} ch",
                    stream.codec.as_deref().unwrap_or("unknown"),
                    stream
                        .channels
                        .map_or_else(|| "?".to_string(), |count| count.to_string())
                ),
            )
        })
        .collect();

    let title_width = rows
        .iter()
        .map(|(_, title, _)| title.len())
        .max()
        .unwrap_or(0);
    let codec_width = rows
        .iter()
        .map(|(_, _, codec)| codec.len())
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    for (row, stream) in rows.iter().zip(&analysis.media.audio_streams) {
        let line = format!(
            "  {:<4} {:<title_width$}  {:<codec_width$}  {}",
            row.0,
            row.1,
            row.2,
            role_of(stream)
        );
        let _ = writeln!(out, "{}", line.trim_end());
    }
    out
}

/// Renders a compact one line summary of a silence list.
pub fn format_interval_summary(intervals: &[Interval]) -> String {
    format!(
        "{} range(s), {:.3} s total",
        intervals.len(),
        interval::sum_durations(intervals)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_timecode_renders_hours_minutes_seconds() {
        assert_eq!(format_timecode(0.0), "00:00:00.000");
        assert_eq!(format_timecode(5.15), "00:00:05.150");
        assert_eq!(format_timecode(3723.456), "01:02:03.456");
    }

    #[test]
    fn format_timecode_clamps_invalid_input() {
        assert_eq!(format_timecode(-1.0), "00:00:00.000");
        assert_eq!(format_timecode(f64::NAN), "00:00:00.000");
    }

    #[test]
    fn format_interval_summary_reports_count_and_total() {
        let intervals = [Interval::new(0.0, 1.5), Interval::new(3.0, 4.0)];
        assert_eq!(
            format_interval_summary(&intervals),
            "2 range(s), 2.500 s total"
        );
    }
}
