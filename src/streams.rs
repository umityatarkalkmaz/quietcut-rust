//! Resolution of the mic and Discord audio streams.
//!
//! By default the streams are taken by position (the recording layout is
//! Mic / Game / Discord). Titles, which come from OBS track names stored in
//! Matroska metadata, are matched only on request. OBS track numbers are never
//! used as indices; every index here is audio relative (`a:N`).

use crate::error::{Error, Result};
use crate::ffprobe::AudioStream;

/// How one of the audio streams is selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamSelector {
    /// Match the stream title, ignoring case.
    ByName(String),
    /// Take the audio-relative index the user passed explicitly.
    ByIndex(usize),
    /// Take the audio-relative index of the default recording layout.
    ByDefaultIndex(usize),
}

impl StreamSelector {
    /// Says how the stream was chosen, for reports.
    pub fn describe(&self) -> &'static str {
        match self {
            Self::ByName(_) => "by title",
            Self::ByIndex(_) => "by index",
            Self::ByDefaultIndex(_) => "default position",
        }
    }
}

/// A resolved stream plus how many streams the selector matched.
#[derive(Clone, Debug)]
pub struct StreamMatch<'a> {
    pub stream: &'a AudioStream,
    pub match_count: usize,
}

/// Resolves one selector.
///
/// An explicit index that does not exist is an error: the user asked for
/// something concrete that the file cannot provide. A missing default index or
/// an unmatched title returns `Ok(None)`, because the caller decides whether
/// that stream is optional.
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
        StreamSelector::ByDefaultIndex(index) => {
            Ok(streams.get(*index).map(|stream| StreamMatch {
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
    fn resolve_stream_accepts_default_index() {
        let streams = sample_streams();
        let resolved = resolve_stream(&streams, &StreamSelector::ByDefaultIndex(2), "discord")
            .expect("resolution must succeed")
            .expect("default index must resolve");
        assert_eq!(resolved.stream.title.as_deref(), Some("Discord"));
    }

    #[test]
    fn resolve_stream_returns_none_for_missing_default_index() {
        let streams = build_streams(&[None, None]);
        assert!(
            resolve_stream(&streams, &StreamSelector::ByDefaultIndex(2), "discord")
                .expect("a missing default is not an error")
                .is_none()
        );
    }

    #[test]
    fn describe_names_every_selection_route() {
        assert_eq!(
            StreamSelector::ByName("Mic".to_string()).describe(),
            "by title"
        );
        assert_eq!(StreamSelector::ByIndex(1).describe(), "by index");
        assert_eq!(
            StreamSelector::ByDefaultIndex(0).describe(),
            "default position"
        );
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
