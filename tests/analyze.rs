//! End-to-end tests that drive real ffmpeg subprocesses.
//!
//! A synthetic three-track MKV stands in for an OBS recording: the mic and
//! Discord tracks go quiet over different ranges, the game track stays loud
//! for the whole file so it can be proven to never influence detection.
//!
//! Every test skips with a message when ffmpeg/ffprobe are missing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use clap::Parser;
use quietcut::cli::{Cli, Command as CliCommand};
use quietcut::pipeline::{self, Analysis, DetectionConfig};

const FIXTURE_DURATION: f64 = 20.0;
const FRAME_RATE: f64 = 30.0;
/// silencedetect reacts a few samples late, and cut points land on frames.
const TOLERANCE: f64 = 0.1;

/// Mic is quiet in (5, 12), Discord in (3, 8); the overlap is (5, 8).
fn build_fixture() -> Option<&'static Path> {
    static FIXTURE: OnceLock<Option<PathBuf>> = OnceLock::new();

    FIXTURE
        .get_or_init(|| {
            if !verify_ffmpeg_available() {
                return None;
            }

            let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("quietcut-fixture.mkv");
            if path.is_file() {
                return Some(path);
            }

            let status = Command::new("ffmpeg")
                .args([
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=320x240:rate=30:duration=20",
                    "-f",
                    "lavfi",
                    "-i",
                    "aevalsrc=0.5*sin(2*PI*440*t)*(between(t\\,0\\,5)+between(t\\,12\\,20)):s=48000:d=20",
                    "-f",
                    "lavfi",
                    "-i",
                    "aevalsrc=0.5*sin(2*PI*330*t):s=48000:d=20",
                    "-f",
                    "lavfi",
                    "-i",
                    "aevalsrc=0.5*sin(2*PI*220*t)*(between(t\\,0\\,3)+between(t\\,8\\,20)):s=48000:d=20",
                    "-map",
                    "0:v",
                    "-map",
                    "1:a",
                    "-map",
                    "2:a",
                    "-map",
                    "3:a",
                    "-c:v",
                    "libx264",
                    "-preset",
                    "ultrafast",
                    "-pix_fmt",
                    "yuv420p",
                    "-c:a",
                    "flac",
                    "-metadata:s:a:0",
                    "title=Mic",
                    "-metadata:s:a:1",
                    "title=Game",
                    "-metadata:s:a:2",
                    "title=Discord",
                ])
                .arg(&path)
                .status()
                .expect("ffmpeg must be runnable once it was found");

            assert!(status.success(), "fixture generation failed: {status}");
            Some(path)
        })
        .as_deref()
}

fn verify_ffmpeg_available() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|tool| {
        Command::new(tool)
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

/// Builds a config the same way the CLI does, so flag handling is covered too.
fn build_config(fixture: &Path, extra: &[&str]) -> DetectionConfig {
    let mut argv: Vec<String> = vec![
        "quietcut".to_string(),
        "analyze".to_string(),
        fixture.to_string_lossy().into_owned(),
    ];
    argv.extend(extra.iter().map(|value| value.to_string()));

    let cli = Cli::try_parse_from(argv).expect("arguments must parse");
    let CliCommand::Analyze(args) = &cli.command;
    args.build_detection_config()
        .expect("arguments must validate")
}

fn run_analysis(fixture: &Path, extra: &[&str]) -> Analysis {
    pipeline::analyze_media(&build_config(fixture, extra)).expect("analysis must succeed")
}

fn keep_boundaries(analysis: &Analysis) -> Vec<(f64, f64)> {
    let rate = analysis.media.frame_rate;
    analysis
        .keep_segments
        .iter()
        .map(|segment| (segment.start_seconds(rate), segment.end_seconds(rate)))
        .collect()
}

#[track_caller]
fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= TOLERANCE,
        "expected {expected} +/- {TOLERANCE}, got {actual}"
    );
}

macro_rules! fixture_or_skip {
    () => {
        match build_fixture() {
            Some(path) => path,
            None => {
                eprintln!("skipping: ffmpeg/ffprobe not available on PATH");
                return;
            }
        }
    };
}

#[test]
fn analyze_cuts_only_where_both_streams_are_silent() {
    let fixture = fixture_or_skip!();
    let analysis = run_analysis(fixture, &[]);

    assert_eq!(analysis.mic.audio_index, 0);
    assert_eq!(
        analysis.discord.as_ref().map(|stream| stream.audio_index),
        Some(2)
    );

    assert_eq!(
        analysis.silences.len(),
        1,
        "silences: {:?}",
        analysis.silences
    );
    assert_close(analysis.silences[0].start, 5.15);
    assert_close(analysis.silences[0].end, 7.85);

    let keep = keep_boundaries(&analysis);
    assert_eq!(keep.len(), 2, "keep segments: {keep:?}");
    assert_close(keep[0].0, 0.0);
    assert_close(keep[0].1, 5.15);
    assert_close(keep[1].0, 7.85);
    assert_close(keep[1].1, FIXTURE_DURATION);
}

#[test]
fn analyze_keeps_cut_points_on_the_frame_grid() {
    let fixture = fixture_or_skip!();
    let analysis = run_analysis(fixture, &[]);
    let rate = analysis.media.frame_rate;

    assert_eq!(rate.fps(), FRAME_RATE);
    for segment in &analysis.keep_segments {
        assert!(segment.start_frame < segment.end_frame);
        // Frame numbers are integers by construction; check the round trip.
        assert_close(
            segment.start_seconds(rate) * FRAME_RATE,
            segment.start_frame as f64,
        );
    }

    let ordered = analysis
        .keep_segments
        .windows(2)
        .all(|pair| pair[0].end_frame <= pair[1].start_frame);
    assert!(ordered, "keep segments must not overlap");
}

#[test]
fn analyze_never_cuts_when_the_second_stream_stays_loud() {
    let fixture = fixture_or_skip!();
    // The game track is loud for the whole file, so intersecting against it
    // must leave nothing to cut.
    let analysis = run_analysis(fixture, &["--discord-name", "Game"]);

    assert!(
        analysis.silences.is_empty(),
        "silences: {:?}",
        analysis.silences
    );
    let keep = keep_boundaries(&analysis);
    assert_eq!(keep.len(), 1);
    assert_close(keep[0].1, FIXTURE_DURATION);
}

#[test]
fn analyze_falls_back_to_mic_only_without_discord() {
    let fixture = fixture_or_skip!();
    let analysis = run_analysis(fixture, &["--discord-name", "Teamspeak"]);

    assert!(analysis.discord.is_none());
    assert!(analysis.discord_silences.is_none());
    assert!(
        analysis
            .warnings
            .iter()
            .any(|warning| warning.contains("mic stream only")),
        "warnings: {:?}",
        analysis.warnings
    );

    assert_eq!(analysis.silences.len(), 1);
    assert_close(analysis.silences[0].start, 5.15);
    assert_close(analysis.silences[0].end, 11.85);
}

#[test]
fn analyze_resolves_streams_by_explicit_index() {
    let fixture = fixture_or_skip!();
    let analysis = run_analysis(fixture, &["--mic-stream", "0", "--discord-stream", "2"]);

    assert_eq!(analysis.silences.len(), 1);
    assert_close(analysis.silences[0].start, 5.15);
    assert_close(analysis.silences[0].end, 7.85);
}

#[test]
fn analyze_honours_padding_and_min_silence() {
    let fixture = fixture_or_skip!();

    let padded = run_analysis(fixture, &["--padding", "0.5"]);
    assert_eq!(padded.silences.len(), 1);
    assert_close(padded.silences[0].start, 5.5);
    assert_close(padded.silences[0].end, 7.5);

    // The overlap is 3 s wide, so a 4 s minimum must discard it.
    let strict = run_analysis(fixture, &["--min-silence", "4"]);
    assert!(strict.silences.is_empty());
}

#[test]
fn analyze_rejects_an_unknown_mic_name() {
    let fixture = fixture_or_skip!();
    let config = build_config(fixture, &["--mic-name", "Nope"]);
    let error = pipeline::analyze_media(&config).expect_err("unknown mic name must fail");
    assert!(
        error
            .to_string()
            .contains("no audio stream matches the mic name"),
        "unexpected error: {error}"
    );
}

#[test]
fn analyze_command_prints_a_report() {
    let fixture = fixture_or_skip!();
    let output = Command::new(env!("CARGO_BIN_EXE_quietcut"))
        .arg("analyze")
        .arg(fixture)
        .output()
        .expect("binary must run");

    assert!(output.status.success(), "exit status: {}", output.status);
    let stdout = String::from_utf8(output.stdout).expect("report must be UTF-8");
    assert!(stdout.contains("Audio streams:"), "stdout: {stdout}");
    assert!(stdout.contains("Keep segments (2):"), "stdout: {stdout}");
    assert!(stdout.contains("<- discord"), "stdout: {stdout}");
}

#[test]
fn analyze_command_fails_on_a_missing_input() {
    let output = Command::new(env!("CARGO_BIN_EXE_quietcut"))
        .args(["analyze", "/nonexistent/quietcut-missing.mkv"])
        .output()
        .expect("binary must run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("input file not found"), "stderr: {stderr}");
}
