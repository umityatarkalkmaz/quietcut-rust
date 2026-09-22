//! Subprocess helpers.
//!
//! Every external tool is invoked through `std::process::Command` with fully
//! separated arguments. No shell is ever spawned, so no argument can be
//! interpreted as shell syntax.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::error::{Error, Result};

pub const FFMPEG: &str = "ffmpeg";
pub const FFPROBE: &str = "ffprobe";

/// Runs `tool` with the given arguments and captures stdout/stderr.
///
/// stdin is closed so a subprocess can never consume the terminal input of the
/// parent process.
pub fn run_tool<I, S>(tool: &'static str, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(tool)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::NotFound => Error::ToolMissing { tool },
            _ => Error::ToolSpawn { tool, source },
        })
}

/// Fails unless both ffmpeg and ffprobe can be executed.
pub fn verify_tools_available() -> Result<()> {
    for tool in [FFMPEG, FFPROBE] {
        let output = run_tool(tool, ["-version"])?;
        if !output.status.success() {
            return Err(Error::ToolFailed {
                tool,
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
    }
    Ok(())
}

/// Decodes captured tool output, rejecting non-UTF-8 bytes.
pub fn decode_output(tool: &'static str, bytes: &[u8]) -> Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| Error::ToolOutputNotUtf8 { tool })
}

/// Validates a media input path and turns it into an absolute path.
///
/// ffmpeg interprets its input string by protocol prefix (`http:`, `concat:`,
/// ...) and treats a leading `-` as an option. Both are avoided by handing it
/// an absolute filesystem path that has been confirmed to point at a regular
/// file. Symlinks are deliberately not resolved, so the path the user typed
/// stays recognisable in the output.
pub fn validate_input_path(path: &Path) -> Result<PathBuf> {
    let metadata = std::fs::metadata(path).map_err(|source| match source.kind() {
        std::io::ErrorKind::NotFound => Error::InputNotFound {
            path: path.to_path_buf(),
        },
        _ => Error::InputPathUnresolvable {
            path: path.to_path_buf(),
            source,
        },
    })?;

    if !metadata.is_file() {
        return Err(Error::InputNotFile {
            path: path.to_path_buf(),
        });
    }

    std::path::absolute(path).map_err(|source| Error::InputPathUnresolvable {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_input_path_rejects_missing_file() {
        let error = validate_input_path(Path::new("/nonexistent/quietcut-test.mkv")).unwrap_err();
        assert!(matches!(error, Error::InputNotFound { .. }));
    }

    #[test]
    fn validate_input_path_rejects_directory() {
        let error = validate_input_path(Path::new("/")).unwrap_err();
        assert!(matches!(error, Error::InputNotFile { .. }));
    }

    #[test]
    fn validate_input_path_returns_absolute_path() {
        let resolved = validate_input_path(Path::new("Cargo.toml")).expect("Cargo.toml must exist");
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("Cargo.toml"));
    }
}
