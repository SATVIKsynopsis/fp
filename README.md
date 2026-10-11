# fp

**Give it a size. It figures out the compression.**


https://github.com/user-attachments/assets/2d6f609a-dc4c-4168-8f86-0d0eafe98aef


Need a video under 25 MB for Discord, email, or an upload form? Skip the bitrate math and the FFmpeg flags:

```
fp compress video.mp4 --size 25mb
```

`fp` tests a few quality settings, measures the real output size of each and keeps the best one that fits under your limit.

## Example

A real run on a 1080p, 4:57 video:

```
$ fp compress without-me.mp4 --size 25mb
Analyzing video...

Input:      without-me.mp4
Original:   99.8 MB
Duration:   4:57
Resolution: 1920x1080
Target max: 25.0 MB
Audio:      128 kbps AAC
Estimated video bitrate: 463 kbps (Very low)

Finding the best candidate under 25.0 MB...
Attempt 1/5: CRF 34 → 48.5 MB (over target)
Attempt 2/5: CRF 40 → 30.8 MB (over target)
Attempt 3/5: CRF 43 → 25.3 MB (over target)
Attempt 4/5: CRF 44 → 23.7 MB (fits)

✔ Best candidate found under 25.0 MB (CRF 44)

Original:  99.8 MB
Output:    23.7 MB
Reduction: 76.2%
```

## Install

`fp` needs **FFmpeg** (`ffmpeg` and `ffprobe` on your PATH). Install it first if you don't have it:

| System | Command |
| --- | --- |
| macOS | `brew install ffmpeg` |
| Ubuntu / Debian | `sudo apt install ffmpeg` |
| Windows | `winget install Gyan.FFmpeg` |

Check with `ffmpeg -version` and `ffprobe -version`.

### Option 1: download a prebuilt binary (no Rust needed)

Get the archive for your system from the [latest release](https://github.com/SATVIKsynopsis/fp/releases/latest):

| System | File |
| --- | --- |
| macOS, Apple Silicon (M1 and newer) | `fp-<version>-aarch64-apple-darwin.tar.gz` |
| macOS, Intel | `fp-<version>-x86_64-apple-darwin.tar.gz` |
| Linux, x86_64 | `fp-<version>-x86_64-unknown-linux-musl.tar.gz` |
| Windows, x86_64 | `fp-<version>-x86_64-pc-windows-msvc.zip` |

**macOS / Linux**
```
tar xzf fp-<version>-<target>.tar.gz
sudo mv fp-<version>-<target>/fp /usr/local/bin/
```

**macOS only:** the binary isn't signed, so macOS may block it the first time. Run this once:
```
xattr -d com.apple.quarantine /usr/local/bin/fp
```

**Windows:** unzip, then move `fp.exe` to a folder on your PATH or run it from that folder with `.\fp.exe`.

Optional: verify the download with the `.sha256` file from the release (`shasum -a 256 -c <file>.sha256` on macOS, `sha256sum -c <file>.sha256` on Linux).

Linux ARM (for example Raspberry Pi) has no prebuilt binary yet. Use Option 2.

### Option 2: with Cargo

```
cargo install ffrp
```

The crate is called `ffrp`; it installs the `fp` command.

## Usage

```
fp compress video.mp4                      # interactive: asks for size and output name
fp compress video.mp4 --size 25mb          # skip the size prompt
fp compress video.mp4 --size 25mb -o out.mp4
fp compress video.mp4 --size 25mb --dry-run
```

| Option | What it does |
| --- | --- |
| `--size SIZE` | Maximum output size. Omit it to get the interactive prompt (presets: 10, 25, 50, 100 MB, or custom). |
| `-o`, `--output` | Output path. Default: `<input-name>-compressed.<ext>` next to the input. |
| `--force` | Compress even if the input is already under the limit. |
| `--dry-run` | Show the plan and the FFmpeg command without encoding or writing anything. |
| `-v`, `--verbose` | Show FFmpeg diagnostics on stderr while encoding. |

**Sizes** accept decimals and these units, case-insensitive: `B`, `KB`, `MB`, `GB`, and `KiB`, `MiB`, `GiB`. Examples: `25mb`, `1.5 GB`, `500 KiB`, `8000000B`.

**Safety:** existing output files are never overwritten, and an oversized result is never published.

## How it works

The size you give is a **maximum**, not an exact target. `fp` reads the input with `ffprobe`, picks an initial quality (CRF) from the duration and target size, then runs a bounded set of test encodes and measures each result. It keeps the best candidate that fits.

- It does **not** try every possible setting.
- If no candidate fits, it fails instead of writing an oversized file.
- Very aggressive limits (like 25 MB for a long 1080p video) can cause visible quality loss. `fp` warns you and lets you continue.
- The CRF shown by `--dry-run` is only the starting guess; the measured encodes may choose a different one.

## Limitations

- Video compression only for now.
- Needs `ffmpeg` and `ffprobe` installed.
- The result is "best that fits", not a guaranteed exact size.

## Development

```
cargo test
```

## License

MIT.
