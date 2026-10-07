use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
};

struct Fixtures {
    _directory: tempfile::TempDir,
    audio_video: PathBuf,
    silent_video: PathBuf,
}

static FIXTURES: OnceLock<Option<Fixtures>> = OnceLock::new();

fn has_command(program: &str) -> bool {
    Command::new(program)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn fixtures() -> Option<&'static Fixtures> {
    FIXTURES
        .get_or_init(|| {
            if !has_command("ffmpeg") || !has_command("ffprobe") {
                eprintln!("skipping FFmpeg integration tests: ffmpeg and ffprobe are required");
                return None;
            }
            let directory = tempfile::tempdir().expect("create media fixture directory");
            let audio_video = directory.path().join("deterministic-av.mp4");
            let silent_video = directory.path().join("deterministic-video.mp4");
            let common_video = [
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x120:rate=10:duration=1.5",
            ];
            let audio_result = Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error"])
                .args(common_video)
                .args([
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=1000:sample_rate=44100:duration=1.5",
                    "-map",
                    "0:v:0",
                    "-map",
                    "1:a:0",
                    "-c:v",
                    "libx264",
                    "-threads",
                    "1",
                    "-preset",
                    "ultrafast",
                    "-crf",
                    "22",
                    "-c:a",
                    "aac",
                    "-b:a",
                    "64k",
                    "-shortest",
                    "-y",
                ])
                .arg(&audio_video)
                .output()
                .expect("run ffmpeg to generate A/V fixture");
            assert!(
                audio_result.status.success(),
                "could not create A/V fixture: {}",
                String::from_utf8_lossy(&audio_result.stderr)
            );
            let silent_result = Command::new("ffmpeg")
                .args(["-hide_banner", "-loglevel", "error"])
                .args(common_video)
                .args([
                    "-c:v",
                    "libx264",
                    "-threads",
                    "1",
                    "-preset",
                    "ultrafast",
                    "-crf",
                    "22",
                    "-an",
                    "-y",
                ])
                .arg(&silent_video)
                .output()
                .expect("run ffmpeg to generate silent fixture");
            assert!(
                silent_result.status.success(),
                "could not create silent fixture: {}",
                String::from_utf8_lossy(&silent_result.stderr)
            );
            Some(Fixtures {
                _directory: directory,
                audio_video,
                silent_video,
            })
        })
        .as_ref()
}

fn invoke(input: &Path, target_bytes: u64, output: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tool"))
        .arg("compress")
        .arg(input)
        .arg("--size")
        .arg(format!("{target_bytes}B"))
        .arg("--output")
        .arg(output)
        .output()
        .expect("run tool CLI")
}

fn assert_output_fits(output: &Path, target: u64) {
    let output_size = fs::metadata(output)
        .unwrap_or_else(|error| panic!("expected output {}: {error}", output.display()))
        .len();
    assert!(
        output_size <= target,
        "output was {output_size} bytes, target was {target} bytes"
    );
}

fn stream_types(path: &Path) -> Vec<String> {
    let result = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .expect("run ffprobe on compressed output");
    assert!(result.status.success());
    String::from_utf8_lossy(&result.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn test_directory() -> tempfile::TempDir {
    tempfile::tempdir().expect("create CLI test directory")
}

#[test]
fn input_smaller_than_target_skips_compression() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let output = work.path().join("must-not-exist.mp4");
    let result = invoke(&media.audio_video, u64::MAX, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("No compression needed."));
    assert!(!output.exists());
}

#[test]
fn input_equal_to_target_skips_compression() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let output = work.path().join("must-not-exist.mp4");
    let input_size = fs::metadata(&media.audio_video).unwrap().len();
    let result = invoke(&media.audio_video, input_size, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("No compression needed."));
    assert!(!output.exists());
}

#[test]
fn normal_compression_succeeds_under_target() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let input = work.path().join("normal-input.mp4");
    fs::copy(&media.audio_video, &input).unwrap();
    let output = work.path().join("normal-input-compressed.mp4");
    let input_size = fs::metadata(&input).unwrap().len();
    let target = input_size * 9 / 10;
    let result = Command::new(env!("CARGO_BIN_EXE_tool"))
        .args(["compress"])
        .arg(&input)
        .arg("--size")
        .arg(format!("{target}B"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_output_fits(&output, target);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("Best candidate found under"));
    assert!(!stdout.contains("Maximum file size"));
    assert!(!stdout.contains("Output file (press Enter"));
}

#[test]
fn aggressive_compression_succeeds_under_target() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let output = work.path().join("aggressive.mp4");
    let input_size = fs::metadata(&media.audio_video).unwrap().len();
    let target = input_size * 3 / 5;
    let result = invoke(&media.audio_video, target, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_output_fits(&output, target);
}

#[test]
fn video_with_audio_keeps_an_audio_stream_and_fits_target() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let output = work.path().join("with-audio.mp4");
    let input_size = fs::metadata(&media.audio_video).unwrap().len();
    let target = input_size * 9 / 10;
    let result = invoke(&media.audio_video, target, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_output_fits(&output, target);
    assert!(stream_types(&output).iter().any(|kind| kind == "audio"));
}

#[test]
fn video_without_audio_stays_silent_and_fits_target() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let output = work.path().join("silent.mp4");
    let input_size = fs::metadata(&media.silent_video).unwrap().len();
    let target = input_size * 4 / 5;
    let result = invoke(&media.silent_video, target, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_output_fits(&output, target);
    assert!(!stream_types(&output).iter().any(|kind| kind == "audio"));
}

#[test]
fn successful_output_never_exceeds_requested_maximum() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let output = work.path().join("bounded.mp4");
    let target = fs::metadata(&media.audio_video).unwrap().len() * 9 / 10;
    let result = invoke(&media.audio_video, target, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_output_fits(&output, target);
}

#[test]
fn existing_output_is_not_overwritten() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let output = work.path().join("existing.mp4");
    fs::write(&output, b"keep this file").unwrap();
    let target = fs::metadata(&media.audio_video).unwrap().len() / 2;
    let result = invoke(&media.audio_video, target, &output);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("output file already exists"));
    assert_eq!(fs::read(&output).unwrap(), b"keep this file");
}

#[cfg(unix)]
#[test]
fn failed_encoding_leaves_no_final_output_or_candidate_directory() {
    use std::os::unix::fs::PermissionsExt;

    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let fake_bin = work.path().join("bin");
    fs::create_dir(&fake_bin).unwrap();
    let fake_ffmpeg = fake_bin.join("ffmpeg");
    fs::write(&fake_ffmpeg, "#!/bin/sh\nexit 17\n").unwrap();
    let mut permissions = fs::metadata(&fake_ffmpeg).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_ffmpeg, permissions).unwrap();

    let output_dir = work.path().join("outputs");
    fs::create_dir(&output_dir).unwrap();
    let output = output_dir.join("failed.mp4");
    let target = fs::metadata(&media.audio_video).unwrap().len() / 2;
    let mut paths = vec![fake_bin];
    if let Some(path) = env::var_os("PATH") {
        paths.extend(env::split_paths(&path));
    }
    let path = env::join_paths(paths).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_tool"))
        .args(["compress"])
        .arg(&media.audio_video)
        .arg("--size")
        .arg(format!("{target}B"))
        .arg("-o")
        .arg(&output)
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("FFmpeg failed on attempt"));
    assert!(!output.exists());
    assert_eq!(fs::read_dir(&output_dir).unwrap().count(), 0);
}

#[test]
fn dry_run_shows_initial_plan_without_running_or_modifying_destination() {
    let Some(media) = fixtures() else { return };
    let work = test_directory();
    let output = work.path().join("dry-run.mp4");
    fs::write(&output, b"existing destination").unwrap();
    let target = fs::metadata(&media.audio_video).unwrap().len() / 2;
    let result = Command::new(env!("CARGO_BIN_EXE_tool"))
        .args(["compress"])
        .arg(&media.audio_video)
        .arg("--size")
        .arg(format!("{target}B"))
        .arg("--dry-run")
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("Dry run:"));
    assert!(stdout.contains("Initial candidate: CRF"));
    assert!(stdout.contains("no encoding or output file will be created"));
    assert!(stdout.contains("-crf"));
    assert_eq!(fs::read(&output).unwrap(), b"existing destination");
}
