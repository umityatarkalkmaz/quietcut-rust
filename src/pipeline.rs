//! The detection pipeline: probe, detect, combine, snap.

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::ffprobe::{self, AudioStream, MediaInfo};
use crate::frame::{FrameRate, FrameSegment, snap_intervals_to_frames};
use crate::interval::{self, Interval};
use crate::silence;
use crate::streams::{self, StreamSelector};

/// Validated detection settings.
#[derive(Clone, Debug)]
pub struct DetectionConfig {
    pub input: PathBuf,
    pub mic: StreamSelector,
    pub discord: StreamSelector,
    pub mic_threshold_db: f64,
    pub discord_threshold_db: f64,
    pub min_silence: f64,
    pub padding: f64,
}

/// Result of analysing one recording.
#[derive(Clone, Debug)]
pub struct Analysis {
    pub media: MediaInfo,
    pub mic: AudioStream,
    pub discord: Option<AudioStream>,
    pub mic_silences: Vec<Interval>,
    pub discord_silences: Option<Vec<Interval>>,
    pub silences: Vec<Interval>,
    pub keep_segments: Vec<FrameSegment>,
    pub warnings: Vec<String>,
}

/// Probes the input, detects silence on the selected streams and derives the
/// frame aligned keep segments.
pub fn analyze_media(config: &DetectionConfig) -> Result<Analysis> {
    let media = ffprobe::probe_media(&config.input)?;
    let mut warnings = Vec::new();

    if media.video_stream_count > 1 {
        warnings.push(format!(
            "file has {} video streams; the frame grid comes from the first one",
            media.video_stream_count
        ));
    }

    let mic = resolve_mic_stream(&media, config)?;
    let discord = resolve_discord_stream(&media, config, &mut warnings)?;

    if let Some(discord) = &discord
        && discord.audio_index == mic.audio_index
    {
        return Err(Error::StreamSelectionCollision {
            index: mic.audio_index,
        });
    }

    let mic_silences = silence::detect_silence(
        &media.path,
        mic.audio_index,
        config.mic_threshold_db,
        // Anything shorter than min_silence can never survive the padding
        // step, so the filter may drop it during detection already.
        config.min_silence,
        media.duration,
    )?;

    let discord_silences = match &discord {
        Some(stream) => Some(silence::detect_silence(
            &media.path,
            stream.audio_index,
            config.discord_threshold_db,
            config.min_silence,
            media.duration,
        )?),
        None => None,
    };

    let silences = combine_silences(
        &mic_silences,
        discord_silences.as_deref(),
        config.padding,
        config.min_silence,
    );
    let keep_segments = build_keep_segments(&silences, media.duration, media.frame_rate);

    if silences.is_empty() {
        warnings.push(
            "no silence matched the current thresholds; the timeline would keep everything"
                .to_string(),
        );
    }

    Ok(Analysis {
        media,
        mic,
        discord,
        mic_silences,
        discord_silences,
        silences,
        keep_segments,
        warnings,
    })
}

/// Combines per-stream silences into the ranges that are safe to cut.
///
/// Discord is optional: without it, detection falls back to the mic alone.
pub fn combine_silences(
    mic_silences: &[Interval],
    discord_silences: Option<&[Interval]>,
    padding: f64,
    min_silence: f64,
) -> Vec<Interval> {
    let combined = match discord_silences {
        Some(discord) => interval::intersect_intervals(mic_silences, discord),
        None => interval::merge_intervals(mic_silences),
    };
    let shrunk = interval::shrink_intervals(&combined, padding);
    interval::filter_intervals(&shrunk, min_silence)
}

/// Turns silences into frame aligned keep segments inside `[0, duration]`.
pub fn build_keep_segments(
    silences: &[Interval],
    duration: f64,
    frame_rate: FrameRate,
) -> Vec<FrameSegment> {
    let span = Interval::new(0.0, duration);
    let keep = interval::complement_intervals(silences, span);
    snap_intervals_to_frames(&keep, frame_rate)
}

fn resolve_mic_stream(media: &MediaInfo, config: &DetectionConfig) -> Result<AudioStream> {
    let resolved = streams::resolve_stream(&media.audio_streams, &config.mic, "mic")?;
    match resolved {
        Some(found) => Ok(found.stream.clone()),
        None => Err(Error::StreamNameNotFound {
            role: "mic",
            name: selector_name(&config.mic),
            available: streams::format_available_titles(&media.audio_streams),
        }),
    }
}

fn resolve_discord_stream(
    media: &MediaInfo,
    config: &DetectionConfig,
    warnings: &mut Vec<String>,
) -> Result<Option<AudioStream>> {
    let resolved = streams::resolve_stream(&media.audio_streams, &config.discord, "discord")?;
    let Some(found) = resolved else {
        warnings.push(format!(
            "no audio stream titled \"{}\"; detecting on the mic stream only",
            selector_name(&config.discord)
        ));
        return Ok(None);
    };

    if found.match_count > 1 {
        warnings.push(format!(
            "{} audio streams are titled \"{}\"; using {}",
            found.match_count,
            selector_name(&config.discord),
            found.stream.label()
        ));
    }
    Ok(Some(found.stream.clone()))
}

fn selector_name(selector: &StreamSelector) -> String {
    match selector {
        StreamSelector::ByName(name) => name.clone(),
        StreamSelector::ByIndex(index) => format!("a:{index}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intervals(pairs: &[(f64, f64)]) -> Vec<Interval> {
        pairs.iter().map(|&(s, e)| Interval::new(s, e)).collect()
    }

    fn pairs(intervals: &[Interval]) -> Vec<(f64, f64)> {
        intervals.iter().map(|iv| (iv.start, iv.end)).collect()
    }

    #[test]
    fn combine_silences_uses_the_intersection_of_both_streams() {
        let mic = intervals(&[(5.0, 12.0)]);
        let discord = intervals(&[(3.0, 8.0)]);
        let combined = combine_silences(&mic, Some(&discord), 0.15, 0.6);
        assert_eq!(pairs(&combined), [(5.15, 7.85)]);
    }

    #[test]
    fn combine_silences_drops_ranges_below_the_minimum() {
        let mic = intervals(&[(5.0, 12.0)]);
        let discord = intervals(&[(4.0, 5.8)]);
        assert!(combine_silences(&mic, Some(&discord), 0.15, 0.6).is_empty());
    }

    #[test]
    fn combine_silences_falls_back_to_the_mic_alone() {
        let mic = intervals(&[(5.0, 12.0), (11.0, 13.0)]);
        let combined = combine_silences(&mic, None, 0.15, 0.6);
        assert_eq!(pairs(&combined), [(5.15, 12.85)]);
    }

    #[test]
    fn combine_silences_without_mic_silence_is_empty() {
        assert!(combine_silences(&[], Some(&intervals(&[(0.0, 20.0)])), 0.15, 0.6).is_empty());
    }

    #[test]
    fn build_keep_segments_snaps_the_complement_to_frames() {
        let rate = FrameRate::new(25, 1).expect("valid frame rate");
        let segments = build_keep_segments(&intervals(&[(5.15, 7.85)]), 20.0, rate);
        let frames: Vec<(i64, i64)> = segments
            .iter()
            .map(|segment| (segment.start_frame, segment.end_frame))
            .collect();
        assert_eq!(frames, [(0, 129), (196, 500)]);
    }

    #[test]
    fn build_keep_segments_returns_whole_media_without_silence() {
        let rate = FrameRate::new(25, 1).expect("valid frame rate");
        let segments = build_keep_segments(&[], 20.0, rate);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].frame_count(), 500);
    }
}
