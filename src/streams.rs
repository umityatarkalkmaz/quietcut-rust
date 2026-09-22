//! Resolution of the mic and Discord audio streams.
//!
//! Stream titles come from OBS track names stored in Matroska metadata. OBS
//! track numbers are never used as indices; every index here is audio relative
//! (`a:N`).

use crate::error::{Error, Result};
use crate::ffprobe::AudioStream;

/// How the user asked for one of the audio streams.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamSelector {
    /// Match the stream title, ignoring case.
    ByName(String),
    /// Take the audio-relative stream index as given.
    ByIndex(usize),
}

/// A resolved stream plus how many streams the selector matched.
#[derive(Clone, Debug)]
pub struct StreamMatch<'a> {
    pub stream: &'a AudioStream,
    pub match_count: usize,
}

/// Resolves one selector.
///
/// An index that does not exist is an error: the user asked for something
/// concrete that the file cannot provide. A title that does not match returns
/// `Ok(None)`, because the caller decides whether that stream is optional.
pub fn resolve_stream<'a>(
    streams: &'a [AudioStream],
    selector: &StreamSelector,
    role: &'static str,
) -> Result<Option<StreamMatch<'a>>> {
    match selector {
        StreamSelector::ByIndex(index) => {
            let stream = streams
                .get(*index)
                .ok_or_else(|| Error::StreamIndexOutOfRange {
                    role,
                    index: *index,
                    count: streams.len(),
                    last: streams.len().saturating_sub(1),
                })?;
            Ok(Some(StreamMatch {
                stream,
                match_count: 1,
            }))
        }
        StreamSelector::ByName(name) => {
            let matches = find_streams_by_name(streams, name);
            Ok(matches.first().map(|stream| StreamMatch {
                stream,
                match_count: matches.len(),
            }))
        }
    }
}

/// Returns every stream whose title equals `name`, ignoring case.
pub fn find_streams_by_name<'a>(streams: &'a [AudioStream], name: &str) -> Vec<&'a AudioStream> {
    let name = name.trim();
    streams
        .iter()
        .filter(|stream| {
            stream
                .title
                .as_deref()
                .is_some_and(|title| title.trim().eq_ignore_ascii_case(name))
        })
        .collect()
}

/// Renders the available stream titles for error messages.
pub fn format_available_titles(streams: &[AudioStream]) -> String {
    if streams.is_empty() {
        return "none".to_string();
    }
    streams
        .iter()
        .map(AudioStream::label)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_streams(titles: &[Option<&str>]) -> Vec<AudioStream> {
        titles
            .iter()
            .enumerate()
            .map(|(audio_index, title)| AudioStream {
                audio_index,
                file_index: audio_index + 1,
                title: title.map(str::to_string),
                codec: Some("flac".to_string()),
                channels: Some(2),
            })
            .collect()
    }

    fn sample_streams() -> Vec<AudioStream> {
        build_streams(&[Some("Mic"), Some("Game"), Some("Discord")])
    }

    #[test]
    fn resolve_stream_matches_title_ignoring_case() {
        let streams = sample_streams();
        let selector = StreamSelector::ByName("discord".to_string());
        let resolved = resolve_stream(&streams, &selector, "discord")
            .expect("resolution must succeed")
            .expect("title must match");
        assert_eq!(resolved.stream.audio_index, 2);
        assert_eq!(resolved.match_count, 1);
    }

    #[test]
    fn resolve_stream_ignores_surrounding_whitespace() {
        let streams = build_streams(&[Some(" Mic ")]);
        let selector = StreamSelector::ByName("mic".to_string());
        assert!(
            resolve_stream(&streams, &selector, "mic")
                .expect("resolution must succeed")
                .is_some()
        );
    }

    #[test]
    fn resolve_stream_reports_ambiguous_titles() {
        let streams = build_streams(&[Some("Mic"), Some("MIC")]);
        let selector = StreamSelector::ByName("Mic".to_string());
        let resolved = resolve_stream(&streams, &selector, "mic")
            .expect("resolution must succeed")
            .expect("title must match");
        assert_eq!(resolved.stream.audio_index, 0);
        assert_eq!(resolved.match_count, 2);
    }

    #[test]
    fn resolve_stream_returns_none_for_unknown_title() {
        let streams = sample_streams();
        let selector = StreamSelector::ByName("Teamspeak".to_string());
        assert!(
            resolve_stream(&streams, &selector, "discord")
                .expect("resolution must succeed")
                .is_none()
        );
    }

    #[test]
    fn resolve_stream_never_matches_untitled_stream() {
        let streams = build_streams(&[None, None]);
        let selector = StreamSelector::ByName("Mic".to_string());
        assert!(
            resolve_stream(&streams, &selector, "mic")
                .expect("resolution must succeed")
                .is_none()
        );
    }

    #[test]
    fn resolve_stream_accepts_audio_relative_index() {
        let streams = sample_streams();
        let resolved = resolve_stream(&streams, &StreamSelector::ByIndex(1), "mic")
            .expect("resolution must succeed")
            .expect("index must resolve");
        assert_eq!(resolved.stream.file_index, 2);
    }

    #[test]
    fn resolve_stream_rejects_out_of_range_index() {
        let streams = sample_streams();
        let error = resolve_stream(&streams, &StreamSelector::ByIndex(3), "mic").unwrap_err();
        assert!(matches!(
            error,
            Error::StreamIndexOutOfRange {
                index: 3,
                count: 3,
                ..
            }
        ));
    }

    #[test]
    fn format_available_titles_lists_every_stream() {
        let streams = build_streams(&[Some("Mic"), None]);
        assert_eq!(
            format_available_titles(&streams),
            "a:0 \"Mic\", a:1 (untitled)"
        );
    }
}
