use crate::probe::ffprobe::Probe;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub const MIN_CRF: u8 = 18;
pub const MAX_CRF: u8 = 51;
pub const MAX_ATTEMPTS: usize = 5;
const SIZE_HEADROOM: f64 = 0.88;
const DEFAULT_AUDIO_BITRATE: u64 = 96_000;

pub fn parse_size(input: &str) -> Result<u64> {
    let value = input.trim().to_ascii_lowercase();
    let split = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let number = &value[..split];
    let unit = value[split..].trim();
    let multiplier: u128 = match unit {
        "b" | "byte" | "bytes" => 1,
        "k" | "kb" => 1_000,
        "m" | "mb" => 1_000_000,
        "g" | "gb" => 1_000_000_000,
        "kib" => 1 << 10,
        "mib" => 1 << 20,
        "gib" => 1 << 30,
        _ => bail!("invalid target size '{input}'. Use B, KB, MB, or GB"),
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if number.matches('.').count() > 1
        || (!whole.is_empty() && !whole.bytes().all(|b| b.is_ascii_digit()))
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (whole.is_empty() && fraction.is_empty())
    {
        bail!("invalid target size '{input}'. Try 8 MB, 500 KB, or 1.5 GB");
    }
    let scale = 10_u128
        .checked_pow(fraction.len() as u32)
        .ok_or_else(|| anyhow::anyhow!("invalid target size '{input}'"))?;
    let whole = if whole.is_empty() {
        0
    } else {
        whole
            .parse::<u128>()
            .map_err(|_| anyhow::anyhow!("invalid target size '{input}'"))?
    };
    let fractional = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u128>()
            .map_err(|_| anyhow::anyhow!("invalid target size '{input}'"))?
    };
    let numerator = whole
        .checked_mul(scale)
        .and_then(|v| v.checked_add(fractional))
        .and_then(|v| v.checked_mul(multiplier))
        .ok_or_else(|| anyhow::anyhow!("target size '{input}' is too large"))?;
    let bytes = numerator / scale;
    if bytes == 0 || bytes > u64::MAX as u128 {
        bail!(
            "target size '{input}' must be between 1 byte and {} bytes",
            u64::MAX
        );
    }
    Ok(bytes as u64)
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub duration: f64,
    pub target_bytes: u64,
    pub audio_bitrate: u64,
    pub estimated_video_bitrate: u64,
    pub quality: QualityEstimate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityLevel {
    VeryLow,
    Low,
    Good,
    VeryGood,
}

impl QualityLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::VeryLow => "Very low",
            Self::Low => "Low",
            Self::Good => "Good",
            Self::VeryGood => "Very good",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QualityEstimate {
    pub level: QualityLevel,
    pub recommendations: [u64; 3],
}

#[derive(Debug)]
pub struct QualitySearch {
    target_bytes: u64,
    attempts: usize,
    best: Option<(u8, u64)>,
    samples: Vec<(u8, u64)>,
}

impl QualitySearch {
    pub fn new(target_bytes: u64) -> Self {
        Self {
            target_bytes,
            attempts: 0,
            best: None,
            samples: Vec::new(),
        }
    }

    pub fn next_crf(&self) -> Option<u8> {
        if self.attempts >= MAX_ATTEMPTS {
            None
        } else {
            let oversized = self
                .samples
                .iter()
                .filter(|(_, size)| *size > self.target_bytes)
                .max_by_key(|(crf, _)| *crf)
                .copied();
            let fitting = self
                .samples
                .iter()
                .filter(|(_, size)| *size <= self.target_bytes)
                .min_by_key(|(crf, _)| *crf)
                .copied();

            match (oversized, fitting) {
                (Some((over_crf, over_size)), Some((fit_crf, fit_size))) => {

                    let predicted = interpolate_crf(
                        (over_crf, over_size),
                        (fit_crf, fit_size),
                        self.target_bytes,
                    );
                    self.pick_untried(
                        over_crf.saturating_add(1),
                        fit_crf.saturating_sub(1),
                        predicted,
                    )
                }
                (Some(_), None) => {
                    let mut points: Vec<_> = self
                        .samples
                        .iter()
                        .filter(|(_, size)| *size > self.target_bytes)
                        .copied()
                        .collect();
                    points.sort_by_key(|(crf, _)| *crf);
                    let (base_crf, predicted) = if points.len() >= 2 {
                        let a = points[points.len() - 2];
                        let b = points[points.len() - 1];
                        (b.0, interpolate_crf(a, b, self.target_bytes))
                    } else {
                        let (crf, bytes) = points[0];
                        (crf, estimate_single_sample(crf, bytes, self.target_bytes))
                    };
                    if base_crf < MAX_CRF {
                        self.pick_untried(base_crf + 1, MAX_CRF, predicted)
                    } else {
                        None
                    }
                }
                (None, Some(_)) => {
                    let mut points: Vec<_> = self
                        .samples
                        .iter()
                        .filter(|(_, size)| *size <= self.target_bytes)
                        .copied()
                        .collect();
                    points.sort_by_key(|(crf, _)| *crf);
                    let (base_crf, predicted) = if points.len() >= 2 {
                        let a = points[0];
                        let b = points[1];
                        (a.0, interpolate_crf(a, b, self.target_bytes))
                    } else {
                        let (crf, bytes) = points[0];
                        (crf, estimate_single_sample(crf, bytes, self.target_bytes))
                    };
                    if base_crf > MIN_CRF {
                        self.pick_untried(MIN_CRF, base_crf - 1, predicted)
                    } else {
                        None
                    }
                }
                (None, None) => Some((MIN_CRF + MAX_CRF) / 2),
            }
        }
    }

    pub fn record(&mut self, crf: u8, output_bytes: u64) {
        if self.was_tested(crf) {
            return;
        }
        self.attempts += 1;
        self.samples.push((crf, output_bytes));
        if output_bytes <= self.target_bytes && self.best.is_none_or(|(best_crf, _)| crf < best_crf)
        {
            self.best = Some((crf, output_bytes));
        }
    }

    pub fn best(&self) -> Option<(u8, u64)> {
        self.best
    }
    pub fn attempts(&self) -> usize {
        self.attempts
    }

    fn was_tested(&self, crf: u8) -> bool {
        self.samples.iter().any(|(tested, _)| *tested == crf)
    }

    fn pick_untried(&self, low: u8, high: u8, preferred: Option<u8>) -> Option<u8> {
        if low > high {
            return None;
        }
        let preferred = preferred.unwrap_or((low + high) / 2).clamp(low, high);
        (low..=high)
            .filter(|crf| !self.was_tested(*crf))
            .min_by_key(|crf| crf.abs_diff(preferred))
    }
}

fn interpolate_crf(a: (u8, u64), b: (u8, u64), target: u64) -> Option<u8> {
    if a.0 == b.0 || a.1 == 0 || b.1 == 0 || target == 0 {
        return None;
    }
    let start = (a.1 as f64).ln();
    let end = (b.1 as f64).ln();
    let target = (target as f64).ln();
    let fraction = (target - start) / (end - start);
    let estimate = a.0 as f64 + fraction * (b.0 as f64 - a.0 as f64);
    (estimate.is_finite() && estimate >= MIN_CRF as f64 && estimate <= MAX_CRF as f64)
        .then_some(estimate.round() as u8)
}

fn estimate_single_sample(crf: u8, bytes: u64, target: u64) -> Option<u8> {
    if bytes == 0 || target == 0 {
        return None;
    }
    let delta = 6.0 * (bytes as f64 / target as f64).log2();
    let estimate = crf as f64 + delta;
    estimate
        .is_finite()
        .then_some(estimate.round().clamp(MIN_CRF as f64, MAX_CRF as f64) as u8)
}

pub fn already_at_or_below_target(input_bytes: u64, target_bytes: u64) -> bool {
    input_bytes <= target_bytes
}

fn audio_bitrate(media: &Probe, duration: f64, target_bytes: u64) -> u64 {
    if media.audio().is_none() {
        return 0;
    }
    let fallback = match media.audio().and_then(|a| a.channels).unwrap_or(2) {
        1 => 64_000,
        2 => DEFAULT_AUDIO_BITRATE,
        _ => 128_000,
    };
    let source = media
        .audio()
        .and_then(|a| a.bit_rate.as_deref())
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(fallback);

    if (target_bytes as f64 * 8.0 / duration) < 400_000.0 {
        source.min(96_000)
    } else {
        source.min(192_000)
    }
}

pub fn plan(media: &Probe, target_bytes: u64) -> Result<Plan> {
    let duration = media.duration()?;
    if !duration.is_finite() || duration <= 0.0 {
        bail!("media duration must be greater than zero");
    }
    let audio_bitrate = audio_bitrate(media, duration, target_bytes);
    let safe_total_bitrate = (target_bytes as f64 * 8.0 * SIZE_HEADROOM / duration) as u64;
    let estimated_video_bitrate = safe_total_bitrate.saturating_sub(audio_bitrate);
    let pixels = media.video().and_then(|v| v.width).unwrap_or(854) as f64
        * media.video().and_then(|v| v.height).unwrap_or(480) as f64;
    let scale = (pixels / (854.0 * 480.0)).clamp(0.5, 4.0);
    let level = if estimated_video_bitrate < (220_000.0 * scale) as u64 {
        QualityLevel::VeryLow
    } else if estimated_video_bitrate < (400_000.0 * scale) as u64 {
        QualityLevel::Low
    } else if estimated_video_bitrate < (700_000.0 * scale) as u64 {
        QualityLevel::Good
    } else {
        QualityLevel::VeryGood
    };
    let recommendation = |video_rate: u64| {
        ((video_rate + audio_bitrate) as f64 * duration / (8.0 * SIZE_HEADROOM)).ceil() as u64
    };
    let quality = QualityEstimate {
        level,
        recommendations: [
            recommendation((250_000.0 * scale) as u64),
            recommendation((500_000.0 * scale) as u64),
            recommendation((800_000.0 * scale) as u64),
        ],
    };
    Ok(Plan {
        duration,
        target_bytes,
        audio_bitrate,
        estimated_video_bitrate,
        quality,
    })
}

pub fn output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    let extension = input
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .unwrap_or("mp4");
    input
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}-compressed.{extension}"))
}

pub fn arguments(input: &Path, output: &Path, crf: u8, audio_bitrate: u64) -> Vec<String> {
    let mut args = vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-n".into(),
        "-i".into(),
        input.display().to_string(),
        "-map".into(),
        "0:v:0".into(),
    ];
    if audio_bitrate > 0 {
        args.extend([
            "-map".into(),
            "0:a:0?".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            format!("{}k", audio_bitrate / 1000),
        ]);
    } else {
        args.extend(["-an".into()]);
    }
    args.extend([
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "medium".into(),
        "-crf".into(),
        crf.to_string(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-movflags".into(),
        "+faststart".into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        output.display().to_string(),
    ]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("8mb").unwrap(), 8_000_000);
        assert_eq!(parse_size("1.5 GB").unwrap(), 1_500_000_000);
        assert_eq!(parse_size("500 KB").unwrap(), 500_000);
        assert_eq!(parse_size("1 KiB").unwrap(), 1_024);
        assert_eq!(parse_size("0.1 KB").unwrap(), 100);
        assert_eq!(parse_size(".5 MB").unwrap(), 500_000);
    }
    #[test]
    fn rejects_bad_sizes() {
        assert!(parse_size("wat").is_err());
        assert!(parse_size("0 B").is_err());
        assert!(parse_size("1.2.3 MB").is_err());
        assert!(parse_size("18446744073709551616 B").is_err());
    }

    #[test]
    fn byte_size_parser_round_trips_integer_boundaries() {
        let values = [1, 2, 999, 1_000, 1_048_576, u32::MAX as u64, u64::MAX];
        for bytes in values {
            assert_eq!(parse_size(&format!("{bytes} B")).unwrap(), bytes);
        }
    }
    #[test]
    fn input_size_boundary_is_inclusive() {
        assert!(already_at_or_below_target(3_300_000, 8_000_000));
        assert!(already_at_or_below_target(8_000_000, 8_000_000));
        assert!(!already_at_or_below_target(8_000_001, 8_000_000));
    }
    fn fixture() -> Probe {
        serde_json::from_str(r#"{"format":{"duration":"296","size":"45600000"},"streams":[{"codec_type":"video","codec_name":"h264","width":854,"height":480},{"codec_type":"audio","bit_rate":"192000","channels":2}]}"#).unwrap()
    }
    #[test]
    fn crf_search_selects_best_fit_and_rejects_oversize() {
        let mut search = QualitySearch::new(8_000_000);
        let first = search.next_crf().unwrap();
        assert_eq!(first, (MIN_CRF + MAX_CRF) / 2);
        search.record(first, 12_000_000);
        assert!(search.best().is_none());
        let second = search.next_crf().unwrap();
        assert_eq!(second, 38);
        search.record(second, 9_000_000);
        let third = search.next_crf().unwrap();
        assert_eq!(third, 40);
        search.record(third, 7_900_000);
        let fourth = search.next_crf().unwrap();
        assert!(fourth < third && fourth > second);
        search.record(fourth, 8_100_000);
        assert_eq!(search.best(), Some((third, 7_900_000)));
        assert_eq!(search.attempts(), 4);
        assert!(search.next_crf().is_none());
    }
    #[test]
    fn crf_bounds_interpolation_and_single_sample_estimates_are_safe() {
        let estimate = interpolate_crf((34, 12_000_000), (51, 5_000_000), 8_000_000).unwrap();
        assert!((MIN_CRF..=MAX_CRF).contains(&estimate));
        assert!(interpolate_crf((34, 0), (51, 5_000_000), 8_000_000).is_none());
        assert!(interpolate_crf((34, 9_000_000), (34, 5_000_000), 8_000_000).is_none());
        assert_eq!(estimate_single_sample(18, u64::MAX, 1), Some(MAX_CRF));
        assert_eq!(estimate_single_sample(51, 0, 1), None);
    }

    #[test]
    fn search_handles_extreme_targets_and_measured_sizes_without_duplicate_crfs() {
        for target in [1, u64::MAX] {
            let mut search = QualitySearch::new(target);
            let mut seen = Vec::new();
            while let Some(crf) = search.next_crf() {
                assert!((MIN_CRF..=MAX_CRF).contains(&crf));
                assert!(!seen.contains(&crf));
                seen.push(crf);
                let measured = if seen.len() == 1 {
                    u64::MAX
                } else if seen.len() == 2 {
                    0
                } else {
                    u64::MAX / seen.len() as u64
                };
                search.record(crf, measured);
            }
            assert!(seen.len() <= MAX_ATTEMPTS);
            assert_eq!(search.attempts(), seen.len());
        }
    }
    #[test]
    fn estimate_flags_aggressive_target_and_recommends_more() {
        let plan = plan(&fixture(), 8_000_000).unwrap();
        assert_eq!(plan.audio_bitrate, 96_000);
        assert_eq!(plan.quality.level, QualityLevel::VeryLow);
        assert!(plan.quality.recommendations[0] > 8_000_000);
        assert!(plan.estimated_video_bitrate < 150_000);
    }
    #[test]
    fn bitrate_estimate_reserves_audio_and_size_headroom() {
        let target = 8_000_000;
        let plan = plan(&fixture(), target).unwrap();
        let safe_total = (target as f64 * 8.0 * SIZE_HEADROOM / plan.duration) as u64;
        assert_eq!(plan.audio_bitrate, 96_000);
        assert_eq!(
            plan.estimated_video_bitrate,
            safe_total.saturating_sub(plan.audio_bitrate)
        );
        assert!(plan.estimated_video_bitrate < safe_total);
    }

    #[test]
    fn maximum_target_does_not_overflow_quality_estimates() {
        let plan = plan(&fixture(), u64::MAX).unwrap();
        assert_eq!(plan.audio_bitrate, 192_000);
        assert_eq!(plan.quality.level, QualityLevel::VeryGood);
        assert!(plan.quality.recommendations.iter().all(|size| *size > 0));
    }
    #[test]
    fn recommendation_budget_includes_container_safety_margin() {
        let plan = plan(&fixture(), 8_000_000).unwrap();
        let raw_low_quality_bytes = (250_000_u64 + plan.audio_bitrate) as f64 * plan.duration / 8.0;
        assert!(plan.quality.recommendations[0] as f64 > raw_low_quality_bytes);
    }
    #[test]
    fn audio_budget_respects_mono_and_silent_inputs() {
        let mono: Probe = serde_json::from_str(r#"{"format":{"duration":"60"},"streams":[{"codec_type":"video"},{"codec_type":"audio","channels":1}]}"#).unwrap();
        let silent: Probe = serde_json::from_str(
            r#"{"format":{"duration":"60"},"streams":[{"codec_type":"video"}]}"#,
        )
        .unwrap();
        assert_eq!(plan(&mono, 10_000_000).unwrap().audio_bitrate, 64_000);
        assert_eq!(plan(&silent, 10_000_000).unwrap().audio_bitrate, 0);
        assert_eq!(plan(&fixture(), 25_000_000).unwrap().audio_bitrate, 192_000);
    }
    #[test]
    fn very_small_targets_are_warned_about_but_not_blocked_by_planner() {
        assert_eq!(
            plan(&fixture(), 100_000).unwrap().quality.level,
            QualityLevel::VeryLow
        );
    }
    #[test]
    fn makes_output_name() {
        assert_eq!(
            output_path(Path::new("/tmp/clip.mov")),
            PathBuf::from("/tmp/clip-compressed.mov")
        );
        assert_eq!(
            output_path(Path::new("/tmp/clip")),
            PathBuf::from("/tmp/clip-compressed.mp4")
        );
    }
    #[test]
    fn generates_quality_arguments() {
        let args = arguments(Path::new("a b.mp4"), Path::new("out.mp4"), 29, 96_000);
        assert!(args.windows(2).any(|w| w == ["-i", "a b.mp4"]));
        assert!(args.windows(2).any(|w| w == ["-crf", "29"]));
        assert!(args.windows(2).any(|w| w == ["-b:a", "96k"]));
        assert!(args.contains(&"pipe:1".into()));
    }
}
