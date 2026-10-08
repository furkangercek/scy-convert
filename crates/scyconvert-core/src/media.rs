//! Option values for video and audio: codecs, encoders and picture changes,
//! and the time and frame-rate values presets and the CLI write as text.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// An enum of named choices. Each value has an id (presets, the CLI, the
/// app's controls) and a name people see; extra spellings parse too.
macro_rules! choices {
    (
        $(#[$meta:meta])*
        pub enum $name:ident ($what:literal) {
            $( $(#[$vmeta:meta])* $variant:ident = $id:literal, $label:literal $(, $alias:literal)*; )+
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The id presets and the CLI use.
            pub fn id(self) -> &'static str {
                match self { $($name::$variant => $id,)+ }
            }

            /// The name people see.
            pub fn name(self) -> &'static str {
                match self { $($name::$variant => $label,)+ }
            }
        }

        impl std::str::FromStr for $name {
            type Err = Error;

            fn from_str(s: &str) -> Result<Self> {
                match s.trim().to_ascii_lowercase().as_str() {
                    $( $id $(| $alias)* => Ok($name::$variant), )+
                    _ => Err(Error::InvalidOption(format!(
                        "{} {s:?}; use {}",
                        $what,
                        [$($id),+].join(", ")
                    ))),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.id())
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
                s.serialize_str(self.id())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

choices! {
    /// A video encoder. Which ones a container takes is up to the engine.
    pub enum VideoCodec ("video codec") {
        /// H.264 (AVC): plays nearly everywhere.
        H264 = "h264", "H.264", "h.264", "avc", "x264";
        /// H.265 (HEVC): smaller files, newer players.
        Hevc = "hevc", "HEVC", "h265", "h.265", "x265";
        /// AV1: smallest files, slow to encode without hardware.
        Av1 = "av1", "AV1";
        /// VP9: WebM's codec.
        Vp9 = "vp9", "VP9";
        /// Apple ProRes 422 HQ: large, for editing.
        ProRes = "prores", "ProRes";
        /// Keep the video stream as it is, without re-encoding.
        Copy = "copy", "Copy (no re-encode)";
    }
}

choices! {
    /// An audio encoder for the audio inside video, and for M4A.
    pub enum AudioCodec ("audio codec") {
        Aac = "aac", "AAC";
        Opus = "opus", "Opus";
        Mp3 = "mp3", "MP3";
        Ac3 = "ac3", "AC-3 (Dolby Digital)";
        Flac = "flac", "FLAC";
        Alac = "alac", "ALAC (Apple Lossless)";
        /// Keep the audio stream as it is, without re-encoding.
        Copy = "copy", "Copy (no re-encode)";
    }
}

choices! {
    /// A hardware video encoder. Unset encodes in software.
    pub enum Hardware ("hardware encoder") {
        Nvidia = "nvenc", "NVIDIA (NVENC)", "nvidia";
        Intel = "qsv", "Intel (Quick Sync)", "intel", "quicksync";
        Amd = "amf", "AMD (AMF)", "amd";
        Apple = "videotoolbox", "Apple (VideoToolbox)", "apple";
    }
}

choices! {
    /// How long the encoder may take. Slower makes smaller files at the
    /// same quality.
    pub enum EncoderSpeed ("encoder speed") {
        Fastest = "fastest", "Fastest";
        Fast = "fast", "Fast";
        Medium = "medium", "Medium";
        Slow = "slow", "Slow";
        Slowest = "slowest", "Slowest";
    }
}

choices! {
    /// An aspect ratio to crop the picture to, around its center.
    pub enum Aspect ("aspect ratio") {
        Wide = "16:9", "16:9";
        Tall = "9:16", "9:16 (vertical)";
        Square = "1:1", "1:1 (square)";
        Standard = "4:3", "4:3";
        Portrait = "4:5", "4:5";
        Cinema = "21:9", "21:9";
    }
}

impl Aspect {
    /// Width and height of the ratio.
    pub fn ratio(self) -> (u32, u32) {
        let (w, h) = self.id().split_once(':').expect("ids are w:h");
        (w.parse().expect("digits"), h.parse().expect("digits"))
    }
}

choices! {
    /// A clockwise rotation.
    pub enum Rotation ("rotation") {
        Right = "90", "90° clockwise", "cw";
        Half = "180", "180°";
        Left = "270", "90° counterclockwise", "ccw", "-90";
    }
}

choices! {
    /// A mirror image.
    pub enum Flip ("flip") {
        Horizontal = "horizontal", "Horizontal", "h";
        Vertical = "vertical", "Vertical", "v";
        Both = "both", "Both";
    }
}

choices! {
    /// Audio channel layout.
    pub enum Channels ("channels") {
        Mono = "mono", "Mono", "1";
        Stereo = "stereo", "Stereo", "2";
    }
}

/// A time or duration with millisecond precision. Written `90`, `1.5`,
/// `1:30` or `1:02:03.250`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    pub millis: u64,
}

impl Timestamp {
    pub fn from_secs(secs: u64) -> Self {
        Timestamp {
            millis: secs * 1000,
        }
    }

    pub fn secs(self) -> f64 {
        self.millis as f64 / 1000.
    }

    /// Seconds with up to three decimals, as FFmpeg reads them: `83.5`.
    pub fn ffmpeg(self) -> String {
        trim_decimals(format!("{}.{:03}", self.millis / 1000, self.millis % 1000))
    }
}

fn trim_decimals(s: String) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

impl std::str::FromStr for Timestamp {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let bad =
            || Error::InvalidOption(format!("time {s:?}; use seconds like 90 or 1.5, or 1:30"));
        let parts: Vec<&str> = s.trim().split(':').collect();
        if parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
            return Err(bad());
        }
        let (whole, last) = parts.split_at(parts.len() - 1);
        let mut millis: u64 = 0;
        for part in whole {
            let n: u64 = part.parse().map_err(|_| bad())?;
            millis = (millis + n) * 60;
        }
        millis *= 1000;
        let (secs, frac) = last[0].split_once('.').unwrap_or((last[0], ""));
        let secs: u64 = secs.parse().map_err(|_| bad())?;
        if !whole.is_empty() && secs >= 60
            || frac.len() > 3
            || !frac.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(bad());
        }
        let frac: u64 = format!("{frac:0<3}").parse().map_err(|_| bad())?;
        Ok(Timestamp {
            millis: millis + secs * 1000 + frac,
        })
    }
}

impl std::fmt::Display for Timestamp {
    /// `45`, `1.5`, `1:30` or `1:02:03.25`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let total = self.millis / 1000;
        let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
        let frac = trim_decimals(format!(".{:03}", self.millis % 1000)).replace('.', "");
        let frac = if frac.is_empty() {
            frac
        } else {
            format!(".{frac}")
        };
        match (h, m) {
            (0, 0) => write!(f, "{s}{frac}"),
            (0, m) => write!(f, "{m}:{s:02}{frac}"),
            (h, m) => write!(f, "{h}:{m:02}:{s:02}{frac}"),
        }
    }
}

impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    /// A string, or a whole number of seconds.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Secs(u64),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Secs(secs) => Ok(Timestamp::from_secs(secs)),
            Raw::Text(text) => text.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// Frames per second, to two decimals: `30`, `29.97`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameRate {
    pub hundredths: u32,
}

impl FrameRate {
    /// As FFmpeg reads it: `30` or `2997/100`.
    pub fn ffmpeg(self) -> String {
        if self.hundredths.is_multiple_of(100) {
            (self.hundredths / 100).to_string()
        } else {
            format!("{}/100", self.hundredths)
        }
    }
}

impl std::str::FromStr for FrameRate {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let bad =
            || Error::InvalidOption(format!("frame rate {s:?}; use a number like 30 or 29.97"));
        let (whole, frac) = s.trim().split_once('.').unwrap_or((s.trim(), ""));
        if frac.len() > 2 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let whole: u32 = whole.parse().map_err(|_| bad())?;
        let frac: u32 = format!("{frac:0<2}").parse().map_err(|_| bad())?;
        Ok(FrameRate {
            hundredths: whole.checked_mul(100).ok_or_else(bad)? + frac,
        })
    }
}

impl std::fmt::Display for FrameRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&trim_decimals(format!(
            "{}.{:02}",
            self.hundredths / 100,
            self.hundredths % 100
        )))
    }
}

impl Serialize for FrameRate {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for FrameRate {
    /// A string, or a whole number.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Whole(u32),
            Text(String),
        }
        match Raw::deserialize(d)? {
            Raw::Whole(n) => Ok(FrameRate {
                hundredths: n.saturating_mul(100),
            }),
            Raw::Text(text) => text.parse().map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        for (text, millis, shown) in [
            ("90", 90_000, "1:30"),
            ("1.5", 1_500, "1.5"),
            ("1:30", 90_000, "1:30"),
            ("0:05.25", 5_250, "5.25"),
            ("1:02:03.250", 3_723_250, "1:02:03.25"),
            ("0", 0, "0"),
        ] {
            let t: Timestamp = text.parse().unwrap();
            assert_eq!(t.millis, millis, "{text}");
            assert_eq!(t.to_string(), shown, "{text}");
            assert_eq!(t.to_string().parse::<Timestamp>().unwrap(), t);
        }
        assert_eq!("83.5".parse::<Timestamp>().unwrap().ffmpeg(), "83.5");
        assert_eq!("2".parse::<Timestamp>().unwrap().ffmpeg(), "2");
        for bad in [
            "", "a", "1:", ":5", "1:60", "1.2345", "-1", "1:2:3:4", "1.x",
        ] {
            assert!(bad.parse::<Timestamp>().is_err(), "{bad}");
        }
    }

    #[test]
    fn frame_rates() {
        let f: FrameRate = "29.97".parse().unwrap();
        assert_eq!(
            (f.hundredths, f.ffmpeg(), f.to_string()),
            (2997, "2997/100".into(), "29.97".into())
        );
        let f: FrameRate = "60".parse().unwrap();
        assert_eq!((f.ffmpeg(), f.to_string()), ("60".into(), "60".into()));
        for bad in ["", "x", "29.976", "-1", "1.a"] {
            assert!(bad.parse::<FrameRate>().is_err(), "{bad}");
        }
    }

    #[test]
    fn choices_round_trip() {
        for codec in VideoCodec::ALL {
            assert_eq!(codec.id().parse::<VideoCodec>().unwrap(), *codec);
        }
        assert_eq!("H.265".parse::<VideoCodec>().unwrap(), VideoCodec::Hevc);
        assert_eq!("cw".parse::<Rotation>().unwrap(), Rotation::Right);
        assert_eq!(Aspect::Tall.ratio(), (9, 16));
        let e = "vp8".parse::<VideoCodec>().unwrap_err().to_string();
        assert!(e.contains("h264, hevc, av1, vp9, prores, copy"), "{e}");
    }
}
