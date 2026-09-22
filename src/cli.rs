//! Command line surface and argument validation.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::error::{Error, Result};
use crate::pipeline::DetectionConfig;
use crate::streams::StreamSelector;

pub const DEFAULT_MIC_NAME: &str = "Mic";
pub const DEFAULT_DISCORD_NAME: &str = "Discord";
pub const DEFAULT_MIC_THRESHOLD_DB: f64 = -40.0;
pub const DEFAULT_DISCORD_THRESHOLD_DB: f64 = -45.0;
pub const DEFAULT_MIN_SILENCE: f64 = 0.6;
pub const DEFAULT_PADDING: f64 = 0.15;

#[derive(Debug, Parser)]
#[command(
    name = "quietcut",
    version,
    about = "Detect silent sections in OBS multi-track recordings"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print resolved streams, detected silences and keep segments.
    Analyze(DetectionArgs),
}

/// Arguments shared by every command that runs silence detection.
#[derive(Debug, Args)]
pub struct DetectionArgs {
    /// OBS recording to analyse.
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,

    /// Stream title of the mic track, matched case-insensitively.
    #[arg(long, value_name = "NAME", default_value = DEFAULT_MIC_NAME, conflicts_with = "mic_stream")]
    pub mic_name: String,

    /// Audio-relative index of the mic track (a:N), bypassing the title match.
    #[arg(long, value_name = "N")]
    pub mic_stream: Option<usize>,

    /// Stream title of the Discord track, matched case-insensitively.
    #[arg(long, value_name = "NAME", default_value = DEFAULT_DISCORD_NAME, conflicts_with = "discord_stream")]
    pub discord_name: String,

    /// Audio-relative index of the Discord track (a:N), bypassing the title match.
    #[arg(long, value_name = "N")]
    pub discord_stream: Option<usize>,

    /// Silence threshold of the mic track in dBFS (must be negative).
    #[arg(long, value_name = "DB", default_value_t = DEFAULT_MIC_THRESHOLD_DB, allow_negative_numbers = true)]
    pub mic_threshold: f64,

    /// Silence threshold of the Discord track in dBFS (must be negative).
    #[arg(long, value_name = "DB", default_value_t = DEFAULT_DISCORD_THRESHOLD_DB, allow_negative_numbers = true)]
    pub discord_threshold: f64,

    /// Shortest silence worth cutting, in seconds.
    #[arg(long, value_name = "SEC", default_value_t = DEFAULT_MIN_SILENCE, allow_negative_numbers = true)]
    pub min_silence: f64,

    /// Breathing room kept on both sides of every cut, in seconds.
    #[arg(long, value_name = "SEC", default_value_t = DEFAULT_PADDING, allow_negative_numbers = true)]
    pub padding: f64,
}

impl DetectionArgs {
    /// Validates the raw arguments and turns them into a [`DetectionConfig`].
    pub fn build_detection_config(&self) -> Result<DetectionConfig> {
        let min_silence = validate_duration("--min-silence", self.min_silence, false)?;
        let padding = validate_duration("--padding", self.padding, true)?;

        if padding >= min_silence {
            return Err(Error::InvalidArgument {
                flag: "--padding",
                reason: format!(
                    "must be smaller than --min-silence ({min_silence}), got {padding}"
                ),
            });
        }

        Ok(DetectionConfig {
            input: self.input.clone(),
            mic: build_selector(&self.mic_name, self.mic_stream),
            discord: build_selector(&self.discord_name, self.discord_stream),
            mic_threshold_db: validate_threshold("--mic-threshold", self.mic_threshold)?,
            discord_threshold_db: validate_threshold(
                "--discord-threshold",
                self.discord_threshold,
            )?,
            min_silence,
            padding,
        })
    }
}

fn build_selector(name: &str, index: Option<usize>) -> StreamSelector {
    match index {
        Some(index) => StreamSelector::ByIndex(index),
        None => StreamSelector::ByName(name.to_string()),
    }
}

/// Accepts finite, strictly negative dBFS thresholds only.
fn validate_threshold(flag: &'static str, value: f64) -> Result<f64> {
    if !value.is_finite() || value >= 0.0 {
        return Err(Error::InvalidArgument {
            flag,
            reason: format!("must be a negative dBFS value, got {value}"),
        });
    }
    Ok(value)
}

/// Accepts finite, non-negative durations; `allow_zero` permits exactly zero.
fn validate_duration(flag: &'static str, value: f64, allow_zero: bool) -> Result<f64> {
    let valid = value.is_finite() && (value > 0.0 || (allow_zero && value == 0.0));
    if !valid {
        let bound = if allow_zero {
            "zero or greater"
        } else {
            "greater than zero"
        };
        return Err(Error::InvalidArgument {
            flag,
            reason: format!("must be {bound} seconds, got {value}"),
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(extra: &[&str]) -> Cli {
        let mut argv = vec!["quietcut", "analyze", "recording.mkv"];
        argv.extend_from_slice(extra);
        Cli::try_parse_from(argv).expect("arguments must parse")
    }

    fn detection_args(extra: &[&str]) -> DetectionArgs {
        match parse_args(extra).command {
            Command::Analyze(args) => args,
        }
    }

    #[test]
    fn build_detection_config_applies_documented_defaults() {
        let config = detection_args(&[])
            .build_detection_config()
            .expect("defaults must validate");
        assert_eq!(config.mic, StreamSelector::ByName("Mic".to_string()));
        assert_eq!(
            config.discord,
            StreamSelector::ByName("Discord".to_string())
        );
        assert_eq!(config.mic_threshold_db, -40.0);
        assert_eq!(config.discord_threshold_db, -45.0);
        assert_eq!(config.min_silence, 0.6);
        assert_eq!(config.padding, 0.15);
    }

    #[test]
    fn build_detection_config_prefers_explicit_indices() {
        let config = detection_args(&["--mic-stream", "2", "--discord-stream", "0"])
            .build_detection_config()
            .expect("indices must validate");
        assert_eq!(config.mic, StreamSelector::ByIndex(2));
        assert_eq!(config.discord, StreamSelector::ByIndex(0));
    }

    #[test]
    fn cli_accepts_negative_thresholds_as_separate_tokens() {
        let args = detection_args(&["--mic-threshold", "-35.5"]);
        assert_eq!(args.mic_threshold, -35.5);
    }

    #[test]
    fn cli_rejects_name_and_index_for_the_same_role() {
        let result = Cli::try_parse_from([
            "quietcut",
            "analyze",
            "recording.mkv",
            "--mic-name",
            "Mic",
            "--mic-stream",
            "0",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn build_detection_config_rejects_non_negative_threshold() {
        let error = detection_args(&["--mic-threshold", "0"])
            .build_detection_config()
            .unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidArgument {
                flag: "--mic-threshold",
                ..
            }
        ));
    }

    #[test]
    fn build_detection_config_rejects_non_positive_min_silence() {
        let error = detection_args(&["--min-silence", "0"])
            .build_detection_config()
            .unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidArgument {
                flag: "--min-silence",
                ..
            }
        ));
    }

    #[test]
    fn build_detection_config_rejects_negative_padding() {
        let error = detection_args(&["--padding", "-0.1"])
            .build_detection_config()
            .unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidArgument {
                flag: "--padding",
                ..
            }
        ));
    }

    #[test]
    fn build_detection_config_rejects_padding_above_min_silence() {
        let error = detection_args(&["--padding", "0.6", "--min-silence", "0.6"])
            .build_detection_config()
            .unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidArgument {
                flag: "--padding",
                ..
            }
        ));
    }

    #[test]
    fn build_detection_config_accepts_zero_padding() {
        let config = detection_args(&["--padding", "0"])
            .build_detection_config()
            .expect("zero padding is allowed");
        assert_eq!(config.padding, 0.0);
    }

    #[test]
    fn build_detection_config_rejects_non_finite_values() {
        let error = detection_args(&["--min-silence", "inf"])
            .build_detection_config()
            .unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidArgument {
                flag: "--min-silence",
                ..
            }
        ));
    }
}
