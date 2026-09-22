//! Error type for every fallible operation in the crate.
//!
//! All messages are developer/CLI facing and therefore English.

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("`{tool}` was not found on PATH; install ffmpeg (which provides ffmpeg and ffprobe)")]
    ToolMissing { tool: &'static str },

    #[error("failed to run `{tool}`")]
    ToolSpawn {
        tool: &'static str,
        #[source]
        source: std::io::Error,
    },

    #[error("`{tool}` failed ({status}){}", format_stderr_tail(.stderr))]
    ToolFailed {
        tool: &'static str,
        status: String,
        stderr: String,
    },

    #[error("`{tool}` produced output that is not valid UTF-8")]
    ToolOutputNotUtf8 { tool: &'static str },

    #[error("invalid value for `{flag}`: {reason}")]
    InvalidArgument { flag: &'static str, reason: String },

    #[error("input file not found: {}", .path.display())]
    InputNotFound { path: PathBuf },

    #[error("input is not a regular file: {}", .path.display())]
    InputNotFile { path: PathBuf },

    #[error("cannot resolve input path: {}", .path.display())]
    InputPathUnresolvable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot parse ffprobe JSON output")]
    ProbeDecode {
        #[source]
        source: serde_json::Error,
    },

    #[error("no video stream found in {}; a video stream is required for the frame grid", .path.display())]
    NoVideoStream { path: PathBuf },

    #[error("no audio stream found in {}", .path.display())]
    NoAudioStream { path: PathBuf },

    #[error("cannot determine the frame rate of {} (ffprobe reported {reported})", .path.display())]
    UnknownFrameRate { path: PathBuf, reported: String },

    #[error("cannot determine the duration of {}", .path.display())]
    UnknownDuration { path: PathBuf },

    #[error(
        "no audio stream matches the {role} name `{name}` (case-insensitive); \
         available titles: {available}. Use `--{role}-stream N` to select by audio-relative index"
    )]
    StreamNameNotFound {
        role: &'static str,
        name: String,
        available: String,
    },

    #[error(
        "`--{role}-stream {index}` is out of range: the file has {count} audio stream(s) (a:0..a:{last})"
    )]
    StreamIndexOutOfRange {
        role: &'static str,
        index: usize,
        count: usize,
        last: usize,
    },

    #[error(
        "mic and discord selectors resolve to the same audio stream a:{index}; they must differ"
    )]
    StreamSelectionCollision { index: usize },

    #[error("output file already exists: {} (use --force to overwrite)", .path.display())]
    OutputExists { path: PathBuf },
}

/// Appends the tail of a captured stderr buffer to an error message, if any.
fn format_stderr_tail(stderr: &str) -> String {
    const MAX_LINES: usize = 12;

    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    let tail = if lines.len() > MAX_LINES {
        &lines[lines.len() - MAX_LINES..]
    } else {
        &lines[..]
    };
    format!(":\n{}", tail.join("\n"))
}
