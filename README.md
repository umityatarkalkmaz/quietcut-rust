# quietcut

Detects silent sections in OBS multi-track MKV recordings so an editor can cut
them out. The original media is never read for anything but analysis: nothing is
modified, re-encoded or copied.

## Status

Milestone 1 is implemented: stream probing, silence detection and the `analyze`
report. Timeline export (FCPXML for DaVinci Resolve) is milestone 2 and is not
part of the CLI yet.

## Requirements

`ffmpeg` and `ffprobe` on `PATH`. Both are checked at startup and every call is
a plain subprocess with separated arguments — no shell is ever involved.
quietcut itself makes no network calls and has no network-capable dependency.

## Usage

```bash
quietcut analyze recording.mkv
```

```text
      --mic-stream N             Mic stream by audio-relative index (a:N) [default: 0]
      --mic-name NAME            Mic stream by title instead, case-insensitive
      --discord-stream N         Discord stream by audio-relative index (a:N) [default: 2]
      --discord-name NAME        Discord stream by title instead, case-insensitive
      --mic-threshold DB         Mic silence threshold in dBFS [default: -40]
      --discord-threshold DB     Discord silence threshold in dBFS [default: -45]
      --min-silence SEC          Shortest silence worth cutting [default: 0.6]
      --padding SEC              Breathing room kept around every cut [default: 0.15]
```

Without flags the recording layout is assumed to be Mic / Game / Discord: the
mic is the first audio stream (`a:0`) and Discord the third (`a:2`). The report
marks how each stream was chosen (`default position`, `by index`, `by title`),
so a recording with a different layout is visible at a glance.

`--mic-name` and `--mic-stream` are mutually exclusive, as are their Discord
counterparts. Indices are always audio relative (`a:0`, `a:1`, ...), never OBS
track numbers.

## How detection works

1. `ffprobe` reports the audio streams, the container duration and the frame rate.
2. The mic and Discord streams come from their default positions, an explicit
   index or a stream title. A missing mic stream is an error, and so is an
   explicit index the file does not have. A missing Discord stream otherwise
   only prints a warning and falls back to mic-only detection. The game track
   (`a:1`) is not used unless selected explicitly.
3. `silencedetect` runs once per stream with its own threshold.
4. Silence is the **intersection** of both streams: a range is only cut when
   both were quiet at the same time.
5. Each range is shrunk by `--padding` on both sides, and ranges shorter than
   `--min-silence` are dropped.
6. Keep segments are the complement of what remains, snapped to the frame grid.

## Development

```bash
cargo test          # unit tests plus ffmpeg-backed integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

The integration tests build a synthetic three-track MKV with ffmpeg and skip
with a message when ffmpeg is unavailable. To confirm offline operation:

```bash
unshare -rn ./target/debug/quietcut analyze recording.mkv
```
