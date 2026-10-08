//! Quick convert's advanced options for video and audio: collapsible
//! sections of dropdowns, checkboxes and time fields. Each control reads and
//! writes one `Options` field, so adding one is an entry in `sections`.

use gpui_kit::SharedString;
use scyconvert_core::{
    Aspect, Category, Channels, EncoderSpeed, Flip, Format, Hardware, Options, Rotation,
    SAMPLE_RATES, Timestamp,
};
use scyconvert_engines::ffmpeg_args::audio_codec_choices;

/// The id of every dropdown's "leave it as it is" choice.
pub const DEFAULT: &str = "default";

pub enum Control {
    Select {
        /// `(id, label)`; the first is the default.
        choices: Vec<(SharedString, SharedString)>,
        get: fn(&Options) -> String,
        set: fn(&mut Options, &str),
    },
    Check {
        get: fn(&Options) -> bool,
        set: fn(&mut Options, bool),
    },
    /// A text field for a time, owned by the view.
    Time {
        get: fn(&Options) -> Option<Timestamp>,
        set: fn(&mut Options, Option<Timestamp>),
    },
}

pub struct Field {
    pub id: &'static str,
    pub label: &'static str,
    pub control: Control,
}

impl Field {
    /// Whether the user or a preset changed it from the default.
    pub fn changed(&self, o: &Options) -> bool {
        match &self.control {
            Control::Select { get, .. } => get(o) != DEFAULT,
            Control::Check { get, .. } => get(o),
            Control::Time { get, .. } => get(o).is_some(),
        }
    }

    /// Copies this field's value from `from` into `to`.
    pub fn copy(&self, from: &Options, to: &mut Options) {
        match &self.control {
            Control::Select { get, set, .. } => set(to, &get(from)),
            Control::Check { get, set } => set(to, get(from)),
            Control::Time { get, set } => set(to, get(from)),
        }
    }

    /// Puts this field back to its default in `o`.
    pub fn reset(&self, o: &mut Options) {
        match &self.control {
            Control::Select { set, .. } => set(o, DEFAULT),
            Control::Check { set, .. } => set(o, false),
            Control::Time { set, .. } => set(o, None),
        }
    }
}

pub struct Section {
    pub id: &'static str,
    pub title: &'static str,
    pub fields: Vec<Field>,
}

fn choices(
    default: &str,
    items: impl IntoIterator<Item = (String, String)>,
) -> Vec<(SharedString, SharedString)> {
    std::iter::once((DEFAULT.into(), default.to_string().into()))
        .chain(
            items
                .into_iter()
                .map(|(id, label)| (id.into(), label.into())),
        )
        .collect()
}

/// The id for an optional value: its own, or `DEFAULT`.
fn id_of<T: ToString>(value: Option<T>) -> String {
    value.map_or(DEFAULT.into(), |v| v.to_string())
}

fn select(
    id: &'static str,
    label: &'static str,
    choices: Vec<(SharedString, SharedString)>,
    get: fn(&Options) -> String,
    set: fn(&mut Options, &str),
) -> Field {
    Field {
        id,
        label,
        control: Control::Select { choices, get, set },
    }
}

fn check(
    id: &'static str,
    label: &'static str,
    get: fn(&Options) -> bool,
    set: fn(&mut Options, bool),
) -> Field {
    Field {
        id,
        label,
        control: Control::Check { get, set },
    }
}

/// The GPU encoders this platform can have.
fn hardware() -> Vec<Hardware> {
    if cfg!(target_os = "macos") {
        vec![Hardware::Apple]
    } else {
        vec![Hardware::Nvidia, Hardware::Intel, Hardware::Amd]
    }
}

fn fps(gif: bool) -> Field {
    let rates: &[&str] = if gif {
        &["10", "15", "20", "25", "30"]
    } else {
        &["23.98", "24", "25", "29.97", "30", "50", "59.94", "60"]
    };
    select(
        "fps",
        "Frame rate",
        choices(
            if gif { "12 fps" } else { "Original" },
            rates.iter().map(|r| (r.to_string(), format!("{r} fps"))),
        ),
        |o| id_of(o.fps),
        |o, id| o.fps = id.parse().ok(),
    )
}

fn encoding(to: &Format) -> Section {
    let mut fields = Vec::new();
    if to.id != "avi" {
        fields.push(select(
            "hardware",
            "Encoder",
            choices(
                "Software (CPU)",
                hardware()
                    .into_iter()
                    .map(|h| (h.id().into(), h.name().into())),
            ),
            |o| id_of(o.hardware),
            |o, id| o.hardware = id.parse().ok(),
        ));
        fields.push(select(
            "encoder-speed",
            "Encoder speed",
            choices(
                "Medium",
                EncoderSpeed::ALL
                    .iter()
                    .filter(|s| **s != EncoderSpeed::Medium)
                    .map(|s| (s.id().into(), s.name().into())),
            ),
            |o| id_of(o.encoder_speed),
            |o, id| o.encoder_speed = id.parse().ok(),
        ));
        fields.push(select(
            "video-bitrate",
            "Bitrate",
            choices(
                "By quality",
                [1000, 2500, 5000, 8000, 12000, 20000, 35000, 50000]
                    .map(|b| (b.to_string(), format!("{} Mbit/s", f64::from(b) / 1000.))),
            ),
            |o| id_of(o.video_bitrate),
            |o, id| o.video_bitrate = id.parse().ok(),
        ));
    }
    fields.push(fps(false));
    if to.id != "avi" {
        fields.push(check(
            "ten-bit",
            "10-bit color",
            |o| o.ten_bit,
            |o, on| o.ten_bit = on,
        ));
    }
    fields.push(strip_metadata());
    Section {
        id: "encoding",
        title: "Encoding",
        fields,
    }
}

fn picture(gif: bool) -> Section {
    let mut fields = vec![
        select(
            "crop",
            "Crop",
            choices(
                "Original",
                Aspect::ALL.iter().map(|a| (a.id().into(), a.name().into())),
            ),
            |o| id_of(o.crop.map(|a| a.id())),
            |o, id| o.crop = id.parse().ok(),
        ),
        select(
            "rotate",
            "Rotate",
            choices(
                "None",
                Rotation::ALL
                    .iter()
                    .map(|r| (r.id().into(), r.name().into())),
            ),
            |o| id_of(o.rotate.map(|r| r.id())),
            |o, id| o.rotate = id.parse().ok(),
        ),
        select(
            "flip",
            "Flip",
            choices(
                "None",
                Flip::ALL.iter().map(|f| (f.id().into(), f.name().into())),
            ),
            |o| id_of(o.flip.map(|f| f.id())),
            |o, id| o.flip = id.parse().ok(),
        ),
    ];
    if gif {
        fields.push(fps(true));
    }
    fields.extend([
        check(
            "deinterlace",
            "Deinterlace",
            |o| o.deinterlace,
            |o, on| o.deinterlace = on,
        ),
        check(
            "denoise",
            "Reduce noise",
            |o| o.denoise,
            |o, on| o.denoise = on,
        ),
        check(
            "grayscale",
            "Black and white",
            |o| o.grayscale,
            |o, on| o.grayscale = on,
        ),
    ]);
    Section {
        id: "picture",
        title: "Picture",
        fields,
    }
}

fn fade_lengths() -> Vec<(SharedString, SharedString)> {
    choices(
        "None",
        ["0.5", "1", "2", "3", "5"].map(|s| (s.to_string(), format!("{s} s"))),
    )
}

fn timing() -> Section {
    Section {
        id: "timing",
        title: "Trim and speed",
        fields: vec![
            Field {
                id: "start",
                label: "Start at",
                control: Control::Time {
                    get: |o| o.start,
                    set: |o, t| o.start = t,
                },
            },
            Field {
                id: "end",
                label: "End at",
                control: Control::Time {
                    get: |o| o.end,
                    set: |o, t| o.end = t,
                },
            },
            select(
                "speed",
                "Speed",
                choices(
                    "Normal",
                    [25, 50, 75, 125, 150, 200, 300, 400]
                        .map(|s| (s.to_string(), format!("{}x", f64::from(s) / 100.))),
                ),
                |o| id_of(o.speed),
                |o, id| o.speed = id.parse().ok(),
            ),
            select(
                "fade-in",
                "Fade in",
                fade_lengths(),
                |o| id_of(o.fade_in),
                |o, id| o.fade_in = id.parse().ok(),
            ),
            select(
                "fade-out",
                "Fade out",
                fade_lengths(),
                |o| id_of(o.fade_out),
                |o, id| o.fade_out = id.parse().ok(),
            ),
        ],
    }
}

fn strip_metadata() -> Field {
    check(
        "strip-metadata",
        "Remove metadata",
        |o| o.strip_metadata,
        |o, on| o.strip_metadata = on,
    )
}

/// The audio section for `to`: a video's soundtrack or an audio file.
fn audio(to: &Format) -> Section {
    let video = to.category == Category::Video;
    let mut fields = Vec::new();
    let codecs = audio_codec_choices(to.id);
    if !codecs.is_empty() {
        let usual = match to.id {
            "webm" => "Opus",
            "avi" => "MP3",
            _ => "AAC",
        };
        fields.push(select(
            "audio-codec",
            "Codec",
            choices(
                usual,
                codecs
                    .iter()
                    .filter(|c| c.name() != usual)
                    .map(|c| (c.id().into(), c.name().into())),
            ),
            |o| id_of(o.audio_codec),
            |o, id| o.audio_codec = id.parse().ok(),
        ));
    }
    if video {
        fields.push(select(
            "audio-bitrate",
            "Bitrate",
            choices(
                "Automatic",
                [320, 256, 192, 160, 128, 96, 64].map(|b| (b.to_string(), format!("{b} kbit/s"))),
            ),
            |o| id_of(o.audio_bitrate),
            |o, id| o.audio_bitrate = id.parse().ok(),
        ));
    }
    fields.push(select(
        "sample-rate",
        "Sample rate",
        choices(
            "Original",
            SAMPLE_RATES
                .iter()
                .map(|r| (r.to_string(), format!("{} kHz", f64::from(*r) / 1000.))),
        ),
        |o| id_of(o.sample_rate),
        |o, id| o.sample_rate = id.parse().ok(),
    ));
    fields.push(select(
        "channels",
        "Channels",
        choices(
            "Original",
            Channels::ALL
                .iter()
                .map(|c| (c.id().into(), c.name().into())),
        ),
        |o| id_of(o.channels),
        |o, id| o.channels = id.parse().ok(),
    ));
    if matches!(to.id, "wav" | "flac") {
        fields.push(select(
            "bit-depth",
            "Bit depth",
            choices("16-bit", [("24".to_string(), "24-bit".to_string())]),
            |o| id_of(o.bit_depth.filter(|b| *b != 16)),
            |o, id| o.bit_depth = id.parse().ok(),
        ));
    }
    fields.push(select(
        "volume",
        "Volume",
        choices(
            "Unchanged",
            [12, 6, 3, -3, -6, -12].map(|db: i8| (db.to_string(), format!("{db:+} dB"))),
        ),
        |o| id_of(o.volume_db),
        |o, id| o.volume_db = id.parse().ok(),
    ));
    fields.push(check(
        "normalize",
        "Even out loudness",
        |o| o.normalize,
        |o, on| o.normalize = on,
    ));
    if !video {
        fields.push(check(
            "trim-silence",
            "Cut silence at the ends",
            |o| o.trim_silence,
            |o, on| o.trim_silence = on,
        ));
        fields.push(strip_metadata());
    }
    Section {
        id: "audio",
        title: "Audio",
        fields,
    }
}

/// The advanced sections for converting to `to`; none for images and
/// documents. `audio` is false when the video's audio is left out.
pub fn sections(to: &Format, with_audio: bool) -> Vec<Section> {
    match to.category {
        Category::Video if to.id == "gif" => vec![picture(true), timing()],
        Category::Video => {
            let mut s = vec![encoding(to), picture(false), timing()];
            if with_audio {
                s.push(audio(to));
            }
            s
        }
        Category::Audio => vec![audio(to), timing()],
        _ => Vec::new(),
    }
}

/// Every advanced field, to clear the ones a target doesn't show.
pub fn every_field() -> Vec<Field> {
    ["mp4", "flac", "m4a", "gif"]
        .iter()
        .filter_map(|id| scyconvert_core::format_by_id(id))
        .flat_map(|to| sections(to, true))
        .flat_map(|s| s.fields)
        .collect()
}

/// Options keeping only the advanced fields `to` shows; the rest of
/// `options` is left alone.
pub fn for_target(options: &Options, to: &Format, with_audio: bool) -> Options {
    let mut out = options.clone();
    for field in every_field() {
        field.reset(&mut out);
    }
    for field in sections(to, with_audio).iter().flat_map(|s| &s.fields) {
        field.copy(options, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_round_trips_its_choices() {
        let fields = every_field();
        assert!(fields.len() >= 25, "{}", fields.len());
        for field in &fields {
            if let Control::Select { choices, get, set } = &field.control {
                for (id, label) in choices {
                    let mut o = Options::default();
                    set(&mut o, id);
                    assert_eq!(get(&o), id.as_ref(), "{} {label}", field.id);
                    assert!(o.validate().is_ok(), "{} {label}", field.id);
                }
            }
            let mut o = Options::default();
            field.reset(&mut o);
            assert!(!field.changed(&o), "{}", field.id);
        }
    }

    #[test]
    fn targets_keep_only_their_fields() {
        let options = Options {
            crop: Some(Aspect::Square),
            bit_depth: Some(24),
            trim_silence: true,
            quality: Some(70),
            ..Options::default()
        };
        let mp4 = scyconvert_core::format_by_id("mp4").unwrap();
        let flac = scyconvert_core::format_by_id("flac").unwrap();
        let kept = for_target(&options, mp4, true);
        assert_eq!(
            (kept.crop, kept.bit_depth, kept.trim_silence, kept.quality),
            (Some(Aspect::Square), None, false, Some(70))
        );
        let kept = for_target(&options, flac, true);
        assert_eq!(
            (kept.crop, kept.bit_depth, kept.trim_silence),
            (None, Some(24), true)
        );
        let png = scyconvert_core::format_by_id("png").unwrap();
        assert!(sections(png, true).is_empty());
    }
}
