use std::path::{Path, PathBuf};
use std::process::Command;

use scyconvert_core::{Background, Ctx, Engine, Error, Result, Step};

use crate::ffmpeg_args::{ffmpeg_args, output_secs};

const VIDEO_IN: &[&str] = &["mp4", "mov", "webm", "mkv", "avi", "gif"];
const VIDEO_OUT: &[&str] = &["mp4", "mov", "webm", "mkv", "avi", "gif"];
/// Video to image grabs the first frame. WebP goes through PNG and the
/// `image` engine: FFmpeg has no WebP encoder of its own, and the bundled
/// builds leave out libwebp.
const FRAME: &[&str] = &["png", "jpeg"];
const AUDIO: &[&str] = &["mp3", "wav", "flac", "aac", "m4a", "ogg", "opus"];

/// The FFmpeg binary the engine would run, found the way every tool is:
/// `SCYCONVERT_FFMPEG`, next to the executable, then `PATH`. The desktop app uses
/// it to grab video thumbnails.
pub fn ffmpeg_path() -> Option<PathBuf> {
    crate::find_tool(&["ffmpeg"], "SCYCONVERT_FFMPEG")
}

/// Only self-contained media containers are accepted. Playlist demuxers can
/// open other local files even when network protocols are disabled.
pub const LOCAL_INPUT_ARGS: &[&str] = &[
    "-protocol_whitelist",
    "file,pipe",
    "-format_whitelist",
    "mov,mp4,m4a,3gp,3g2,mj2,matroska,webm,avi,gif,mp3,wav,flac,aac,ogg",
];

fn local_path(path: &Path) -> std::ffi::OsString {
    let mut input = std::ffi::OsString::from("file:");
    input.push(path.as_os_str());
    input
}

/// Build the desktop thumbnail invocation, shared with the local media engine.
pub fn thumbnail_command(ffmpeg: &Path, path: &Path, at: &str, width: u32) -> Command {
    let input = local_path(path);
    let mut command = Command::new(ffmpeg);
    crate::hide_console(&mut command);
    command
        .args(LOCAL_INPUT_ARGS)
        .args(["-nostdin", "-v", "error", "-ss", at, "-i"])
        .arg(input)
        .args(["-frames:v", "1", "-an", "-sn", "-vf"])
        .arg(format!("scale={width}:-2"))
        .args(["-f", "image2pipe", "-c:v", "png", "-"]);
    command
}

/// Runs the FFmpeg binary as a child process, so a crash in a codec can't take
/// the app down with it.
pub struct FfmpegEngine {
    ffmpeg: Option<PathBuf>,
    ffprobe: Option<PathBuf>,
}

impl FfmpegEngine {
    pub fn new() -> Self {
        Self {
            ffmpeg: crate::find_tool(&["ffmpeg"], "SCYCONVERT_FFMPEG"),
            ffprobe: crate::find_tool(&["ffprobe"], "SCYCONVERT_FFPROBE"),
        }
    }

    /// The input's duration for progress, or `None` when ffprobe is missing
    /// or can't tell. Only cancellation is an error.
    fn duration_us(&self, ctx: &Ctx, input: &Path) -> Result<Option<f64>> {
        let Some(ffprobe) = &self.ffprobe else {
            return Ok(None);
        };
        let mut cmd = Command::new(ffprobe);
        cmd.args(LOCAL_INPUT_ARGS)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
            ])
            .arg(local_path(input));
        let mut secs = None;
        match crate::run_tool("ffprobe", cmd, ctx, |line| {
            secs = secs.or_else(|| line.trim().parse::<f64>().ok());
        }) {
            Err(Error::Cancelled) => Err(Error::Cancelled),
            Err(_) => Ok(None),
            Ok(()) => Ok(secs.map(|s| s * 1_000_000.0)),
        }
    }
}

/// FFmpeg writes the extracted frame's (possibly scaled) pixel ratio in pHYs.
/// Read that generated PNG before decoding discards its metadata; this also
/// follows FFmpeg's chosen stream without running another subprocess.
fn png_pixel_aspect(path: &Path) -> Option<(u16, u16)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let mut signature = [0; 8];
    file.read_exact(&mut signature).ok()?;
    if signature != *b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    loop {
        let mut header = [0; 8];
        file.read_exact(&mut header).ok()?;
        let len = u32::from_be_bytes(header[..4].try_into().ok()?);
        match &header[4..] {
            b"pHYs" if len == 9 => {
                let mut data = [0; 9];
                file.read_exact(&mut data).ok()?;
                let num = u32::from_be_bytes(data[..4].try_into().ok()?);
                let den = u32::from_be_bytes(data[4..8].try_into().ok()?);
                if num == 0 || den == 0 {
                    return None;
                }
                let (mut a, mut b) = (num, den);
                while b != 0 {
                    (a, b) = (b, a % b);
                }
                let (num, den) = (num / a, den / a);
                // JFIF has only 16-bit density fields. Scaled frames can need
                // larger ratios; retain their proportions to the nearest
                // representable density instead of falling back to 1:1.
                let scale = f64::from(num.max(den)).max(f64::from(u16::MAX));
                let fit = |value| {
                    (f64::from(value) * f64::from(u16::MAX) / scale)
                        .round()
                        .max(1.) as u16
                };
                return Some((fit(num), fit(den)));
            }
            b"IDAT" | b"IEND" => return None,
            _ => {
                file.seek(SeekFrom::Current(i64::from(len) + 4)).ok()?;
            }
        }
    }
}

impl Default for FfmpegEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for FfmpegEngine {
    fn id(&self) -> &'static str {
        "ffmpeg"
    }

    fn unavailable_reason(&self) -> Option<String> {
        self.ffmpeg
            .is_none()
            .then(|| "ffmpeg not found (set SCYCONVERT_FFMPEG or install it)".into())
    }

    fn steps(&self) -> Vec<Step> {
        let video_targets: Vec<_> = VIDEO_OUT
            .iter()
            .chain(AUDIO)
            .chain(FRAME)
            .copied()
            .collect();
        crate::steps(VIDEO_IN, &video_targets)
            // GIF has no audio stream, so extraction is never meaningful.
            .filter(|step| !(step.from.id == "gif" && AUDIO.contains(&step.to.id)))
            .chain(crate::steps(AUDIO, AUDIO))
            .collect()
    }

    fn convert(&self, ctx: &Ctx, input: &Path, out_dir: &Path) -> Result<Vec<PathBuf>> {
        let ffmpeg = self.ffmpeg.as_ref().ok_or(Error::EngineUnavailable {
            engine: "ffmpeg",
            reason: "ffmpeg not found".into(),
        })?;
        let input_secs = self
            .duration_us(ctx, input)?
            .filter(|t| *t > 0.0)
            .map(|us| us / 1_000_000.0);
        let to = ctx.step.to.id;
        let args = ffmpeg_args(
            if to == "jpeg" { "png" } else { to },
            ctx.options,
            input_secs,
        )?;
        // Progress follows the output's timeline: trimmed, at the new speed.
        let total = output_secs(ctx.options, input_secs)
            .filter(|t| *t > 0.0)
            .map(|secs| secs * 1_000_000.0);
        if total.is_none() {
            ctx.indeterminate();
        }
        let output = ctx.artifact(out_dir, 0);
        if to == "gif" && matches!(ctx.options.background, Some(Background::Color(_))) {
            return Err(Error::InvalidOption(
                "a background color isn't supported for video to GIF yet".into(),
            ));
        }
        // A JPEG frame is written as PNG and encoded by the image engine, so
        // transparency gets the same background handling as any other image
        // (FFmpeg's JPEG encoder dropped it to black). A PNG frame keeps
        // FFmpeg's file, with its color and pixel-aspect tags.
        let via_png = to == "jpeg";
        let frame = out_dir.join("frame.png");
        let written = if via_png { &frame } else { &output };
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-hide_banner", "-nostdin", "-y", "-v", "error"])
            .args(LOCAL_INPUT_ARGS)
            .args(["-progress", "pipe:1", "-nostats"])
            .args(&args.input)
            .arg("-i")
            .arg(local_path(input))
            .args(&args.output)
            .arg(written);
        crate::run_tool("ffmpeg", cmd, ctx, |line| {
            let us = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<f64>().ok());
            if let (Some(us), Some(total)) = (us, total) {
                ctx.progress((us / total) as f32);
            }
        })?;
        if via_png {
            let img = image::open(&frame).map_err(|e| Error::EngineFailed {
                engine: "ffmpeg",
                message: e.to_string(),
            })?;
            let pixel_aspect = png_pixel_aspect(&frame);
            let _ = std::fs::remove_file(&frame);
            crate::image::encode_with_pixel_aspect(img, to, ctx.options, &output, pixel_aspect)?;
        } else if to == "png" {
            crate::image::background_png(&output, ctx.options)?;
        }
        Ok(vec![output])
    }
}

#[cfg(test)]
mod tests {
    use scyconvert_core::{Options, Timestamp, VideoCodec};

    use super::*;
    use crate::ffmpeg_args::video_scale;

    /// Converts a generated clip with FFmpeg, if it is installed.
    fn convert_clip(to: &str, options: &Options) -> Option<(tempfile::TempDir, PathBuf)> {
        convert_generated(to, options, "duration=1", "sine=frequency=440:duration=1")
    }

    /// Converts a clip made from lavfi `testsrc` (with `video` appended) and
    /// `audio` sources.
    fn convert_generated(
        to: &str,
        options: &Options,
        video: &str,
        audio: &str,
    ) -> Option<(tempfile::TempDir, PathBuf)> {
        let engine = FfmpegEngine::new();
        if let Some(reason) = engine.unavailable_reason() {
            eprintln!("skipping: {reason}");
            return None;
        }
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("clip.mp4");
        let made = Command::new(engine.ffmpeg.as_ref().unwrap())
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                &format!("testsrc=size=128x96:rate=10:{video}"),
            ])
            .args(["-f", "lavfi", "-i", audio])
            .args([
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
            ])
            .arg(&input)
            .status()
            .unwrap();
        assert!(made.success(), "could not generate a test clip");
        let step = Step {
            from: scyconvert_core::format_by_id("mp4").unwrap(),
            to: scyconvert_core::format_by_id(to).unwrap(),
        };
        let cancel = scyconvert_core::Cancel::new();
        let ctx = Ctx::new(step, options, &|_| {}, &cancel);
        let out = dir.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let outputs = match engine.convert(&ctx, &input, &out) {
            Ok(outputs) => outputs,
            Err(e)
                if [
                    "Unknown encoder",
                    "Error while opening encoder",
                    "No capable devices",
                ]
                .iter()
                .any(|m| e.to_string().contains(m)) =>
            {
                eprintln!("skipping: this FFmpeg lacks an encoder: {e}");
                return None;
            }
            Err(e) => panic!("{e}"),
        };
        Some((dir, outputs[0].clone()))
    }

    #[test]
    fn video_height_is_a_downscale_cap_with_even_dimensions() {
        for to in ["mov", "gif"] {
            for (height, expected) in [(480, (128, 96)), (48, (64, 48))] {
                let Some((_dir, output)) = convert_clip(
                    to,
                    &Options {
                        video_height: Some(height),
                        ..Options::default()
                    },
                ) else {
                    return;
                };
                let Some(probe) = FfmpegEngine::new().ffprobe else {
                    eprintln!("SKIP resolution check: ffprobe missing");
                    return;
                };
                let result = Command::new(probe)
                    .args([
                        "-v",
                        "error",
                        "-select_streams",
                        "v:0",
                        "-show_entries",
                        "stream=width,height",
                        "-of",
                        "csv=p=0",
                    ])
                    .arg(output)
                    .output()
                    .unwrap();
                assert!(result.status.success());
                assert_eq!(
                    String::from_utf8(result.stdout).unwrap().trim(),
                    format!("{},{}", expected.0, expected.1),
                    "{to}, cap {height}"
                );
            }
        }
    }

    #[test]
    fn video_height_handles_narrow_odd_sources_without_zero_sentinels() {
        let engine = FfmpegEngine::new();
        let Some(ffmpeg) = engine.ffmpeg else {
            eprintln!("SKIP narrow video: FFmpeg missing");
            return;
        };
        for (width, succeeds) in [(3, true), (1, false)] {
            let dir = tempfile::tempdir().unwrap();
            let output = dir.path().join("narrow.mov");
            let result = Command::new(&ffmpeg)
                .args(["-v", "error", "-f", "lavfi", "-i"])
                .arg(format!("testsrc=size={width}x101:rate=1:duration=1"))
                .args([
                    "-vf",
                    &video_scale(48),
                    "-c:v",
                    "libx264",
                    "-pix_fmt",
                    "yuv420p",
                ])
                .arg(&output)
                .output()
                .unwrap();
            if String::from_utf8_lossy(&result.stderr).contains("Unknown encoder") {
                eprintln!("SKIP narrow video: libx264 missing");
                return;
            }
            assert_eq!(result.status.success(), succeeds, "{result:?}");
            if succeeds {
                let Some(probe) = &engine.ffprobe else {
                    eprintln!("SKIP narrow resolution check: ffprobe missing");
                    return;
                };
                let dimensions = Command::new(probe)
                    .args([
                        "-v",
                        "error",
                        "-select_streams",
                        "v:0",
                        "-show_entries",
                        "stream=width,height",
                        "-of",
                        "csv=p=0",
                    ])
                    .arg(output)
                    .output()
                    .unwrap();
                assert!(dimensions.status.success());
                assert_eq!(String::from_utf8(dimensions.stdout).unwrap().trim(), "2,48");
            }
        }
    }

    /// The codec names of each stream, via ffprobe; `None` without it.
    fn streams(path: &Path) -> Option<Vec<String>> {
        let ffprobe = FfmpegEngine::new().ffprobe?;
        let out = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(path)
            .output()
            .unwrap();
        assert!(out.status.success());
        Some(
            String::from_utf8(out.stdout)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect(),
        )
    }

    #[test]
    fn real_ffmpeg_honors_codec_and_strip_audio() {
        let cases = [
            (Options::default(), "h264", true),
            (
                Options {
                    video_codec: Some(VideoCodec::Hevc),
                    ..Options::default()
                },
                "hevc",
                true,
            ),
            (
                Options {
                    strip_audio: true,
                    ..Options::default()
                },
                "h264",
                false,
            ),
        ];
        for (options, codec, audio) in cases {
            let Some((_dir, output)) = convert_clip("mov", &options) else {
                return;
            };
            let Some(streams) = streams(&output) else {
                eprintln!("skipping the stream check: ffprobe not found");
                return;
            };
            assert!(
                streams.iter().any(|s| s == &format!("{codec},video")),
                "{streams:?}"
            );
            assert_eq!(
                streams.iter().any(|s| s.ends_with(",audio")),
                audio,
                "{streams:?}"
            );
        }
    }

    /// `key=value` lines for the output's length and its streams' shapes.
    fn probe(path: &Path) -> Option<Vec<String>> {
        let ffprobe = FfmpegEngine::new().ffprobe?;
        let out = Command::new(ffprobe)
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration:stream=codec_name,width,height,r_frame_rate,channels,sample_rate,pix_fmt",
                "-of",
                "default=nw=1",
            ])
            .arg(path)
            .output()
            .unwrap();
        assert!(out.status.success());
        Some(
            String::from_utf8(out.stdout)
                .unwrap()
                .lines()
                .map(str::to_string)
                .collect(),
        )
    }

    fn value<'a>(probe: &'a [String], key: &str) -> &'a str {
        probe
            .iter()
            .find_map(|line| line.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("no {key} in {probe:?}"))
    }

    fn duration(probe: &[String]) -> f64 {
        value(probe, "duration").parse().unwrap()
    }

    #[test]
    fn real_ffmpeg_trims_crops_rotates_and_reshapes() {
        let options = Options {
            start: Some("1".parse().unwrap()),
            end: Some("3".parse().unwrap()),
            speed: Some(200),
            crop: Some(scyconvert_core::Aspect::Square),
            rotate: Some(scyconvert_core::Rotation::Right),
            fps: Some("25".parse().unwrap()),
            fade_in: Some("0.2".parse().unwrap()),
            fade_out: Some("0.2".parse().unwrap()),
            channels: Some(scyconvert_core::Channels::Mono),
            sample_rate: Some(22050),
            grayscale: true,
            denoise: true,
            deinterlace: true,
            strip_metadata: true,
            ..Options::default()
        };
        let Some((_dir, output)) = convert_generated(
            "mp4",
            &options,
            "duration=4",
            "sine=frequency=440:duration=4",
        ) else {
            return;
        };
        let Some(p) = probe(&output) else { return };
        // Seconds 1 to 3 at double speed.
        assert!((duration(&p) - 1.0).abs() < 0.15, "{p:?}");
        // 128x96 cropped to 96x96, then turned.
        assert_eq!(
            (value(&p, "width"), value(&p, "height")),
            ("96", "96"),
            "{p:?}"
        );
        assert_eq!(value(&p, "r_frame_rate"), "25/1");
        assert_eq!(value(&p, "channels"), "1");
        assert_eq!(value(&p, "sample_rate"), "22050");
    }

    #[test]
    fn real_ffmpeg_encodes_every_codec() {
        let cases = [
            ("mp4", VideoCodec::Av1, "av1"),
            ("webm", VideoCodec::Vp9, "vp9"),
            ("mov", VideoCodec::ProRes, "prores"),
            ("mkv", VideoCodec::Copy, "h264"),
        ];
        for (to, codec, name) in cases {
            let options = Options {
                video_codec: Some(codec),
                encoder_speed: Some(scyconvert_core::EncoderSpeed::Fastest),
                ten_bit: matches!(codec, VideoCodec::Av1),
                ..Options::default()
            };
            let Some((_dir, output)) = convert_clip(to, &options) else {
                continue;
            };
            let Some(p) = probe(&output) else { return };
            assert_eq!(value(&p, "codec_name"), name, "{to}: {p:?}");
            if codec == VideoCodec::Av1 {
                assert_eq!(value(&p, "pix_fmt"), "yuv420p10le", "{p:?}");
            }
        }
    }

    #[test]
    fn real_ffmpeg_hardware_encoder_when_present() {
        let options = Options {
            hardware: Some(scyconvert_core::Hardware::Nvidia),
            video_codec: Some(VideoCodec::Hevc),
            ..Options::default()
        };
        // Skips on machines without an NVIDIA GPU. NVENC needs frames
        // larger than the usual test clip.
        let Some((_dir, output)) = convert_generated(
            "mp4",
            &options,
            "duration=1:size=320x240",
            "sine=frequency=440:duration=1",
        ) else {
            return;
        };
        let Some(p) = probe(&output) else { return };
        assert_eq!(value(&p, "codec_name"), "hevc", "{p:?}");
    }

    #[test]
    fn real_ffmpeg_audio_options() {
        // One second of silence on each side of two seconds of tone.
        let quiet_edges = "sine=frequency=440:duration=2,adelay=1000|1000,apad=pad_dur=1";
        let options = Options {
            trim_silence: true,
            normalize: true,
            volume_db: Some(-3),
            ..Options::default()
        };
        let Some((_dir, output)) = convert_generated("mp3", &options, "duration=4", quiet_edges)
        else {
            return;
        };
        let Some(p) = probe(&output) else { return };
        assert!((duration(&p) - 2.0).abs() < 0.2, "{p:?}");
        // loudnorm would resample to 192 kHz without an explicit rate.
        assert_eq!(value(&p, "sample_rate"), "48000");

        let wav = Options {
            bit_depth: Some(24),
            fade_out: Some(Timestamp::from_secs(1)),
            ..Options::default()
        };
        let Some((_dir, output)) = convert_clip("wav", &wav) else {
            return;
        };
        let Some(p) = probe(&output) else { return };
        assert_eq!(value(&p, "codec_name"), "pcm_s24le");
    }
}

#[cfg(all(test, target_os = "linux"))]
mod security_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    #[test]
    fn local_media_never_fetches_dash_references() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let (end, count) = (stop.clone(), requests.clone());
        let thread = std::thread::spawn(move || {
            while !end.load(Ordering::SeqCst) {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
                        .unwrap();
                    let _ = stream.read(&mut [0; 2048]);
                    count.fetch_add(1, Ordering::SeqCst);
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("disguised.mp4");
        std::fs::write(&input, format!(r#"<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011" profiles="urn:mpeg:dash:profile:isoff-on-demand:2011" type="static" mediaPresentationDuration="PT1S" minBufferTime="PT1S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="1" bandwidth="1000"><BaseURL>http://{address}/probe.mp4</BaseURL><SegmentBase indexRange="0-100"><Initialization range="0-100"/></SegmentBase></Representation></AdaptationSet></Period></MPD>"#)).unwrap();
        // The review reproduction used a descriptor without an extension.
        // Keep it open in this process while children read through procfs.
        use std::os::fd::{AsRawFd, FromRawFd};
        let fd = unsafe { libc::memfd_create(c"scyconvert-security-dash".as_ptr(), 0) };
        assert!(fd >= 0);
        let mut memfd = unsafe { std::fs::File::from_raw_fd(fd) };
        memfd.write_all(&std::fs::read(&input).unwrap()).unwrap();
        let memfd_path = PathBuf::from(format!(
            "/proc/{}/fd/{}",
            std::process::id(),
            memfd.as_raw_fd()
        ));
        let options = Options::default();
        let cancel = scyconvert_core::Cancel::new();
        let ctx = Ctx::new(
            Step {
                from: scyconvert_core::format_by_id("mp4").unwrap(),
                to: scyconvert_core::format_by_id("png").unwrap(),
            },
            &options,
            &|_| {},
            &cancel,
        );
        for base in [
            PathBuf::from("/usr/bin"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/out/scyconvert"),
        ] {
            let engine = FfmpegEngine {
                ffmpeg: Some(base.join("ffmpeg")),
                ffprobe: Some(base.join("ffprobe")),
            };
            if !engine.ffmpeg.as_ref().unwrap().exists()
                || !engine.ffprobe.as_ref().unwrap().exists()
            {
                assert_ne!(
                    std::env::var("SCYCONVERT_REQUIRE_MEDIA_TOOLS").as_deref(),
                    Ok("1"),
                    "required toolset missing: {base:?}"
                );
                eprintln!("SKIP missing security toolset: {base:?}");
                continue;
            }
            let before = requests.load(Ordering::SeqCst);
            for path in [&input, &memfd_path] {
                let _ = engine.duration_us(&ctx, path);
                assert!(engine.convert(&ctx, path, dir.path()).is_err());
                let out = thumbnail_command(engine.ffmpeg.as_ref().unwrap(), path, "0", 64)
                    .output()
                    .unwrap();
                assert!(!out.status.success());
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains("whitelist"),
                    "{:?}",
                    out.stderr
                );
            }
            eprintln!(
                "{base:?}: {} HTTP requests for probe, conversion and thumbnail, path and memfd",
                requests.load(Ordering::SeqCst) - before
            );
        }
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap();
        assert_eq!(
            requests.load(Ordering::SeqCst),
            0,
            "local media issued HTTP requests"
        );
    }
}

#[cfg(test)]
mod local_playlist_tests {
    use super::*;

    #[test]
    fn refuses_manifests_that_reference_other_local_files() {
        let dir = tempfile::tempdir().unwrap();
        let segment = dir.path().join("private.mp4");
        std::fs::write(&segment, b"private local file").unwrap();
        let manifests = [
            format!(
                "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1,\n{}\n#EXT-X-ENDLIST\n",
                segment.display()
            ),
            format!("ffconcat version 1.0\nfile '{}'\n", segment.display()),
            format!(
                r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" profiles="urn:mpeg:dash:profile:isoff-on-demand:2011" type="static" mediaPresentationDuration="PT1S" minBufferTime="PT1S"><Period><AdaptationSet mimeType="video/mp4"><Representation id="1" bandwidth="1000"><BaseURL>{}</BaseURL><SegmentBase indexRange="0-100"><Initialization range="0-100"/></SegmentBase></Representation></AdaptationSet></Period></MPD>"#,
                segment.display()
            ),
        ];
        let mut tools = Vec::new();
        if let Some(probe) = crate::find_tool(&["ffprobe"], "SCYCONVERT_FFPROBE") {
            tools.push(probe);
        }
        let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/out/scyconvert/ffprobe");
        if bundle.exists() {
            tools.push(bundle);
        }
        for tool in tools {
            let demuxers = Command::new(&tool)
                .args(["-hide_banner", "-demuxers"])
                .output()
                .unwrap();
            assert!(demuxers.status.success(), "{tool:?}: cannot list demuxers");
            let demuxers = String::from_utf8_lossy(&demuxers.stdout);
            for (n, manifest) in manifests.iter().enumerate() {
                let demuxer = ["hls", "concat", "dash"][n];
                if !demuxers
                    .lines()
                    .any(|line| line.split_whitespace().nth(1) == Some(demuxer))
                {
                    eprintln!("SKIP {tool:?}: {demuxer} demuxer unavailable");
                    continue;
                }
                let input = dir
                    .path()
                    .join(["playlist.m3u8", "playlist.ffconcat", "playlist.mpd"][n]);
                std::fs::write(&input, manifest).unwrap();
                let out = Command::new(&tool)
                    .args(LOCAL_INPUT_ARGS)
                    .args(["-v", "error"])
                    .arg(local_path(&input))
                    .output()
                    .unwrap();
                assert!(!out.status.success());
                assert!(
                    String::from_utf8_lossy(&out.stderr).contains("whitelist"),
                    "{tool:?}: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }
}
