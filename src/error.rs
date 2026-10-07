use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("input file does not exist: {0}")]
    MissingInput(String),
    #[error(
        "FFmpeg was not found. Install it with `brew install ffmpeg` (macOS), `sudo apt install ffmpeg` (Ubuntu/Debian), or `sudo dnf install ffmpeg` (Fedora)."
    )]
    FfmpegMissing,
    #[error("ffprobe was not found. Install FFmpeg; it includes ffprobe.")]
    FfprobeMissing,
    #[error("no video stream was found in this file")]
    NoVideo,
    #[error("output file already exists: {0}")]
    OutputExists(String),
    #[error("could not analyze this media file: {0}")]
    Unsupported(String),
}
