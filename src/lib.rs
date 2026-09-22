//! quietcut detects silent sections in OBS multi-track recordings.
//!
//! The original media is never modified: detection runs through ffmpeg
//! subprocesses and the result is a list of keep segments that a timeline
//! exporter can turn into cuts.

#![forbid(unsafe_code)]

pub mod cli;
pub mod error;
pub mod exec;
pub mod ffprobe;
pub mod frame;
pub mod interval;
pub mod pipeline;
pub mod report;
pub mod silence;
pub mod streams;

pub use error::{Error, Result};
