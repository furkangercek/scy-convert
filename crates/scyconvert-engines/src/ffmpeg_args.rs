//! The FFmpeg arguments for one conversion: input options (trimming), the
//! encoders a container takes, and the video and audio filter chains the
//! options build.

use std::path::Path;

use scyconvert_core::{
    AudioCodec, Caption, CaptionPlace, Channels, EncoderSpeed, Error, Flip, Hardware, Options,
    Playback, Result, Rotation, VideoCodec,
};

/// Video containers FFmpeg writes. GIF is separate: no audio, its own palette.
const CONTAINERS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpeg", "m2ts", "3gp", "ogv",
];

/// What the arguments need to know about the input.
#[derive(Debug, Clone, Copy)]
pub struct Source<'a> {
    /// The input's format id.
    pub format: &'a str,
    /// The input's length in seconds, which fading out needs.
    pub secs: Option<f64>,
    /// A bold font file for captions.
    pub font: Option<&'a Path>,
}

#[cfg(test)]
impl Source<'_> {
    pub fn video(secs: Option<f64>) -> Self {
        Source {
            format: "mp4",
            secs,
            font: None,
        }
    }
}

/// The command line around `-i <input>` and before the output path.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub input: Vec<String>,
    pub output: Vec<String>,
}

fn invalid(message: String) -> Error {
    Error::InvalidOption(message)
}

/// Maps 1-100 quality onto a CRF scale where `best` is quality 100.
fn crf(quality: u8, worst: f32) -> String {
    format!("{:.0}", worst - f32::from(quality) * 0.25)
}

pub fn video_scale(height: u32) -> String {
    // Zero is a special FFmpeg size sentinel, so clamp narrow frames to two.
    // A one-pixel source or cap cannot satisfy even sizing without enlargement.
    format!(
        "scale='if(lt(iw,2),nan,max(2,trunc(iw*min(1,{height}/ih)/2)*2))':'if(lt(min(ih,{height}),2),nan,max(2,trunc(min(ih,{height})/2)*2))'"
    )
}

/// How long the output runs, in seconds: the trimmed input at the new speed.
/// `None` when neither the input's length nor an end is known.
pub fn output_secs(o: &Options, input_secs: Option<f64>) -> Option<f64> {
    let start = o.start.map_or(0., |s| s.secs());
    let end = match (o.end.map(|e| e.secs()), input_secs) {
        (Some(end), Some(length)) => end.min(length),
        (end, length) => end.or(length)?,
    };
    Some((end - start).max(0.) * 100. / f64::from(o.speed.unwrap_or(100)))
}

/// Seconds as FFmpeg reads them, to the millisecond.
fn secs(value: f64) -> String {
    let s = format!("{value:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Changes to each frame's content: they work on stills too.
fn picture_filters(o: &Options) -> Vec<String> {
    let mut f = Vec::new();
    if o.deinterlace {
        f.push("bwdif".into());
    }
    if let Some(aspect) = o.crop {
        let (w, h) = aspect.ratio();
        f.push(format!(
            "crop='trunc(min(iw,ih*{w}/{h})/2)*2':'trunc(min(ih,iw*{h}/{w})/2)*2'"
        ));
    }
    match o.rotate {
        Some(Rotation::Right) => f.push("transpose=1".into()),
        Some(Rotation::Half) => f.push("hflip,vflip".into()),
        Some(Rotation::Left) => f.push("transpose=2".into()),
        None => {}
    }
    match o.flip {
        Some(Flip::Horizontal) => f.push("hflip".into()),
        Some(Flip::Vertical) => f.push("vflip".into()),
        Some(Flip::Both) => f.push("hflip,vflip".into()),
        None => {}
    }
    if o.denoise {
        f.push("hqdn3d".into());
    }
    if o.grayscale {
        f.push("hue=s=0".into());
    }
    f
}

fn speed_filter(o: &Options) -> Option<String> {
    o.speed
        .filter(|s| *s != 100)
        .map(|s| format!("setpts=PTS*100/{s}"))
}

/// Fades for video (`fade`) or audio (`afade`), on the output's timeline.
fn fades(filter: &str, o: &Options, out_secs: Option<f64>) -> Result<Vec<String>> {
    let mut f = Vec::new();
    if let Some(d) = o.fade_in {
        f.push(format!("{filter}=t=in:st=0:d={}", d.ffmpeg()));
    }
    if let Some(d) = o.fade_out {
        let length = out_secs.ok_or_else(|| {
            invalid("fading out needs the length of the input, and ffprobe can't read it".into())
        })?;
        let start = (length - d.secs()).max(0.);
        f.push(format!(
            "{filter}=t=out:st={}:d={}",
            secs(start),
            d.ffmpeg()
        ));
    }
    Ok(f)
}

/// `atempo` takes 0.5x to 100x per instance; slower speeds chain it.
fn tempo(speed: u16) -> Vec<String> {
    let mut factor = f64::from(speed) / 100.;
    let mut f = Vec::new();
    while factor < 0.5 {
        f.push("atempo=0.5".to_string());
        factor /= 0.5;
    }
    f.push(format!("atempo={}", secs(factor)));
    f
}

fn audio_filters(o: &Options, out_secs: Option<f64>) -> Result<Vec<String>> {
    if o.trim_silence && o.fade_out.is_some() {
        return Err(invalid(
            "trimming silence changes the length, so it can't be combined with a fade out".into(),
        ));
    }
    let mut f = Vec::new();
    if let Some(speed) = o.speed.filter(|s| *s != 100) {
        f.extend(tempo(speed));
    }
    if o.normalize {
        f.push("loudnorm=I=-16:TP=-1.5:LRA=11".into());
    }
    if let Some(db) = o.volume_db.filter(|v| *v != 0) {
        f.push(format!("volume={db}dB"));
    }
    if o.trim_silence {
        // Silence at the start, then (reversed) at the end, leaving pauses
        // in the middle alone.
        let lead = "silenceremove=start_periods=1:start_threshold=-50dB:start_silence=0.05";
        f.extend([lead, "areverse", lead, "areverse"].map(String::from));
    }
    f.extend(fades("afade", o, out_secs)?);
    Ok(f)
}

/// Sample rate, channels and bit depth for any audio output.
fn audio_shape(to: &str, o: &Options) -> Vec<String> {
    let mut a = Vec::new();
    // loudnorm resamples to 192 kHz unless told otherwise.
    if let Some(rate) = o.sample_rate.or(o.normalize.then_some(48_000)) {
        a.extend(["-ar".into(), rate.to_string()]);
    }
    if let Some(channels) = o.channels {
        let n = match channels {
            Channels::Mono => "1",
            Channels::Stereo => "2",
        };
        a.extend(["-ac".into(), n.into()]);
    }
    if to == "flac" {
        match o.bit_depth {
            Some(24) => {
                a.extend(["-sample_fmt", "s32", "-bits_per_raw_sample", "24"].map(String::from))
            }
            Some(16) => a.extend(["-sample_fmt", "s16"].map(String::from)),
            _ => {}
        }
    }
    a
}

fn audio_changed(o: &Options) -> bool {
    o.speed.is_some_and(|s| s != 100)
        || o.normalize
        || o.volume_db.is_some_and(|v| v != 0)
        || o.trim_silence
        || o.fade_in.is_some()
        || o.fade_out.is_some()
        || o.sample_rate.is_some()
        || o.channels.is_some()
}

fn picture_changed(o: &Options) -> bool {
    !picture_filters(o).is_empty()
        || o.video_height.is_some()
        || o.fps.is_some()
        || o.speed.is_some_and(|s| s != 100)
        || o.fade_in.is_some()
        || o.fade_out.is_some()
        || o.ten_bit
}

/// The codec a container gets when the options name none. `None` is the
/// container's own older codec, which no option names (`native_video`).
fn default_video_codec(to: &str) -> Option<VideoCodec> {
    match to {
        "webm" => Some(VideoCodec::Vp9),
        "avi" | "wmv" | "mpeg" | "ogv" => None,
        _ => Some(VideoCodec::H264),
    }
}

/// The encoder and quality scale of a container's own codec: MPEG-4 Part 2
/// for AVI, WMV 8 for WMV, MPEG-2 for MPEG and Theora for OGV, which their
/// players expect.
fn native_video(to: &str, quality: Option<u8>) -> Vec<String> {
    // qscale runs from 2 (best) to 31.
    let qscale = quality.map_or(3, |q| 2 + (100 - u32::from(q)) * 29 / 99);
    let (name, q) = match to {
        "wmv" => ("wmv2", qscale.to_string()),
        "mpeg" => ("mpeg2video", qscale.to_string()),
        // Theora's scale runs from 0 to 10 (best).
        "ogv" => (
            "libtheora",
            quality.map_or(7, |q| u32::from(q) / 10).to_string(),
        ),
        _ => ("mpeg4", qscale.to_string()),
    };
    ["-c:v", name, "-q:v", &q].map(String::from).to_vec()
}

/// What the Codec menu calls the codec `to` gets when none is picked.
pub fn usual_video_codec(to: &str) -> &'static str {
    match (default_video_codec(to), to) {
        (Some(codec), _) => codec.name(),
        (None, "wmv") => "WMV",
        (None, "mpeg") => "MPEG-2",
        (None, "ogv") => "Theora",
        (None, _) => "MPEG-4",
    }
}

/// The audio codec a container gets when the options name none; `None` is
/// the container's own (`native_audio`).
fn default_audio_codec(to: &str) -> Option<AudioCodec> {
    match to {
        "webm" => Some(AudioCodec::Opus),
        "avi" => Some(AudioCodec::Mp3),
        "wmv" | "mpeg" | "ogv" => None,
        _ => Some(AudioCodec::Aac),
    }
}

fn native_audio(to: &str, bitrate: Option<u32>) -> Vec<String> {
    let rate = |default: u32| format!("{}k", bitrate.unwrap_or(default));
    let mut a = match to {
        "wmv" => vec!["-c:a".into(), "wmav2".into(), "-b:a".into(), rate(192)],
        "mpeg" => vec!["-c:a".into(), "mp2".into(), "-b:a".into(), rate(192)],
        _ => vec!["-c:a".into(), "libvorbis".into()],
    };
    if to == "ogv" {
        match bitrate {
            Some(b) => a.extend(["-b:a".into(), format!("{b}k")]),
            None => a.extend(["-q:a".into(), "5".into()]),
        }
    }
    a
}

/// What the audio Codec menu calls the codec `to` gets when none is picked.
pub fn usual_audio_codec(to: &str) -> &'static str {
    match (default_audio_codec(to), to) {
        (Some(codec), _) => codec.name(),
        (None, "wmv") => "WMA",
        (None, "mpeg") => "MP2",
        (None, _) => "Vorbis",
    }
}

fn video_codecs(to: &str) -> &'static [VideoCodec] {
    use VideoCodec::*;
    match to {
        "mp4" => &[H264, Hevc, Av1, Copy],
        "mov" => &[H264, Hevc, ProRes, Copy],
        "mkv" => &[H264, Hevc, Av1, Vp9, ProRes, Copy],
        "webm" => &[Vp9, Av1, Copy],
        "flv" | "3gp" => &[H264, Copy],
        "m2ts" => &[H264, Hevc, Copy],
        "avi" | "wmv" | "mpeg" | "ogv" => &[Copy],
        _ => &[],
    }
}

fn audio_codecs(to: &str) -> &'static [AudioCodec] {
    use AudioCodec::*;
    match to {
        "mp4" => &[Aac, Mp3, Ac3, Opus, Flac, Copy],
        "mov" => &[Aac, Mp3, Ac3, Alac, Copy],
        "mkv" => &[Aac, Opus, Mp3, Ac3, Flac, Alac, Copy],
        "webm" => &[Opus, Copy],
        "avi" => &[Mp3, Ac3, Copy],
        "flv" => &[Aac, Mp3, Copy],
        "m2ts" => &[Aac, Ac3, Mp3, Copy],
        "mpeg" => &[Ac3, Mp3, Copy],
        "3gp" => &[Aac, Copy],
        "ogv" => &[Opus, Copy],
        "wmv" => &[Copy],
        "m4a" => &[Aac, Alac, Copy],
        _ => &[],
    }
}

/// The codecs `to` takes, for the app's menus. Empty where the format
/// decides alone.
pub fn video_codec_choices(to: &str) -> &'static [VideoCodec] {
    video_codecs(to)
}

pub fn audio_codec_choices(to: &str) -> &'static [AudioCodec] {
    audio_codecs(to)
}

fn not_in(what: &str, to: &str, names: impl Iterator<Item = &'static str>) -> Error {
    let names: Vec<_> = names.collect();
    invalid(format!(
        "{what} can't go in {}; it takes {}",
        to.to_uppercase(),
        names.join(", ")
    ))
}

fn speed_index(o: &Options) -> usize {
    match o.encoder_speed.unwrap_or(EncoderSpeed::Medium) {
        EncoderSpeed::Fastest => 0,
        EncoderSpeed::Fast => 1,
        EncoderSpeed::Medium => 2,
        EncoderSpeed::Slow => 3,
        EncoderSpeed::Slowest => 4,
    }
}

/// `-c:v` and its settings, and the pixel format.
fn video_encoder(to: &str, o: &Options) -> Result<Vec<String>> {
    let codec = match o.video_codec.or_else(|| default_video_codec(to)) {
        None => {
            if o.ten_bit {
                return Err(invalid(format!(
                    "{} video can't be 10-bit",
                    usual_video_codec(to)
                )));
            }
            if o.hardware.is_some() {
                return Err(invalid(format!(
                    "{} video is encoded on the CPU; pick a codec for the GPU",
                    usual_video_codec(to)
                )));
            }
            return Ok(native_video(to, o.quality));
        }
        Some(codec) if !video_codecs(to).contains(&codec) => {
            return Err(not_in(
                &format!("{} video", codec.name()),
                to,
                video_codecs(to).iter().map(|c| c.name()),
            ));
        }
        Some(codec) => codec,
    };
    if codec == VideoCodec::Copy {
        if picture_changed(o) {
            return Err(invalid(
                "copying the video keeps every frame as it is, so it can't also crop, resize, \
                 change speed or frame rate, fade or switch to 10-bit; pick a codec to re-encode"
                    .into(),
            ));
        }
        return Ok(vec!["-c:v".into(), "copy".into()]);
    }
    if o.ten_bit && codec == VideoCodec::H264 {
        return Err(invalid(
            "10-bit needs HEVC, AV1, VP9 or ProRes; few players handle 10-bit H.264".into(),
        ));
    }
    let speed = speed_index(o);
    let bitrate = o.video_bitrate.map(|b| format!("{b}k"));
    let mut a: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));
    let hw_quality = o.quality.map_or("23".into(), |q| crf(q, 40.0));
    let encoder = match (o.hardware, codec) {
        (None, VideoCodec::ProRes) | (Some(Hardware::Apple), VideoCodec::ProRes) => {
            let name = if o.hardware.is_some() {
                "prores_videotoolbox"
            } else {
                "prores_ks"
            };
            // Profile 3 is 422 HQ; ProRes is always 10-bit 4:2:2.
            push(&["-c:v", name, "-profile:v", "3", "-pix_fmt", "yuv422p10le"]);
            return Ok(a);
        }
        (None, VideoCodec::H264 | VideoCodec::Hevc) => {
            let (name, worst, default) = if codec == VideoCodec::H264 {
                ("libx264", 40.0, "20")
            } else {
                ("libx265", 44.0, "24")
            };
            let preset = ["veryfast", "fast", "medium", "slow", "veryslow"][speed];
            push(&["-c:v", name, "-preset", preset]);
            match o.video_bitrate {
                // A cap on peaks keeps hard (grainy) footage near the target;
                // an average alone overshoots it.
                Some(b) => push(&[
                    "-b:v",
                    &format!("{b}k"),
                    "-maxrate",
                    &format!("{b}k"),
                    "-bufsize",
                    &format!("{}k", b * 2),
                ]),
                None => push(&["-crf", &o.quality.map_or(default.into(), |q| crf(q, worst))]),
            }
            name
        }
        (None, VideoCodec::Av1) => {
            let preset = ["12", "10", "8", "5", "3"][speed];
            push(&["-c:v", "libsvtav1", "-preset", preset]);
            match &bitrate {
                Some(b) => push(&["-b:v", b]),
                None => {
                    let q = o
                        .quality
                        .map_or("30".into(), |q| format!("{:.0}", 63. - f32::from(q) * 0.45));
                    push(&["-crf", &q]);
                }
            }
            "libsvtav1"
        }
        (None, VideoCodec::Vp9) => {
            push(&["-c:v", "libvpx-vp9"]);
            match &bitrate {
                Some(b) => push(&["-b:v", b]),
                None => push(&[
                    "-crf",
                    &o.quality.map_or("32".into(), |q| crf(q, 52.0)),
                    "-b:v",
                    "0",
                ]),
            }
            let cpu = ["8", "6", "4", "2", "1"][speed];
            push(&["-row-mt", "1", "-deadline", "good", "-cpu-used", cpu]);
            "libvpx-vp9"
        }
        (Some(hw), codec) => {
            let suffix = match hw {
                Hardware::Nvidia => "nvenc",
                Hardware::Intel => "qsv",
                Hardware::Amd => "amf",
                Hardware::Apple => "videotoolbox",
            };
            let supported = match hw {
                Hardware::Nvidia | Hardware::Amd => {
                    matches!(codec, VideoCodec::H264 | VideoCodec::Hevc | VideoCodec::Av1)
                }
                Hardware::Intel => matches!(
                    codec,
                    VideoCodec::H264 | VideoCodec::Hevc | VideoCodec::Av1 | VideoCodec::Vp9
                ),
                Hardware::Apple => matches!(codec, VideoCodec::H264 | VideoCodec::Hevc),
            };
            if !supported {
                return Err(invalid(format!(
                    "{} can't encode {} video",
                    hw.name(),
                    codec.name()
                )));
            }
            let name = format!("{}_{suffix}", codec.id());
            push(&["-c:v", &name]);
            match hw {
                Hardware::Nvidia => {
                    push(&[
                        "-preset",
                        ["p1", "p2", "p4", "p6", "p7"][speed],
                        "-rc",
                        "vbr",
                    ]);
                    match &bitrate {
                        Some(b) => push(&["-b:v", b]),
                        None => push(&["-cq", &hw_quality, "-b:v", "0"]),
                    }
                }
                Hardware::Intel => {
                    push(&[
                        "-preset",
                        ["veryfast", "faster", "medium", "slower", "veryslow"][speed],
                    ]);
                    match &bitrate {
                        Some(b) => push(&["-b:v", b]),
                        None => push(&["-global_quality", &hw_quality]),
                    }
                }
                Hardware::Amd => {
                    push(&[
                        "-quality",
                        ["speed", "speed", "balanced", "quality", "quality"][speed],
                    ]);
                    match &bitrate {
                        Some(b) => push(&["-rc", "vbr_peak", "-b:v", b]),
                        None => push(&["-rc", "cqp", "-qp_i", &hw_quality, "-qp_p", &hw_quality]),
                    }
                }
                Hardware::Apple => match &bitrate {
                    Some(b) => push(&["-b:v", b]),
                    None => push(&["-q:v", &o.quality.unwrap_or(65).to_string()]),
                },
            }
            "hardware"
        }
        (None, VideoCodec::Copy) => unreachable!("copy returned above"),
    };
    // QuickTime and Apple devices only play HEVC tagged hvc1.
    if codec == VideoCodec::Hevc && matches!(to, "mp4" | "mov") {
        push(&["-tag:v", "hvc1"]);
    }
    let pix_fmt = match (encoder, o.ten_bit) {
        ("hardware", false) if o.hardware != Some(Hardware::Nvidia) => "nv12",
        ("hardware", true) => "p010le",
        (_, true) => "yuv420p10le",
        // FFmpeg 9 refuses VP9 in RGB (gbrap, from a transparent GIF), and
        // few players handle anything but 4:2:0.
        _ => "yuv420p",
    };
    push(&["-pix_fmt", pix_fmt]);
    Ok(a)
}

/// `-c:a` and its settings for the audio inside a video.
fn video_audio_encoder(to: &str, o: &Options) -> Result<Vec<String>> {
    let codec = match o.audio_codec {
        Some(codec) if !audio_codecs(to).contains(&codec) => {
            return Err(not_in(
                &format!("{} audio", codec.name()),
                to,
                audio_codecs(to).iter().map(|c| c.name()),
            ));
        }
        Some(codec) => codec,
        None => match default_audio_codec(to) {
            Some(codec) => codec,
            None => return Ok(native_audio(to, o.audio_bitrate)),
        },
    };
    audio_encoder(codec, o)
}

fn audio_encoder(codec: AudioCodec, o: &Options) -> Result<Vec<String>> {
    let bitrate = o.audio_bitrate.map(|b| format!("{b}k"));
    let mut a: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));
    match codec {
        AudioCodec::Aac => push(&["-c:a", "aac", "-b:a", bitrate.as_deref().unwrap_or("192k")]),
        AudioCodec::Mp3 => match &bitrate {
            Some(b) => push(&["-c:a", "libmp3lame", "-b:a", b]),
            None => push(&["-c:a", "libmp3lame"]),
        },
        AudioCodec::Ac3 => push(&["-c:a", "ac3", "-b:a", bitrate.as_deref().unwrap_or("448k")]),
        AudioCodec::Opus => {
            push(&["-c:a", "libopus"]);
            if let Some(b) = &bitrate {
                push(&["-b:a", b]);
            }
        }
        AudioCodec::Flac => push(&["-c:a", "flac"]),
        AudioCodec::Alac => push(&["-c:a", "alac"]),
        AudioCodec::Copy => {
            if audio_changed(o) {
                return Err(invalid(
                    "copying the audio keeps it as it is, so it can't also change speed, \
                     volume, sample rate or channels, or fade; pick a codec to re-encode"
                        .into(),
                ));
            }
            push(&["-c:a", "copy"]);
        }
    }
    Ok(a)
}

/// Encoder settings for audio-only targets.
fn audio_target(to: &str, o: &Options) -> Result<Vec<String>> {
    let bitrate = o.audio_bitrate.map(|b| format!("{b}k"));
    let aac = format!("{}k", o.audio_bitrate.unwrap_or(192));
    let mut a: Vec<String> = vec!["-vn".into()];
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));
    match to {
        "mp3" => match &bitrate {
            Some(b) => push(&["-c:a", "libmp3lame", "-b:a", b]),
            None => push(&["-c:a", "libmp3lame", "-q:a", "2"]),
        },
        "wav" => push(&[
            "-c:a",
            if o.bit_depth == Some(24) {
                "pcm_s24le"
            } else {
                "pcm_s16le"
            },
        ]),
        "aiff" => push(&[
            "-c:a",
            if o.bit_depth == Some(24) {
                "pcm_s24be"
            } else {
                "pcm_s16be"
            },
        ]),
        "wma" => push(&[
            "-c:a",
            "wmav2",
            "-b:a",
            bitrate.as_deref().unwrap_or("192k"),
        ]),
        "ac3" => push(&["-c:a", "ac3", "-b:a", bitrate.as_deref().unwrap_or("384k")]),
        "flac" => push(&["-c:a", "flac"]),
        "aac" => push(&["-c:a", "aac", "-b:a", &aac, "-f", "adts"]),
        "m4a" => match o.audio_codec {
            Some(codec) if !audio_codecs(to).contains(&codec) => {
                return Err(not_in(
                    &format!("{} audio", codec.name()),
                    to,
                    audio_codecs(to).iter().map(|c| c.name()),
                ));
            }
            Some(codec) if codec != AudioCodec::Aac => a.extend(audio_encoder(codec, o)?),
            _ => push(&["-c:a", "aac", "-b:a", &aac]),
        },
        "ogg" => match &bitrate {
            Some(b) => push(&["-c:a", "libvorbis", "-b:a", b]),
            None => push(&["-c:a", "libvorbis", "-q:a", "5"]),
        },
        "opus" => push(&[
            "-c:a",
            "libopus",
            "-b:a",
            bitrate.as_deref().unwrap_or("128k"),
        ]),
        _ => {}
    }
    Ok(a)
}

fn metadata(o: &Options) -> Vec<String> {
    if o.strip_metadata {
        ["-map_metadata", "-1", "-map_chapters", "-1"]
            .map(String::from)
            .to_vec()
    } else {
        Vec::new()
    }
}

fn chain(filters: Vec<String>) -> Option<String> {
    (!filters.is_empty()).then(|| filters.join(","))
}

/// `o` with a size target turned into bitrates: the input's own average
/// bitrate (from its length and size on disk) scaled to `size_percent`, with
/// a fifth (32 to 128 kbit/s) kept for the audio.
pub fn with_size_target(
    to: &str,
    o: &Options,
    input_secs: Option<f64>,
    input_bytes: Option<u64>,
) -> Result<Options> {
    let Some(percent) = o.size_percent else {
        return Ok(o.clone());
    };
    let (Some(secs), Some(bytes)) = (input_secs.filter(|s| *s > 0.), input_bytes) else {
        return Err(invalid(
            "compressing to a share of the size needs the input's length, and ffprobe can't read it"
                .into(),
        ));
    };
    let total = bytes as f64 * 8. / secs / 1000. * f64::from(percent) / 100.;
    let mut sized = o.clone();
    if CONTAINERS.contains(&to) {
        let audio = if o.strip_audio {
            0.
        } else {
            o.audio_bitrate
                .map_or((total * 0.2).clamp(32., 128.), f64::from)
        };
        if !o.strip_audio {
            sized.audio_bitrate = Some(audio as u32);
        }
        sized.video_bitrate = Some(((total - audio) as u32).max(100));
        sized.quality = None;
    } else {
        sized.audio_bitrate = Some((total as u32).clamp(8, 640));
    }
    Ok(sized)
}

/// Escapes `value` for a filter option inside a `-vf` graph: once for the
/// option parser, then again for the graph parser.
fn filter_value(value: &str) -> String {
    let escape = |text: &str, special: &[char]| {
        text.chars().fold(String::new(), |mut out, c| {
            if special.contains(&c) {
                out.push('\\');
            }
            out.push(c);
            out
        })
    };
    escape(
        &escape(value, &['\\', '\'', ':']),
        &['\\', '\'', '[', ']', ',', ';'],
    )
}

/// Longest caption line, in characters.
const CAPTION_LINE: usize = 22;

/// `text` in lines of at most `width` characters, broken between words
/// where it can be.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > width {
            let rest = word.split_off(width);
            lines.push(word.into_iter().collect());
            word = rest;
        }
        let word: String = word.into_iter().collect();
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(&word);
            }
            _ => lines.push(word),
        }
    }
    lines
}

/// A white bar above or below the picture with `caption` centered on it in
/// black. Sizes are shares of the width, so the text fits any picture: the
/// longest line fills about 90% of it, up to a cap for short captions.
fn caption_filters(caption: &Caption, font: &Path) -> Vec<String> {
    let lines = wrap(caption.text.trim(), CAPTION_LINE);
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1);
    // Bold Arial averages about 0.6 em per character.
    let size = (0.9 / (longest as f64 * 0.6)).min(0.12);
    let line = size * 1.2;
    let margin = size * 0.3;
    let bar = margin * 2. + line * lines.len() as f64;
    let bar_px = format!("trunc(iw*{bar:.4}/2)*2");
    let (pad_y, top) = match caption.place {
        CaptionPlace::Top => (bar_px.as_str(), "0".to_string()),
        CaptionPlace::Bottom => ("0", format!("h-w*{bar:.4}")),
    };
    let font = filter_value(&font.to_string_lossy().replace('\\', "/"));
    let mut f = vec![format!("pad=w=iw:h=ih+{bar_px}:x=0:y={pad_y}:color=white")];
    for (i, text) in lines.iter().enumerate() {
        let y = margin + line * i as f64;
        f.push(format!(
            "drawtext=fontfile={font}:text={}:expansion=none:fontcolor=black:\
             fontsize=w*{size:.4}:x=(w-text_w)/2:y={top}+w*{y:.4}:y_align=font",
            filter_value(text)
        ));
    }
    f
}

/// Frames for GIF output: the picture changes, caption and playback, then
/// a palette made for this GIF.
fn gif_graph(o: &Options, source: &Source, out_secs: Option<f64>) -> Result<String> {
    // A GIF keeps its own frame rate and size unless asked; video becomes
    // 12 fps and at most 720 px wide, which keeps the file reasonable.
    let from_gif = source.format == "gif";
    let mut vf = picture_filters(o);
    vf.extend(speed_filter(o));
    if from_gif && o.speed.is_some_and(|s| s > 100) {
        // Browsers slow frames shorter than 2/100 s right down, so a sped-up
        // GIF keeps at most 50 frames a second.
        vf.push("select='isnan(prev_selected_t)+gte(t-prev_selected_t,0.02)'".into());
    }
    match (o.fps, from_gif) {
        (Some(fps), _) => vf.push(format!("fps={}", fps.ffmpeg())),
        (None, false) => vf.push("fps=12".into()),
        (None, true) => {}
    }
    match (o.video_height, from_gif) {
        (Some(height), _) => vf.push(format!("{}:flags=lanczos", video_scale(height))),
        (None, false) => vf.push("scale='min(720,iw)':-2:flags=lanczos".into()),
        (None, true) => {}
    }
    vf.extend(fades("fade", o, out_secs)?);
    if let Some(caption) = &o.caption {
        let font = source
            .font
            .ok_or_else(|| invalid("captions need a bold font, and none was found".into()))?;
        vf.extend(caption_filters(caption, font));
    }
    match o.playback {
        Some(Playback::Reverse) => vf.push("reverse".into()),
        // The backward half starts one frame in, so the turn doesn't stall.
        Some(Playback::Boomerang) => vf.push(
            "split[f][r];[r]reverse,trim=start_frame=1,setpts=PTS-STARTPTS[b];\
             [f][b]concat=n=2:v=1:a=0"
                .into(),
        ),
        None => {}
    }
    vf.push("split[a][b];[a]palettegen[p];[b][p]paletteuse".into());
    Ok(vf.join(","))
}

/// The arguments to convert `source` to `to`.
pub fn ffmpeg_args(to: &str, o: &Options, source: &Source) -> Result<Args> {
    let input_secs = source.secs;
    if to != "gif" && (o.playback.is_some() || o.caption.is_some()) {
        return Err(invalid(
            "reversing and captions are only for GIF output".into(),
        ));
    }
    let out_secs = output_secs(o, input_secs);
    let mut input = Vec::new();
    if let Some(start) = o.start {
        input.extend(["-ss".into(), start.ffmpeg()]);
    }
    let is_still = to == "png";
    if let Some(end) = o.end.filter(|_| !is_still) {
        input.extend(["-to".into(), end.ffmpeg()]);
    }
    let mut output: Vec<String> = Vec::new();
    match to {
        _ if CONTAINERS.contains(&to) => {
            let vcodec = video_encoder(to, o)?;
            let copying = vcodec.iter().any(|a| a == "copy");
            output.extend(vcodec);
            if !o.strip_audio {
                let acodec = video_audio_encoder(to, o)?;
                let audio_copy = acodec.iter().any(|a| a == "copy");
                output.extend(acodec);
                if !audio_copy {
                    output.extend(audio_shape(to, o));
                }
            }
            if matches!(to, "mp4" | "mov" | "3gp") {
                output.extend(["-movflags".into(), "+faststart".into()]);
            }
            if o.strip_audio {
                output.push("-an".into());
            }
            output.extend(metadata(o));
            if !copying {
                let mut vf = picture_filters(o);
                vf.extend(o.video_height.map(video_scale));
                vf.extend(speed_filter(o));
                vf.extend(o.fps.map(|f| format!("fps={}", f.ffmpeg())));
                vf.extend(fades("fade", o, out_secs)?);
                if let Some(vf) = chain(vf) {
                    output.extend(["-vf".into(), vf]);
                }
            }
            if !o.strip_audio
                && let Some(af) = chain(audio_filters(o, out_secs)?)
            {
                output.extend(["-af".into(), af]);
            }
        }
        "gif" => {
            output.extend(["-vf".into(), gif_graph(o, source, out_secs)?, "-an".into()]);
            if source.format == "gif" && o.fps.is_none() {
                // Each frame keeps its own delay. At FFmpeg's usual constant
                // rate, a 10 fps GIF sped up would lose every other frame.
                output.extend(["-fps_mode", "vfr", "-enc_time_base:v", "1/100"].map(String::from));
            }
        }
        // Still frames are always written as PNG, keeping any alpha; the
        // caller then encodes the target through the image engine.
        "png" => {
            output.extend(["-frames:v", "1", "-update", "1"].map(String::from));
            let mut vf = picture_filters(o);
            if let Some(m) = o.max_size {
                vf.push(format!(
                    "scale='min(iw,{m})':'min(ih,{m})':force_original_aspect_ratio=decrease"
                ));
            }
            if let Some(vf) = chain(vf) {
                output.extend(["-vf".into(), vf]);
            }
        }
        _ => {
            output.extend(audio_target(to, o)?);
            if !(to == "m4a" && o.audio_codec == Some(AudioCodec::Copy)) {
                output.extend(audio_shape(to, o));
                if let Some(af) = chain(audio_filters(o, out_secs)?) {
                    output.extend(["-af".into(), af]);
                }
            }
            output.extend(metadata(o));
        }
    }
    Ok(Args { input, output })
}

#[cfg(test)]
mod tests {
    use scyconvert_core::{Aspect, FrameRate, Timestamp};

    use super::*;

    fn out(to: &str, o: &Options) -> String {
        ffmpeg_args(to, o, &Source::video(Some(10.)))
            .unwrap()
            .output
            .join(" ")
    }

    fn err(to: &str, o: &Options) -> String {
        ffmpeg_args(to, o, &Source::video(Some(10.)))
            .unwrap_err()
            .to_string()
    }

    #[test]
    fn options_shape_encoder_args() {
        let o = Options {
            quality: Some(80),
            video_height: Some(480),
            audio_bitrate: Some(96),
            ..Options::default()
        };
        let mp4 = out("mp4", &o);
        assert!(mp4.contains("-crf 20") && mp4.contains("-b:a 96k"), "{mp4}");
        assert!(mp4.ends_with(&format!("-vf {}", video_scale(480))), "{mp4}");
        let gif = out("gif", &o);
        assert!(
            gif.contains(&format!("{}:flags", video_scale(480))) && !gif.contains("-vf scale"),
            "{gif}"
        );
        let mp3 = out("mp3", &o);
        assert!(mp3.contains("-b:a 96k") && !mp3.contains("-q:a"), "{mp3}");
        assert!(!out("png", &o).contains("scale"));
    }

    #[test]
    fn gif_edits_keep_the_gifs_own_frames() {
        let font = Path::new(r"C:\Windows\Fonts\arialbd.ttf");
        let gif = Source {
            format: "gif",
            secs: Some(3.),
            font: Some(font),
        };
        let args = |o: &Options| ffmpeg_args("gif", o, &gif).unwrap().output.join(" ");
        assert_eq!(
            args(&Options {
                playback: Some(Playback::Reverse),
                ..Options::default()
            }),
            "-vf reverse,split[a][b];[a]palettegen[p];[b][p]paletteuse -an \
             -fps_mode vfr -enc_time_base:v 1/100"
        );
        let faster = args(&Options {
            speed: Some(200),
            ..Options::default()
        });
        assert!(
            faster.starts_with("-vf setpts=PTS*100/200,select=") && !faster.contains("fps="),
            "{faster}"
        );
        let captioned = args(&Options {
            caption: Some(Caption {
                text: "it's: 100% [fine]".into(),
                place: CaptionPlace::Bottom,
            }),
            ..Options::default()
        });
        assert!(captioned.contains("pad=w=iw:h=ih+trunc(iw*"), "{captioned}");
        assert!(
            captioned.contains(r"fontfile=C\\:/Windows/Fonts/arialbd.ttf"),
            "{captioned}"
        );
        assert!(
            captioned.contains(r"text=it\\\'s\\: 100% \[fine\]:expansion=none"),
            "{captioned}"
        );
        assert!(captioned.contains(":y=h-w*"), "{captioned}");
        // Video to GIF still gets its usual rate and size.
        assert!(out("gif", &Options::default()).starts_with("-vf fps=12,scale="));
        assert!(
            ffmpeg_args(
                "gif",
                &Options {
                    caption: Some(Caption {
                        text: "hi".into(),
                        place: CaptionPlace::Top,
                    }),
                    ..Options::default()
                },
                &Source { font: None, ..gif }
            )
            .is_err()
        );
        assert!(
            err(
                "mp4",
                &Options {
                    playback: Some(Playback::Boomerang),
                    ..Options::default()
                }
            )
            .contains("only for GIF")
        );
    }

    #[test]
    fn captions_wrap_between_words() {
        assert_eq!(wrap("  one  two three ", 9), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij k", 4), ["abcd", "efgh", "ij k"]);
        assert!(wrap(" ", 9).is_empty());
    }

    #[test]
    fn defaults_are_unchanged() {
        let o = Options::default();
        assert_eq!(
            out("mp4", &o),
            "-c:v libx264 -preset medium -crf 20 -pix_fmt yuv420p -c:a aac -b:a 192k -movflags +faststart"
        );
        assert_eq!(
            out("mkv", &o),
            "-c:v libx264 -preset medium -crf 20 -pix_fmt yuv420p -c:a aac -b:a 192k"
        );
        assert_eq!(
            out("webm", &o),
            "-c:v libvpx-vp9 -crf 32 -b:v 0 -row-mt 1 -deadline good -cpu-used 4 -pix_fmt yuv420p -c:a libopus"
        );
        assert_eq!(out("avi", &o), "-c:v mpeg4 -q:v 3 -c:a libmp3lame");
        assert_eq!(
            out("gif", &o),
            "-vf fps=12,scale='min(720,iw)':-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse -an"
        );
        assert_eq!(out("mp3", &o), "-vn -c:a libmp3lame -q:a 2");
        assert!(
            ffmpeg_args("mp4", &o, &Source::video(None))
                .unwrap()
                .input
                .is_empty()
        );
    }

    #[test]
    fn codec_and_strip_audio_shape_encoder_args() {
        let hevc = Options {
            video_codec: Some(VideoCodec::Hevc),
            ..Options::default()
        };
        let mp4 = out("mp4", &hevc);
        assert!(
            mp4.contains("-c:v libx265") && mp4.contains("-tag:v hvc1") && !mp4.contains("libx264"),
            "{mp4}"
        );
        assert!(!out("mkv", &hevc).contains("hvc1"));
        // WebM can't hold HEVC, and says so instead of quietly using VP9.
        assert!(err("webm", &hevc).contains("VP9, AV1"));

        let silent = Options {
            strip_audio: true,
            ..Options::default()
        };
        for to in ["mp4", "mov", "mkv", "webm", "avi"] {
            let args = out(to, &silent);
            assert!(args.contains("-an"), "{to}: {args}");
            assert!(!args.contains("-c:a"), "{to}: {args}");
        }
        // Audio targets ignore it rather than producing an empty file.
        assert!(!out("mp3", &silent).contains("-an"));
    }

    #[test]
    fn codecs_follow_the_container() {
        let with = |codec| Options {
            video_codec: Some(codec),
            ..Options::default()
        };
        assert!(out("mp4", &with(VideoCodec::Av1)).contains("-c:v libsvtav1 -preset 8 -crf 30"));
        assert!(out("webm", &with(VideoCodec::Av1)).contains("libsvtav1"));
        assert!(out("mkv", &with(VideoCodec::Vp9)).contains("libvpx-vp9"));
        let prores = out("mov", &with(VideoCodec::ProRes));
        assert!(
            prores.starts_with("-c:v prores_ks -profile:v 3 -pix_fmt yuv422p10le -c:a aac"),
            "{prores}"
        );
        assert!(err("mp4", &with(VideoCodec::ProRes)).contains("ProRes video can't go in MP4"));
        assert!(err("avi", &with(VideoCodec::H264)).contains("AVI"));
        assert_eq!(
            out("mkv", &with(VideoCodec::Copy)),
            "-c:v copy -c:a aac -b:a 192k"
        );
        let copy_and_crop = Options {
            crop: Some(Aspect::Square),
            ..with(VideoCodec::Copy)
        };
        assert!(err("mp4", &copy_and_crop).contains("pick a codec"));
    }

    #[test]
    fn size_targets_come_from_the_input() {
        let o = Options {
            size_percent: Some(50),
            ..Options::default()
        };
        // 10 MB over 10 s is 8000 kbit/s; half is 4000, 128 of it audio.
        let sized = with_size_target("mp4", &o, Some(10.), Some(10_000_000)).unwrap();
        assert_eq!(
            (sized.video_bitrate, sized.audio_bitrate),
            (Some(3872), Some(128))
        );
        assert!(out("mp4", &sized).contains("-b:v 3872k"));
        let muted = Options {
            strip_audio: true,
            ..o.clone()
        };
        let sized = with_size_target("mp4", &muted, Some(10.), Some(10_000_000)).unwrap();
        assert_eq!(
            (sized.video_bitrate, sized.audio_bitrate),
            (Some(4000), None)
        );
        assert!(with_size_target("mp4", &o, None, Some(1)).is_err());
        assert_eq!(
            with_size_target("mp4", &Options::default(), None, None).unwrap(),
            Options::default()
        );
    }

    #[test]
    fn older_containers_use_their_own_codecs() {
        let o = Options::default();
        assert_eq!(out("wmv", &o), "-c:v wmv2 -q:v 3 -c:a wmav2 -b:a 192k");
        assert_eq!(out("mpeg", &o), "-c:v mpeg2video -q:v 3 -c:a mp2 -b:a 192k");
        assert_eq!(
            out("ogv", &o),
            "-c:v libtheora -q:v 7 -c:a libvorbis -q:a 5"
        );
        assert!(out("flv", &o).starts_with("-c:v libx264"));
        assert!(out("3gp", &o).ends_with("-movflags +faststart"));
        // HEVC in M2TS needs no QuickTime tag.
        let hevc = Options {
            video_codec: Some(VideoCodec::Hevc),
            ..Options::default()
        };
        assert!(!out("m2ts", &hevc).contains("hvc1"));
        assert_eq!(usual_video_codec("wmv"), "WMV");
        assert_eq!(usual_audio_codec("mpeg"), "MP2");
        let nv = Options {
            hardware: Some(Hardware::Nvidia),
            ..Options::default()
        };
        assert!(err("wmv", &nv).contains("on the CPU"));
        let mute = Options {
            video_codec: Some(VideoCodec::Copy),
            strip_audio: true,
            ..Options::default()
        };
        assert_eq!(out("wmv", &mute), "-c:v copy -an");
        let aiff = Options {
            bit_depth: Some(24),
            ..Options::default()
        };
        assert_eq!(out("aiff", &aiff), "-vn -c:a pcm_s24be");
        assert_eq!(out("wma", &Options::default()), "-vn -c:a wmav2 -b:a 192k");
    }

    #[test]
    fn hardware_encoders() {
        let nv = Options {
            hardware: Some(Hardware::Nvidia),
            video_codec: Some(VideoCodec::Hevc),
            encoder_speed: Some(EncoderSpeed::Slowest),
            ..Options::default()
        };
        assert!(
            out("mp4", &nv).starts_with(
                "-c:v hevc_nvenc -preset p7 -rc vbr -cq 23 -b:v 0 -tag:v hvc1 -pix_fmt yuv420p"
            ),
            "{}",
            out("mp4", &nv)
        );
        let ten = Options {
            ten_bit: true,
            ..nv.clone()
        };
        assert!(out("mp4", &ten).contains("-pix_fmt p010le"));
        let qsv = Options {
            hardware: Some(Hardware::Intel),
            video_bitrate: Some(8000),
            ..Options::default()
        };
        assert!(
            out("mp4", &qsv).starts_with("-c:v h264_qsv -preset medium -b:v 8000k -pix_fmt nv12")
        );
        let prores_nv = Options {
            hardware: Some(Hardware::Nvidia),
            video_codec: Some(VideoCodec::ProRes),
            ..Options::default()
        };
        assert!(err("mov", &prores_nv).contains("can't encode ProRes"));
        let h264_ten = Options {
            ten_bit: true,
            ..Options::default()
        };
        assert!(err("mp4", &h264_ten).contains("10-bit needs"));
    }

    #[test]
    fn filters_and_trimming() {
        let o = Options {
            start: Some("1".parse().unwrap()),
            end: Some("5".parse().unwrap()),
            speed: Some(200),
            crop: Some(Aspect::Square),
            rotate: Some(Rotation::Right),
            grayscale: true,
            fps: Some(FrameRate { hundredths: 3000 }),
            fade_in: Some("0.5".parse().unwrap()),
            fade_out: Some(Timestamp::from_secs(1)),
            volume_db: Some(-3),
            channels: Some(Channels::Mono),
            ..Options::default()
        };
        let args = ffmpeg_args("mp4", &o, &Source::video(Some(10.))).unwrap();
        assert_eq!(args.input, ["-ss", "1", "-to", "5"]);
        let output = args.output.join(" ");
        // 4 s of input at double speed is 2 s, so the fade out starts at 1 s.
        assert!(
            output.contains(
                "-vf crop='trunc(min(iw,ih*1/1)/2)*2':'trunc(min(ih,iw*1/1)/2)*2',transpose=1,hue=s=0,setpts=PTS*100/200,fps=30,fade=t=in:st=0:d=0.5,fade=t=out:st=1:d=1"
            ),
            "{output}"
        );
        assert!(
            output.contains("-af atempo=2,volume=-3dB,afade=t=in:st=0:d=0.5,afade=t=out:st=1:d=1"),
            "{output}"
        );
        assert!(output.contains("-ac 1"), "{output}");
        assert_eq!(output_secs(&o, None), Some(2.));
        assert!(
            ffmpeg_args(
                "mp4",
                &Options {
                    end: None,
                    ..o.clone()
                },
                &Source::video(None)
            )
            .unwrap_err()
            .to_string()
            .contains("fading out needs")
        );
        assert_eq!(tempo(25), ["atempo=0.5", "atempo=0.5"]);
        assert_eq!(tempo(150), ["atempo=1.5"]);
    }

    #[test]
    fn audio_options() {
        let o = Options {
            normalize: true,
            bit_depth: Some(24),
            ..Options::default()
        };
        assert_eq!(
            out("wav", &o),
            "-vn -c:a pcm_s24le -ar 48000 -af loudnorm=I=-16:TP=-1.5:LRA=11"
        );
        assert!(out("flac", &o).contains("-sample_fmt s32 -bits_per_raw_sample 24"));
        let alac = Options {
            audio_codec: Some(AudioCodec::Alac),
            sample_rate: Some(44100),
            strip_metadata: true,
            ..Options::default()
        };
        assert_eq!(
            out("m4a", &alac),
            "-vn -c:a alac -ar 44100 -map_metadata -1 -map_chapters -1"
        );
        assert!(
            err(
                "m4a",
                &Options {
                    audio_codec: Some(AudioCodec::Opus),
                    ..Options::default()
                }
            )
            .contains("Opus audio can't go in M4A")
        );
        let fade_and_trim = Options {
            trim_silence: true,
            fade_out: Some(Timestamp::from_secs(1)),
            ..Options::default()
        };
        assert!(err("mp3", &fade_and_trim).contains("trimming silence"));
        let copy_louder = Options {
            audio_codec: Some(AudioCodec::Copy),
            volume_db: Some(6),
            ..Options::default()
        };
        assert!(err("mkv", &copy_louder).contains("copying the audio"));
    }
}
