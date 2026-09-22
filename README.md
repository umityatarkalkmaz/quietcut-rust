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
      --mic-name NAME            Mic stream title, case-insensitive [default: Mic]
      --mic-stream N             Mic stream by audio-relative index (a:N)
      --discord-name NAME        Discord stream title [default: Discord]
      --discord-stream N         Discord stream by audio-relative index (a:N)
      --mic-threshold DB         Mic silence threshold in dBFS [default: -40]
      --discord-threshold DB     Discord silence threshold in dBFS [default: -45]
      --min-silence SEC          Shortest silence worth cutting [default: 0.6]
      --padding SEC              Breathing room kept around every cut [default: 0.15]
```

`--mic-name` and `--mic-stream` are mutually exclusive, as are their Discord
counterparts. Indices are always audio relative (`a:0`, `a:1`, ...), never OBS
track numbers.

## How detection works

1. `ffprobe` reports the audio streams, the container duration and the frame rate.
2. The mic and Discord streams are resolved by stream title, or by an explicit
   index. A missing mic stream is an error; a missing Discord stream only prints
   a warning and falls back to mic-only detection. The game track is never used.
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
