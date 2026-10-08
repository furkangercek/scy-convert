//! One-click actions the menus offer next to plain targets: compressing to a
//! stated size, making audio mono, reversing a GIF and the like. An action is a target and
//! options chosen for the file at hand, so a compressed MKV stays an MKV.
//! Actions come in menus of their own, each a submenu in Explorer.

use crate::{Category, Channels, Format, Options, Playback, Registry, VideoCodec, format_by_id};

/// A submenu of actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionMenu {
    Compress,
    Audio,
    Gif,
}

impl ActionMenu {
    pub const ALL: [ActionMenu; 3] = [ActionMenu::Compress, ActionMenu::Audio, ActionMenu::Gif];

    /// The id the CLI's menu output uses.
    pub fn id(self) -> &'static str {
        match self {
            ActionMenu::Compress => "compress",
            ActionMenu::Audio => "audio",
            ActionMenu::Gif => "gif",
        }
    }

    /// The short name the app shows over its actions.
    pub fn name(self) -> &'static str {
        match self {
            ActionMenu::Compress => "Compress",
            ActionMenu::Audio => "Audio",
            ActionMenu::Gif => "Edit GIF",
        }
    }
}

/// The action that asks for its text first: the app opens a window for it,
/// and the CLI needs `--caption`.
pub const CAPTION: &str = "gif-caption";

/// Every action id, in menu order.
pub const ACTION_IDS: &[&str] = &[
    "compress-half",
    "compress-third",
    "compress-smallest",
    "compress-192",
    "compress-128",
    "compress-64",
    "extract-audio",
    "mute",
    "mono",
    "stereo",
    "normalize",
    "gif-reverse",
    "gif-boomerang",
    "gif-faster",
    "gif-fastest",
    "gif-slower",
    CAPTION,
];

/// What an action does to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionPlan {
    pub menu: ActionMenu,
    /// What the entry says for this file, amount included.
    pub label: String,
    pub to: &'static Format,
    pub options: Options,
    /// Added to the output's name when it keeps the input's format, so
    /// `song.wav` becomes `song-mono.wav`.
    pub suffix: Option<&'static str>,
}

fn format(id: &str) -> &'static Format {
    format_by_id(id).expect("a known format id")
}

/// Video containers a compressed video stays in; others become MP4.
const COMPRESS_KEEPS: &[&str] = &["mp4", "mkv", "mov", "webm"];
/// Lossless audio becomes MP3 when compressed; lossy audio keeps its format.
const LOSSLESS_AUDIO: &[&str] = &["wav", "flac", "aiff"];

/// What action `id` does to a file of `from`, or `None` if it doesn't apply.
pub fn plan(id: &str, from: &'static Format) -> Option<ActionPlan> {
    let video = from.category == Category::Video;
    let audio = from.category == Category::Audio;
    let gif = from.id == "gif";
    let plan = |menu, label: String, to, options, suffix| ActionPlan {
        menu,
        label,
        to,
        options,
        suffix,
    };
    let same =
        |menu, label: &str, options, suffix| plan(menu, label.into(), from, options, Some(suffix));
    let compress_video = |percent: u8, label: &str, height: Option<u32>| {
        let to = if COMPRESS_KEEPS.contains(&from.id) {
            from
        } else {
            format("mp4")
        };
        let label = if to == from {
            label.to_string()
        } else {
            format!("{label}, as MP4")
        };
        plan(
            ActionMenu::Compress,
            label,
            to,
            Options {
                size_percent: Some(percent),
                video_height: height,
                shrink_only: true,
                ..Options::default()
            },
            (to == from).then_some("compressed"),
        )
    };
    let compress_audio = |kbps: u32, label: &str| {
        let lossless = LOSSLESS_AUDIO.contains(&from.id);
        let to = if lossless { format("mp3") } else { from };
        let label = if lossless {
            format!("MP3 at {kbps} kbit/s ({label})")
        } else {
            format!("{kbps} kbit/s ({label})")
        };
        plan(
            ActionMenu::Compress,
            label,
            to,
            Options {
                audio_bitrate: Some(kbps),
                shrink_only: true,
                ..Options::default()
            },
            (!lossless).then_some("compressed"),
        )
    };
    let edit_gif = |label: &str, options, suffix| same(ActionMenu::Gif, label, options, suffix);
    let gif_speed = |percent: u16, label: &str, suffix| {
        edit_gif(
            label,
            Options {
                speed: Some(percent),
                ..Options::default()
            },
            suffix,
        )
    };
    Some(match id {
        "compress-half" if video => compress_video(50, "To about half the size", None),
        "compress-third" if video => compress_video(33, "To about a third of the size", None),
        "compress-smallest" if video => {
            compress_video(15, "Smallest: about 15% of the size, 720p", Some(720))
        }
        "compress-192" if audio => compress_audio(192, "high quality"),
        "compress-128" if audio => compress_audio(128, "good quality"),
        "compress-64" if audio => compress_audio(64, "for speech"),
        "extract-audio" if video => plan(
            ActionMenu::Audio,
            "Save the audio as MP3".into(),
            format("mp3"),
            Options::default(),
            None,
        ),
        "mute" if video => same(
            ActionMenu::Audio,
            "Remove the audio",
            Options {
                strip_audio: true,
                video_codec: Some(VideoCodec::Copy),
                ..Options::default()
            },
            "muted",
        ),
        "mono" if audio => same(
            ActionMenu::Audio,
            "Make mono",
            Options {
                channels: Some(Channels::Mono),
                ..Options::default()
            },
            "mono",
        ),
        "stereo" if audio => same(
            ActionMenu::Audio,
            "Make stereo",
            Options {
                channels: Some(Channels::Stereo),
                ..Options::default()
            },
            "stereo",
        ),
        "normalize" if audio || video => same(
            ActionMenu::Audio,
            "Even out loudness",
            Options {
                normalize: true,
                ..Options::default()
            },
            "normalized",
        ),
        "gif-reverse" if gif => edit_gif(
            "Reverse",
            Options {
                playback: Some(Playback::Reverse),
                ..Options::default()
            },
            "reversed",
        ),
        "gif-boomerang" if gif => edit_gif(
            "Play forward, then backward",
            Options {
                playback: Some(Playback::Boomerang),
                ..Options::default()
            },
            "boomerang",
        ),
        "gif-faster" if gif => gif_speed(150, "Speed up 1.5x", "1.5x"),
        "gif-fastest" if gif => gif_speed(200, "Speed up 2x", "2x"),
        "gif-slower" if gif => gif_speed(50, "Slow down to half speed", "0.5x"),
        CAPTION if gif => edit_gif("Add a caption...", Options::default(), "captioned"),
        _ => return None,
    })
}

/// The actions `registry` can run on a file of `from`, as `(id, plan)` in
/// menu order.
pub fn available(registry: &Registry, from: &'static Format) -> Vec<(&'static str, ActionPlan)> {
    ACTION_IDS
        .iter()
        .filter_map(|id| plan(id, from).map(|p| (*id, p)))
        .filter(|(_, p)| registry.plan(from, p.to).is_ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_fit_the_source() {
        let mkv = format("mkv");
        let half = plan("compress-half", mkv).unwrap();
        assert_eq!((half.to, half.suffix), (mkv, Some("compressed")));
        assert_eq!(half.options.size_percent, Some(50));
        assert!(half.options.shrink_only);
        // A container compression doesn't suit becomes MP4, under its own name.
        let avi = plan("compress-third", format("avi")).unwrap();
        assert_eq!((avi.to.id, avi.suffix), ("mp4", None));
        assert!(avi.label.ends_with("as MP4"));
        let wav = plan("compress-128", format("wav")).unwrap();
        assert_eq!(
            (wav.to.id, wav.label.as_str()),
            ("mp3", "MP3 at 128 kbit/s (good quality)")
        );
        assert_eq!(plan("compress-64", format("ogg")).unwrap().to.id, "ogg");
        assert!(plan("mono", mkv).is_none());
        assert!(plan("compress-half", format("mp3")).is_none());
        assert_eq!(
            plan("mono", format("flac")).unwrap().menu,
            ActionMenu::Audio
        );
        assert!(plan("nope", mkv).is_none());
        let gif = format("gif");
        let reversed = plan("gif-reverse", gif).unwrap();
        assert_eq!(
            (reversed.menu, reversed.to, reversed.suffix),
            (ActionMenu::Gif, gif, Some("reversed"))
        );
        assert_eq!(plan("gif-fastest", gif).unwrap().options.speed, Some(200));
        assert!(plan("gif-reverse", mkv).is_none());
        assert!(plan(CAPTION, format("png")).is_none());
        for id in ACTION_IDS {
            for f in crate::FORMATS {
                if let Some(p) = plan(id, f) {
                    assert!(p.options.validate().is_ok(), "{id} {}", f.id);
                }
            }
        }
    }
}
