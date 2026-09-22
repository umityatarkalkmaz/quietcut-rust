//! Media probing through `ffprobe`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::exec::{self, FFPROBE};
use crate::frame::{FrameRate, parse_frame_rate};

/// Everything the pipeline needs to know about the input file.
#[derive(Clone, Debug)]
pub struct MediaInfo {
    pub path: PathBuf,
    pub duration: f64,
    pub frame_rate: FrameRate,
    pub video_stream_count: usize,
    pub audio_streams: Vec<AudioStream>,
}

/// One audio stream, identified by its audio-relative index (`a:N`).
#[derive(Clone, Debug)]
pub struct AudioStream {
    pub audio_index: usize,
    pub file_index: usize,
    pub title: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<u32>,
}

impl AudioStream {
    pub fn label(&self) -> String {
        match &self.title {
            Some(title) => format!("a:{} \"{}\"", self.audio_index, title),
            None => format!("a:{} (untitled)", self.audio_index),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    format: ProbeFormat,
    #[serde(default)]
    streams: Vec<ProbeStream>,
}

#[derive(Debug, Default, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    index: usize,
    codec_type: Option<String>,
    codec_name: Option<String>,
    channels: Option<u32>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    duration: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

/// Probes streams, duration and frame rate of `path`.
pub fn probe_media(path: &Path) -> Result<MediaInfo> {
    let path = exec::validate_input_path(path)?;
    let output = exec::run_tool(
        FFPROBE,
        [
            "-v".as_ref(),
            "error".as_ref(),
            "-of".as_ref(),
            "json".as_ref(),
            "-show_entries".as_ref(),
            "format=duration:stream=index,codec_type,codec_name,channels,r_frame_rate,avg_frame_rate,duration:stream_tags"
                .as_ref(),
            path.as_os_str(),
        ],
    )?;

    if !output.status.success() {
        return Err(Error::ToolFailed {
            tool: FFPROBE,
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let json = exec::decode_output(FFPROBE, &output.stdout)?;
    parse_probe_output(&json, &path)
}

/// Turns raw ffprobe JSON into a [`MediaInfo`].
pub fn parse_probe_output(json: &str, path: &Path) -> Result<MediaInfo> {
    let probe: ProbeOutput =
        serde_json::from_str(json).map_err(|source| Error::ProbeDecode { source })?;

    let video_streams: Vec<&ProbeStream> = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("video"))
        .collect();
    let first_video = video_streams.first().ok_or_else(|| Error::NoVideoStream {
        path: path.to_path_buf(),
    })?;

    let frame_rate = select_frame_rate(first_video).ok_or_else(|| Error::UnknownFrameRate {
        path: path.to_path_buf(),
        reported: format!(
            "r_frame_rate={}, avg_frame_rate={}",
            first_video.r_frame_rate.as_deref().unwrap_or("none"),
            first_video.avg_frame_rate.as_deref().unwrap_or("none")
        ),
    })?;

    let audio_streams: Vec<AudioStream> = probe
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("audio"))
        .enumerate()
        .map(|(audio_index, stream)| AudioStream {
            audio_index,
            file_index: stream.index,
            title: fetch_tag(&stream.tags, "title"),
            codec: stream.codec_name.clone(),
            channels: stream.channels,
        })
        .collect();

    if audio_streams.is_empty() {
        return Err(Error::NoAudioStream {
            path: path.to_path_buf(),
        });
    }

    let duration = select_duration(&probe).ok_or_else(|| Error::UnknownDuration {
        path: path.to_path_buf(),
    })?;

    Ok(MediaInfo {
        path: path.to_path_buf(),
        duration,
        frame_rate,
        video_stream_count: video_streams.len(),
        audio_streams,
    })
}

fn select_frame_rate(stream: &ProbeStream) -> Option<FrameRate> {
    stream
        .r_frame_rate
        .as_deref()
        .and_then(parse_frame_rate)
        .or_else(|| stream.avg_frame_rate.as_deref().and_then(parse_frame_rate))
}

/// Picks the container duration, falling back to the longest stream duration.
///
/// Matroska usually reports per-stream duration only as a `DURATION` tag.
fn select_duration(probe: &ProbeOutput) -> Option<f64> {
    if let Some(duration) = probe.format.duration.as_deref().and_then(parse_seconds) {
        return Some(duration);
    }

    probe
        .streams
        .iter()
        .filter_map(|stream| {
            stream
                .duration
                .as_deref()
                .and_then(parse_seconds)
                .or_else(|| {
                    fetch_tag(&stream.tags, "duration")
                        .as_deref()
                        .and_then(parse_timestamp)
                })
        })
        .max_by(f64::total_cmp)
}

/// Reads a tag by name, ignoring case (Matroska writers disagree on casing).
fn fetch_tag(tags: &BTreeMap<String, String>, name: &str) -> Option<String> {
    tags.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_seconds(text: &str) -> Option<f64> {
    let seconds: f64 = text.trim().parse().ok()?;
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

/// Parses a `HH:MM:SS.nnnnnnnnn` timestamp into seconds.
fn parse_timestamp(text: &str) -> Option<f64> {
    let mut seconds = 0.0;
    let mut parts = 0;
    for field in text.trim().split(':') {
        let value: f64 = field.parse().ok()?;
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        seconds = seconds * 60.0 + value;
        parts += 1;
    }
    (parts > 0 && seconds > 0.0).then_some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "streams": [
            {"index": 0, "codec_name": "h264", "codec_type": "video",
             "r_frame_rate": "30000/1001", "avg_frame_rate": "30000/1001",
             "tags": {"DURATION": "00:00:20.000000000"}},
            {"index": 1, "codec_name": "flac", "codec_type": "audio", "channels": 1,
             "r_frame_rate": "0/0", "tags": {"title": "Mic"}},
            {"index": 2, "codec_name": "flac", "codec_type": "audio", "channels": 2,
             "r_frame_rate": "0/0", "tags": {"TITLE": "Game"}},
            {"index": 3, "codec_name": "flac", "codec_type": "audio", "channels": 2,
             "r_frame_rate": "0/0", "tags": {}}
        ],
        "format": {"duration": "20.004000"}
    }"#;

    fn parse_sample(json: &str) -> Result<MediaInfo> {
        parse_probe_output(json, Path::new("/tmp/sample.mkv"))
    }

    #[test]
    fn parse_probe_output_maps_audio_relative_indices() {
        let media = parse_sample(SAMPLE).expect("sample must parse");
        let indices: Vec<(usize, usize)> = media
            .audio_streams
            .iter()
            .map(|stream| (stream.audio_index, stream.file_index))
            .collect();
        assert_eq!(indices, [(0, 1), (1, 2), (2, 3)]);
    }

    #[test]
    fn parse_probe_output_reads_titles_case_insensitively() {
        let media = parse_sample(SAMPLE).expect("sample must parse");
        let titles: Vec<Option<&str>> = media
            .audio_streams
            .iter()
            .map(|stream| stream.title.as_deref())
            .collect();
        assert_eq!(titles, [Some("Mic"), Some("Game"), None]);
    }

    #[test]
    fn parse_probe_output_reads_duration_and_frame_rate() {
        let media = parse_sample(SAMPLE).expect("sample must parse");
        assert_eq!(media.duration, 20.004);
        assert_eq!(media.frame_rate, FrameRate::new(30000, 1001).unwrap());
        assert_eq!(media.video_stream_count, 1);
    }

    #[test]
    fn parse_probe_output_falls_back_to_duration_tag() {
        let json = SAMPLE.replace(r#""format": {"duration": "20.004000"}"#, r#""format": {}"#);
        let media = parse_sample(&json).expect("sample must parse");
        assert_eq!(media.duration, 20.0);
    }

    #[test]
    fn parse_probe_output_rejects_file_without_video_stream() {
        let json = r#"{"streams": [{"index": 0, "codec_type": "audio", "channels": 2}],
                       "format": {"duration": "10.0"}}"#;
        assert!(matches!(
            parse_sample(json),
            Err(Error::NoVideoStream { .. })
        ));
    }

    #[test]
    fn parse_probe_output_rejects_file_without_audio_stream() {
        let json = r#"{"streams": [{"index": 0, "codec_type": "video", "r_frame_rate": "25/1"}],
                       "format": {"duration": "10.0"}}"#;
        assert!(matches!(
            parse_sample(json),
            Err(Error::NoAudioStream { .. })
        ));
    }

    #[test]
    fn parse_probe_output_rejects_unusable_frame_rate() {
        let json = r#"{"streams": [
                {"index": 0, "codec_type": "video", "r_frame_rate": "0/0", "avg_frame_rate": "0/0"},
                {"index": 1, "codec_type": "audio", "channels": 2}],
               "format": {"duration": "10.0"}}"#;
        assert!(matches!(
            parse_sample(json),
            Err(Error::UnknownFrameRate { .. })
        ));
    }

    #[test]
    fn parse_probe_output_rejects_missing_duration() {
        let json = r#"{"streams": [
                {"index": 0, "codec_type": "video", "r_frame_rate": "25/1"},
                {"index": 1, "codec_type": "audio", "channels": 2}],
               "format": {}}"#;
        assert!(matches!(
            parse_sample(json),
            Err(Error::UnknownDuration { .. })
        ));
    }

    #[test]
    fn parse_probe_output_rejects_invalid_json() {
        assert!(matches!(
            parse_sample("not json"),
            Err(Error::ProbeDecode { .. })
        ));
    }

    #[test]
    fn parse_timestamp_reads_matroska_duration_tags() {
        assert_eq!(parse_timestamp("00:00:20.500000000"), Some(20.5));
        assert_eq!(parse_timestamp("01:02:03.000000000"), Some(3723.0));
        assert_eq!(parse_timestamp("N/A"), None);
    }
}
