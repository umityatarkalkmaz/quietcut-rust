//! The detection pipeline: probe, detect, combine, snap.

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::ffprobe::{self, AudioStream, MediaInfo};
use crate::frame::{FrameRate, FrameSegment, snap_intervals_to_frames};
use crate::interval::{self, Interval};
use crate::silence;
use crate::streams::{self, StreamMatch, StreamSelector};

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

    let mic = resolve_mic_stream(&media, config, &mut warnings)?;
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

fn resolve_mic_stream(
    media: &MediaInfo,
    config: &DetectionConfig,
    warnings: &mut Vec<String>,
) -> Result<AudioStream> {
    let resolved = streams::resolve_stream(&media.audio_streams, &config.mic, "mic")?;
    let Some(found) = resolved else {
        return Err(build_missing_stream_error("mic", &config.mic, media));
    };
    warn_about_ambiguous_title(&found, &config.mic, warnings);
    Ok(found.stream.clone())
}

fn resolve_discord_stream(
    media: &MediaInfo,
    config: &DetectionConfig,
    warnings: &mut Vec<String>,
) -> Result<Option<AudioStream>> {
    let resolved = streams::resolve_stream(&media.audio_streams, &config.discord, "discord")?;
    let Some(found) = resolved else {
        warnings.push(format!(
            "{}; detecting on the mic stream only",
            describe_missing_stream("discord", &config.discord, media)
        ));
        return Ok(None);
    };
    warn_about_ambiguous_title(&found, &config.discord, warnings);
    Ok(Some(found.stream.clone()))
}

/// Explains why a required stream could not be resolved.
fn build_missing_stream_error(
    role: &'static str,
    selector: &StreamSelector,
    media: &MediaInfo,
) -> Error {
    let count = media.audio_streams.len();
    match selector {
        StreamSelector::ByName(name) => Error::StreamNameNotFound {
            role,
            name: name.clone(),
            available: streams::format_available_titles(&media.audio_streams),
        },
        StreamSelector::ByIndex(index) => Error::StreamIndexOutOfRange {
            role,
            index: *index,
            count,
            last: count.saturating_sub(1),
        },
        StreamSelector::ByDefaultIndex(index) => Error::DefaultStreamMissing {
            role,
            index: *index,
            count,
        },
    }
}

/// Explains why an optional stream could not be resolved.
fn describe_missing_stream(role: &str, selector: &StreamSelector, media: &MediaInfo) -> String {
    match selector {
        StreamSelector::ByName(name) => format!("no audio stream titled \"{name}\""),
        StreamSelector::ByIndex(index) | StreamSelector::ByDefaultIndex(index) => format!(
            "no {role} stream at a:{index} (the file has {} audio stream(s))",
            media.audio_streams.len()
        ),
    }
}

fn warn_about_ambiguous_title(
    found: &StreamMatch<'_>,
    selector: &StreamSelector,
    warnings: &mut Vec<String>,
) {
    if found.match_count > 1
        && let StreamSelector::ByName(name) = selector
    {
        warnings.push(format!(
            "{} audio streams are titled \"{name}\"; using {}",
            found.match_count,
            found.stream.label()
        ));
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

    fn build_media(titles: &[Option<&str>]) -> MediaInfo {
        MediaInfo {
            path: PathBuf::from("/tmp/sample.mkv"),
            duration: 20.0,
            frame_rate: FrameRate::new(30, 1).expect("valid frame rate"),
            video_stream_count: 1,
            audio_streams: titles
                .iter()
                .enumerate()
                .map(|(audio_index, title)| AudioStream {
                    audio_index,
                    file_index: audio_index + 1,
                    title: title.map(str::to_string),
                    codec: Some("aac".to_string()),
                    channels: Some(2),
                })
                .collect(),
        }
    }

    fn build_config(mic: StreamSelector, discord: StreamSelector) -> DetectionConfig {
        DetectionConfig {
            input: PathBuf::from("/tmp/sample.mkv"),
            mic,
            discord,
            mic_threshold_db: -40.0,
            discord_threshold_db: -45.0,
            min_silence: 0.6,
            padding: 0.15,
        }
    }

    fn default_config() -> DetectionConfig {
        build_config(
            StreamSelector::ByDefaultIndex(0),
            StreamSelector::ByDefaultIndex(2),
        )
    }

    #[test]
    fn resolve_streams_uses_the_default_layout_without_titles() {
        let media = build_media(&[None, None, None]);
        let mut warnings = Vec::new();
        let mic = resolve_mic_stream(&media, &default_config(), &mut warnings).expect("mic");
        let discord = resolve_discord_stream(&media, &default_config(), &mut warnings)
            .expect("resolution must succeed")
            .expect("discord must resolve");
        assert_eq!((mic.audio_index, discord.audio_index), (0, 2));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
    }

    #[test]
    fn resolve_discord_stream_warns_when_the_default_is_missing() {
        let media = build_media(&[Some("Mic"), Some("Game")]);
        let mut warnings = Vec::new();
        let discord = resolve_discord_stream(&media, &default_config(), &mut warnings)
            .expect("a missing default discord stream is not an error");
        assert!(discord.is_none());
        assert_eq!(
            warnings,
            [
                "no discord stream at a:2 (the file has 2 audio stream(s)); \
              detecting on the mic stream only"
            ]
        );
    }

    #[test]
    fn resolve_discord_stream_rejects_a_missing_explicit_index() {
        let media = build_media(&[None, None]);
        let config = build_config(
            StreamSelector::ByDefaultIndex(0),
            StreamSelector::ByIndex(2),
        );
        let error = resolve_discord_stream(&media, &config, &mut Vec::new()).unwrap_err();
        assert!(matches!(
            error,
            Error::StreamIndexOutOfRange { index: 2, .. }
        ));
    }

    #[test]
    fn resolve_mic_stream_rejects_a_missing_default_index() {
        let media = build_media(&[None]);
        let config = build_config(
            StreamSelector::ByDefaultIndex(1),
            StreamSelector::ByDefaultIndex(2),
        );
        let error = resolve_mic_stream(&media, &config, &mut Vec::new()).unwrap_err();
        assert!(matches!(
            error,
            Error::DefaultStreamMissing {
                role: "mic",
                index: 1,
                count: 1
            }
        ));
    }

    #[test]
    fn resolve_mic_stream_rejects_an_unknown_title() {
        let media = build_media(&[Some("Mic")]);
        let config = build_config(
            StreamSelector::ByName("Microphone".to_string()),
            StreamSelector::ByDefaultIndex(2),
        );
        let error = resolve_mic_stream(&media, &config, &mut Vec::new()).unwrap_err();
        assert!(matches!(
            error,
            Error::StreamNameNotFound { role: "mic", .. }
        ));
    }

    #[test]
    fn resolve_mic_stream_warns_about_ambiguous_titles() {
        let media = build_media(&[Some("Mic"), Some("mic")]);
        let config = build_config(
            StreamSelector::ByName("MIC".to_string()),
            StreamSelector::ByDefaultIndex(2),
        );
        let mut warnings = Vec::new();
        let mic = resolve_mic_stream(&media, &config, &mut warnings).expect("mic");
        assert_eq!(mic.audio_index, 0);
        assert_eq!(
            warnings,
            ["2 audio streams are titled \"MIC\"; using a:0 \"Mic\""]
        );
    }

    #[test]
    fn build_keep_segments_returns_whole_media_without_silence() {
        let rate = FrameRate::new(25, 1).expect("valid frame rate");
        let segments = build_keep_segments(&[], 20.0, rate);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].frame_count(), 500);
    }
}
