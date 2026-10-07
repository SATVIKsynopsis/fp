use crate::planner::compression::{self, Plan};
use anyhow::Result;
use dialoguer::{Input, Select};
use std::path::{Path, PathBuf};

pub fn choose_size() -> Result<u64> {
    let presets = ["10 MB", "25 MB", "50 MB", "100 MB", "Custom"];
    let choice = Select::new()
        .with_prompt("Maximum file size?")
        .items(&presets)
        .default(1)
        .interact()?;
    let size = if choice == 4 {
        Input::<String>::new()
            .with_prompt("Enter a maximum size (for example 8mb, 25mb, 500mb, 1gb)")
            .validate_with(|input: &String| validate_custom_size(input))
            .interact_text()?
    } else {
        presets[choice].to_string()
    };
    parse_custom_size(&size)
}

pub fn choose_output_path(default_path: &Path, allow_existing: bool) -> Result<PathBuf> {
    let default = default_path.to_string_lossy().into_owned();
    let path = Input::<String>::new()
        .with_prompt("Output file? Press Enter to use the default")
        .default(default)
        .show_default(true)
        .validate_with(|input: &String| validate_output_path(input, allow_existing))
        .interact_text()?;
    Ok(PathBuf::from(path))
}

fn parse_custom_size(input: &str) -> Result<u64> {
    compression::parse_size(input)
}

fn validate_custom_size(input: &str) -> std::result::Result<(), String> {
    if input.trim().is_empty() {
        return Err("enter a size such as 8mb, 500kb, or 1gb".into());
    }
    parse_custom_size(input)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn validate_output_path(input: &str, allow_existing: bool) -> std::result::Result<(), String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("enter an output filename".into());
    }
    let path = Path::new(input);
    if !allow_existing && path.exists() {
        return Err(format!(
            "{} already exists; choose another output file",
            path.display()
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(format!(
            "output directory {} does not exist",
            parent.display()
        ));
    }
    Ok(())
}

pub fn confirm() -> Result<bool> {
    let choices = ["Yes", "No"];
    let choice = Select::new()
        .with_prompt("Start compression?")
        .items(&choices)
        .default(0)
        .interact()?;
    Ok(confirm_choice(choice))
}

fn confirm_choice(choice: usize) -> bool {
    choice == 0
}
pub fn confirm_compress_anyway() -> Result<bool> {
    let options = ["Keep original", "Compress anyway"];
    Ok(Select::new()
        .with_prompt("The input is already at or below the target. What would you like to do?")
        .items(&options)
        .default(0)
        .interact()?
        == 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggressiveChoice {
    Continue,
    Recommended,
    Cancel,
}

pub fn aggressive_target_choice(plan: &Plan, width: u32, height: u32) -> Result<AggressiveChoice> {
    println!(
        "\n⚠ {} is a very aggressive target for this video.\nSignificant quality loss may occur at this size.\n\nDuration:    {}\nResolution:  {}x{}\nTarget:      {}\n\nEstimated quality at this target: {}\n\nRecommended sizes:\n  {}  Low quality\n  {}  Good quality\n  {}  Very good quality",
        human_size(plan.target_bytes),
        duration(plan.duration),
        width,
        height,
        human_size(plan.target_bytes),
        plan.quality.level.label(),
        human_size(plan.quality.recommendations[0]),
        human_size(plan.quality.recommendations[1]),
        human_size(plan.quality.recommendations[2])
    );
    let choices = ["Continue anyway", "Use recommended size", "Cancel"];
    match Select::new()
        .with_prompt("Choose how to continue")
        .items(&choices)
        .default(0)
        .interact()?
    {
        0 => Ok(AggressiveChoice::Continue),
        1 => Ok(AggressiveChoice::Recommended),
        _ => Ok(AggressiveChoice::Cancel),
    }
}
pub fn human_size(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}
pub fn duration(seconds: f64) -> String {
    format!("{}:{:02}", seconds as u64 / 60, seconds as u64 % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_custom_target_sizes_with_the_shared_parser() {
        for size in ["8mb", "25mb", "500mb", "1gb", "1.5 GB"] {
            assert!(parse_custom_size(size).is_ok(), "{size}");
        }
        for size in ["", "banana", "-8mb", "0mb", "18446744073709551616b", "8mxb"] {
            assert!(validate_custom_size(size).is_err(), "{size}");
        }
        assert!(
            validate_custom_size("")
                .unwrap_err()
                .contains("enter a size")
        );
        assert!(
            validate_custom_size("18446744073709551616b")
                .unwrap_err()
                .contains("between 1 byte")
        );
    }

    #[test]
    fn output_validation_rejects_existing_or_missing_parent_paths() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("exists.mp4");
        std::fs::write(&existing, b"x").unwrap();
        assert!(validate_output_path(existing.to_str().unwrap(), false).is_err());
        assert!(validate_output_path(existing.to_str().unwrap(), true).is_ok());
        assert!(
            validate_output_path(dir.path().join("missing/out.mp4").to_str().unwrap(), false)
                .is_err()
        );
        assert!(validate_output_path(dir.path().join("new.mp4").to_str().unwrap(), false).is_ok());
    }

    #[test]
    fn confirmation_no_choice_does_not_start() {
        assert!(confirm_choice(0));
        assert!(!confirm_choice(1));
    }
}
