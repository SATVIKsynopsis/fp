# fp

`fp` compresses video to fit a user-specified maximum file size. It currently supports video compression only.

## Installation and requirements

The crates.io package is named `ffrp`; Cargo installs the `fp` command:

```sh
cargo install ffrp
```

Installing with Cargo requires Rust and Cargo. `ffmpeg` and `ffprobe` must also be installed and available in `PATH`. Check that both are available with:

```sh
ffmpeg -version
ffprobe -version
```

## Usage

Basic interactive compression:

```sh
fp compress video.mp4
```

When `--size` is omitted, `fp` asks for a maximum file size. Presets are 10 MB, 25 MB, 50 MB, and 100 MB; Custom accepts a size such as `8mb`, `500mb`, or `1gb`. When `-o` is omitted in interactive mode, `fp` asks for an output filename and offers a default.

Provide a maximum size directly to run without the size prompt:

```sh
fp compress video.mp4 --size 25mb
```

Supplying `--size` skips the size prompt. Supplying `-o` or `--output` skips the output filename prompt.

Sizes accept decimal values with these units: `B`, `byte`, `bytes`, `K`/`KB`, `M`/`MB`, and `G`/`GB`. Binary units `KiB`, `MiB`, and `GiB` are also supported. Unit matching is case-insensitive. Examples include `8000000B`, `25mb`, `1.5 GB`, and `500 KiB`.

Set a custom output path with `-o` or `--output`:

```sh
fp compress video.mp4 --size 25mb -o compressed.mp4
```

If no output path is supplied, the default is `<input-name>-compressed.<input-extension>` beside the input. For example, `video.mov` produces `video-compressed.mov`. Inputs without an extension use `.mp4`. Existing output files are never overwritten. Custom output paths may include a different directory.

Files already at or below the maximum are normally left unchanged. Use `--force` to compress anyway:

```sh
fp compress video.mp4 --size 25mb --force
```

Very aggressive maximum sizes can cause significant quality loss. `fp` warns about this and allows you to continue.

## Compression and dry run

The size is a maximum, not an exact output size. `fp` tests a bounded set of quality candidates, measures their output sizes, and selects the best candidate it finds that fits under the maximum. It does not exhaustively test every possible quality setting. If no candidate fits, compression fails without publishing an oversized output.

Use `--dry-run` to inspect the initial plan and FFmpeg command without encoding. The displayed CRF is only the initial candidate; measured encodes may lead the search to choose another. Dry-run does not create or overwrite the destination, even if it already exists:

```sh
fp compress video.mp4 --size 25mb --dry-run
```

## Options and help

- `-o`, `--output`: output path and filename.
- `--size SIZE`: maximum output size; omitting it enables the interactive size prompt.
- `--force`: compress even when the input is already at or below the maximum.
- `--dry-run`: show the initial plan and command without encoding or writing output.
- `-v`, `--verbose`: show FFmpeg diagnostics on stderr during encoding. Without it, FFmpeg stderr is suppressed; failures still report that verbose mode can show diagnostics.

```sh
fp compress video.mp4 --size 25mb --verbose
fp --help
fp compress --help
fp --version
```

## Examples

```sh
fp compress video.mp4
fp compress video.mp4 --size 25mb
fp compress video.mp4 --size 25mb -o compressed.mp4
fp compress video.mp4 --size 25mb --dry-run
fp compress video.mp4 --size 25mb --force
```
