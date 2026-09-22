# Project Brief: quietcut

> Repository: `quietcut-rust`. Crate and binary name: `quietcut`.

Read this file fully before writing code. My global CLAUDE.md rules apply on top of this brief.

## Goal

A Rust CLI that detects silent sections in OBS multi-track MKV recordings and exports a
**DaVinci Resolve timeline (FCPXML)** with those sections removed. The original media is
never modified or re-encoded; Resolve performs the cuts on import.

## Hard constraints

- **Fully offline.** No network access of any kind. No HTTP clients, telemetry, update checks
  or crash reporters in the dependency tree. Verify with `cargo tree` before each milestone.
- **No `unsafe`.** Add `#![forbid(unsafe_code)]` to the crate root.
- **ffmpeg/ffprobe as subprocesses** via `std::process::Command`. Pass every argument with
  `.arg()` / `.args()`. Never build a shell string, never use `sh -c`.
- Check `ffmpeg` and `ffprobe` availability at startup and fail with a clear message if missing.
- Cross-platform (Linux primary, macOS secondary). No hardcoded paths.

## Input: my recording setup

- OBS records MKV with a single video stream (webcam composited in OBS) and **3 audio streams**:
  1. Mic
  2. Game
  3. Discord
- These are OBS tracks 2/3/4. In the file they appear as audio streams `a:0`, `a:1`, `a:2`.
  **Never assume indices from OBS track numbers.**
- OBS track names are written into MKV stream metadata (expected tag: `title`).
  Confirm this on a real sample with:
  `ffprobe -v error -select_streams a -show_entries stream=index:stream_tags=title -of json <file>`

## Stream resolution

1. Match by stream title (case-insensitive), configurable names, defaults `Mic` and `Discord`.
2. Fall back to explicit indices from CLI (`--mic-stream N`, `--discord-stream N`, audio-relative).
3. Mic not found by either route → error. Do not guess.
4. Discord not found → continue with mic only and print a warning (Discord is optional).
5. The game track is **never** used for detection.

## Silence detection algorithm

1. Run `silencedetect` separately on the mic and Discord streams, each with its own threshold:
   `ffmpeg -hide_banner -nostats -i <file> -map 0:a:<N> -af silencedetect=noise=<dB>:d=<sec> -f null -`
2. Parse `silence_start` / `silence_end` from stderr. A trailing `silence_start` without an end
   closes at the media duration.
3. Silence = **intersection** of mic silences and Discord silences (both quiet at the same time).
4. Shrink each silent interval by `padding` on both sides; drop intervals that become shorter
   than `min_silence`.
5. Keep segments = complement of the remaining silences within `[0, duration]`.
6. Snap every cut point to the video frame grid (frame rate from ffprobe).

Starting defaults (tunable): mic threshold `-40dB`, Discord threshold `-45dB`,
`min_silence` 0.6 s, `padding` 0.15 s.

## Output: FCPXML

- One asset referencing the original MKV, one sequence whose spine contains an `asset-clip`
  per keep segment (`offset`, `start`, `duration` as frame-aligned rational times).
- All audio streams stay attached to each clip, so the 3 tracks remain separate in Resolve.
- `src` is a percent-encoded `file://` URI. All XML text and attributes must be escaped
  (use a proper XML writer, no string concatenation of raw values).
- **Path mapping:** `--path-map FROM=TO` rewrites the path prefix in `src`
  (e.g. record on Linux, edit on macOS). Repeatable. Validate the `FROM=TO` format.
- Put export behind a trait (e.g. `TimelineExporter`) with FCPXML as the only v1
  implementation. Future exporters (FCP7 XML for Premiere, OTIO) must be addable without
  touching detection code.

**Early risk:** confirm that Resolve imports the generated file with video + all 3 audio
tracks and correct cut positions. Produce a minimal FCPXML against a real recording as the
first milestone deliverable, before polishing anything else.

## CLI sketch

```text
quietcut analyze <input.mkv>              # print resolved streams, silences, keep segments
quietcut export  <input.mkv> -o <out.fcpxml>
    [--mic-name NAME | --mic-stream N]
    [--discord-name NAME | --discord-stream N]
    [--mic-threshold DB] [--discord-threshold DB]
    [--min-silence SEC] [--padding SEC]
    [--path-map FROM=TO]...
    [--force]                               # overwrite existing output
```

- Optional TOML config for persistent defaults (names, thresholds, path maps) in the
  platform config dir; CLI flags override config.
- Validate all numeric inputs (thresholds negative dB, durations positive, padding < min_silence).
- Refuse to overwrite an existing output unless `--force`.
- CLI output language: English (no target user language stated).

## Testing

- Unit: silencedetect stderr parser (fixture strings), interval intersection/complement/padding,
  frame snapping, path mapping, XML escaping.
- Integration: generate a synthetic MKV with ffmpeg (`lavfi` tones and silence on 3 audio
  streams with titles), run `export`, assert segment boundaries. Skip with a message if
  ffmpeg is unavailable.
- Manual: run once with networking disabled (`unshare -rn`) to prove offline operation.

## Non-goals for v1

Re-encoding or rendering, GUI, transcription, chapter generation, Adobe/OTIO export,
per-segment track muting.

## Milestones

1. Stream probing + silence detection + `analyze` output on a real recording.
2. Minimal FCPXML export, verified by importing into Resolve.
3. Config file, path mapping, validation polish, tests.
