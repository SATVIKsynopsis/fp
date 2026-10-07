use crate::error::AppError;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{path::Path, process::Command};

#[derive(Debug, Deserialize)]
pub struct Probe {
    #[serde(default)]
    pub streams: Vec<Stream>,
    #[serde(default)]
    pub format: Format,
}

#[derive(Debug, Default, Deserialize)]
pub struct Format {
    pub duration: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Stream {
    pub codec_type: Option<String>,
    pub codec_name: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bit_rate: Option<String>,
    pub channels: Option<u32>,
}

impl Probe {
    pub fn duration(&self) -> Result<f64> {
        self.format
            .duration
            .as_deref()
            .context("duration is unavailable")?
            .parse()
            .context("invalid media duration")
    }
    pub fn video(&self) -> Option<&Stream> {
        self.streams
            .iter()
            .find(|s| s.codec_type.as_deref() == Some("video"))
    }
    pub fn audio(&self) -> Option<&Stream> {
        self.streams
            .iter()
            .find(|s| s.codec_type.as_deref() == Some("audio"))
    }
}

pub fn inspect(path: &Path) -> Result<Probe> {
    if !path.is_file() {
        return Err(AppError::MissingInput(path.display().to_string()).into());
    }
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                AppError::FfprobeMissing
            } else {
                AppError::Unsupported(e.to_string())
            }
        })?;
    if !output.status.success() {
        return Err(AppError::Unsupported("ffprobe could not read the file".into()).into());
    }
    let result: Probe =
        serde_json::from_slice(&output.stdout).context("invalid ffprobe response")?;
    if result.video().is_none() {
        return Err(AppError::NoVideo.into());
    }
    Ok(result)
}
