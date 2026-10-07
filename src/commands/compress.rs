use crate::{
    planner::compression::{self, QualityLevel, QualitySearch},
    probe::ffprobe,
    ui::prompts::{self, AggressiveChoice},
};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn run(
    input: PathBuf,
    size: Option<String>,
    output: Option<PathBuf>,
    dry_run: bool,
    verbose: bool,
    force: bool,
) -> Result<()> {
    let interactive = size.is_none();
    println!("Analyzing video...");
    let media = ffprobe::inspect(&input)?;
    let mut target = match size {
        Some(s) => compression::parse_size(&s)?,
        None => prompts::choose_size()?,
    };
    let input_size = fs::metadata(&input)?.len();
    if interactive {
        println!(
            "\nInput:       {} ({})\nDuration:    {}\nResolution:  {}x{} • {}\nAudio:       {}\nTarget:      {}",
            input
                .file_name()
                .unwrap_or(input.as_os_str())
                .to_string_lossy(),
            prompts::human_size(input_size),
            prompts::duration(media.duration()?),
            media.video().and_then(|v| v.width).unwrap_or(0),
            media.video().and_then(|v| v.height).unwrap_or(0),
            media
                .video()
                .and_then(|v| v.codec_name.as_deref())
                .unwrap_or("unknown video codec"),
            media
                .audio()
                .and_then(|v| v.codec_name.as_deref())
                .unwrap_or("none"),
            prompts::human_size(target)
        );
    }
    if compression::already_at_or_below_target(input_size, target) && !force {
        println!(
            "\n✔ Input is already at or below the maximum size.\n\nCurrent: {}\nMaximum: {}\n\nNo compression needed.",
            prompts::human_size(input_size),
            prompts::human_size(target)
        );
        if !interactive {
            return Ok(());
        }
        if !prompts::confirm_compress_anyway()? {
            return Ok(());
        }
    }

    let mut plan = compression::plan(&media, target)?;
    if plan.quality.level == QualityLevel::VeryLow {
        if interactive {
            match prompts::aggressive_target_choice(
                &plan,
                media.video().and_then(|v| v.width).unwrap_or(0),
                media.video().and_then(|v| v.height).unwrap_or(0),
            )? {
                AggressiveChoice::Continue => {}
                AggressiveChoice::Recommended => {
                    target = plan.quality.recommendations[1];
                    plan = compression::plan(&media, target)?;
                    println!("Using recommended maximum: {}", prompts::human_size(target));
                }
                AggressiveChoice::Cancel => {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
        } else {
            eprintln!(
                "⚠ {} is a very aggressive maximum for this video. Significant quality loss may occur at this size. The CLI will still try to fit the output under the maximum (estimated video bitrate: {} kbps).",
                prompts::human_size(target),
                plan.estimated_video_bitrate / 1000
            );
        }
    }

    let output = match output {
        Some(output) => output,
        None if interactive => {
            prompts::choose_output_path(&compression::output_path(&input), dry_run)?
        }
        None => compression::output_path(&input),
    };
    if !interactive {
        println!(
            "\nInput:       {}\nOriginal:    {}\nDuration:    {}\nResolution:  {}x{}\nTarget max:  {}\nAudio:       {} kbps AAC\nEstimated video bitrate: {} kbps ({})",
            input.display(),
            prompts::human_size(input_size),
            prompts::duration(plan.duration),
            media.video().and_then(|v| v.width).unwrap_or(0),
            media.video().and_then(|v| v.height).unwrap_or(0),
            prompts::human_size(plan.target_bytes),
            plan.audio_bitrate / 1000,
            plan.estimated_video_bitrate / 1000,
            plan.quality.level.label()
        );
    }

    let mut search = QualitySearch::new(plan.target_bytes);
    let first_crf = search.next_crf().context("no CRF candidates available")?;
    if dry_run {
        let args = compression::arguments(&input, &output, first_crf, plan.audio_bitrate);
        println!(
            "\nDry run: no encoding or output file will be created. Initial candidate: CRF {}. The selected candidate may use a different CRF after measured encodes (up to {}).\n\nInitial FFmpeg arguments:\nffmpeg {}",
            first_crf,
            compression::MAX_ATTEMPTS,
            args.iter()
                .map(|s| format!("{:?}", s))
                .collect::<Vec<_>>()
                .join(" ")
        );
        return Ok(());
    }
    if interactive && !prompts::confirm()? {
        println!("Cancelled.");
        return Ok(());
    }
    if output.exists() {
        return Err(crate::error::AppError::OutputExists(output.display().to_string()).into());
    }
    println!(
        "\nFinding the best candidate under {}...",
        prompts::human_size(plan.target_bytes)
    );
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temp = tempfile::tempdir_in(parent)
        .context("could not create temporary output directory beside the destination")?;
    let mut best_path: Option<PathBuf> = None;
    while let Some(crf) = search.next_crf() {
        let attempt = search.attempts() + 1;
        let candidate = temp.path().join(format!("candidate-crf-{crf}.mp4"));
        let bytes = crate::ffmpeg::runner::execute_candidate(
            &input,
            &candidate,
            &crate::ffmpeg::runner::CandidateRun {
                crf,
                audio_bitrate: plan.audio_bitrate,
                duration: plan.duration,
                attempt,
                max_attempts: compression::MAX_ATTEMPTS,
                verbose,
            },
        )?;
        let fits = bytes <= plan.target_bytes;
        println!(
            "Attempt {attempt}/{}: CRF {crf} → {} ({})",
            compression::MAX_ATTEMPTS,
            prompts::human_size(bytes),
            if fits { "fits" } else { "over target" }
        );
        search.record(crf, bytes);
        if fits && search.best().is_some_and(|(best_crf, _)| best_crf == crf) {
            best_path = Some(candidate);
        }
    }
    let Some((best_crf, best_bytes)) = search.best() else {
        bail!(
            "no candidate fit under {} after {} of {} quality-search attempts; no output was kept. Try a larger target.",
            prompts::human_size(plan.target_bytes),
            search.attempts(),
            compression::MAX_ATTEMPTS
        );
    };
    let best_path = best_path.context("best-fitting candidate was not retained")?;
    if best_bytes > plan.target_bytes {
        bail!("internal size check failed: selected output exceeded the target");
    }
    let actual = crate::ffmpeg::runner::publish_candidate(&best_path, &output, plan.target_bytes)?;
    let reduction = if input_size > 0 {
        (1.0 - actual as f64 / input_size as f64).max(0.0) * 100.0
    } else {
        0.0
    };
    println!(
        "\n✔ Best candidate found under {} (CRF {})\n\nOriginal:    {}\nOutput:      {}\nReduction:   {:.1}%\n\nSaved to:\n{}",
        prompts::human_size(plan.target_bytes),
        best_crf,
        prompts::human_size(input_size),
        prompts::human_size(actual),
        reduction,
        output.display()
    );
    Ok(())
}
