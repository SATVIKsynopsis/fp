use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

pub fn publish_candidate(candidate: &Path, output: &Path, target_bytes: u64) -> Result<u64> {
    let candidate_bytes = fs::metadata(candidate)
        .with_context(|| format!("could not inspect candidate {}", candidate.display()))?
        .len();
    if candidate_bytes > target_bytes {
        anyhow::bail!(
            "candidate is {} bytes, above the requested maximum of {} bytes",
            candidate_bytes,
            target_bytes
        );
    }
    match fs::hard_link(candidate, output) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(crate::error::AppError::OutputExists(output.display().to_string()).into());
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!("could not safely publish output to {}", output.display())
            });
        }
    }
    let output_bytes = match fs::metadata(output) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            let _ = fs::remove_file(output);
            return Err(error)
                .with_context(|| format!("could not verify output {}", output.display()));
        }
    };
    if output_bytes > target_bytes {
        let _ = fs::remove_file(output);
        anyhow::bail!(
            "encoded output exceeded the requested maximum; no oversized output was kept"
        );
    }
    Ok(output_bytes)
}

pub struct CandidateRun {
    pub crf: u8,
    pub audio_bitrate: u64,
    pub duration: f64,
    pub attempt: usize,
    pub max_attempts: usize,
    pub verbose: bool,
}

pub fn execute_candidate(input: &Path, output: &Path, run: &CandidateRun) -> Result<u64> {
    let args = crate::planner::compression::arguments(input, output, run.crf, run.audio_bitrate);
    let mut child = Command::new("ffmpeg")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(if run.verbose {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::Error::from(crate::error::AppError::FfmpegMissing)
            } else {
                anyhow::Error::from(e)
            }
        })?;
    let stdout = child
        .stdout
        .take()
        .context("could not read FFmpeg progress")?;
    let pb = ProgressBar::new((run.duration * 1_000_000.0) as u64);
    pb.set_style(
        ProgressStyle::with_template("{msg} [{bar:28.cyan/blue}] {percent}% • ETA {eta}")?
            .progress_chars("█░"),
    );
    pb.set_message(format!(
        "Attempt {}/{} • CRF {}",
        run.attempt, run.max_attempts, run.crf
    ));
    pb.enable_steady_tick(Duration::from_millis(100));
    let progress_result: std::io::Result<()> =
        BufReader::new(stdout).lines().try_for_each(|line| {
            let line = line?;
            if let Some(value) = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<u64>().ok())
            {
                pb.set_position(value.min(pb.length().unwrap_or(value)));
            }
            Ok(())
        });
    if let Err(error) = progress_result {
        pb.abandon_with_message("Could not read FFmpeg progress");
        let _ = child.kill();
        let _ = child.wait();
        return Err(error).context("could not read FFmpeg progress; stopped the current encode");
    }
    let status = child.wait()?;
    if !status.success() {
        pb.abandon_with_message("FFmpeg failed");
        anyhow::bail!(
            "FFmpeg failed on attempt {}/{} (CRF {}). Run again with --verbose to see diagnostics.",
            run.attempt,
            run.max_attempts,
            run.crf
        );
    }
    pb.finish_and_clear();
    Ok(std::fs::metadata(output)
        .with_context(|| format!("FFmpeg did not create {}", output.display()))?
        .len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishing_rejects_oversized_candidates_without_creating_output() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("candidate.mp4");
        let output = dir.path().join("output.mp4");
        fs::write(&candidate, vec![0; 11]).unwrap();
        assert!(publish_candidate(&candidate, &output, 10).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn publishing_keeps_a_fitting_candidate_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("candidate.mp4");
        let output = dir.path().join("output.mp4");
        fs::write(&candidate, vec![0; 10]).unwrap();
        assert_eq!(publish_candidate(&candidate, &output, 10).unwrap(), 10);
        assert_eq!(fs::metadata(&output).unwrap().len(), 10);
        assert!(matches!(
            publish_candidate(&candidate, &output, 10)
                .unwrap_err()
                .downcast_ref::<crate::error::AppError>(),
            Some(crate::error::AppError::OutputExists(_))
        ));
    }
}
