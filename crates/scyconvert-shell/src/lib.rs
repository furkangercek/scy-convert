//! Explorer commands. Only the installed CLI probes engines; the DLL never
//! loads codecs into Explorer or duplicates the registry's routing policy.
#[cfg(windows)]
mod windows;

/// The top-level menus, each its own COM class and Explorer verb, in the
/// order they appear: Windows 11 shows one level of submenu, so each kind of
/// command gets an entry of its own.
pub const MENUS: [(&str, &str); 3] = [
    ("convert", "Convert with scyconvert"),
    ("compress", "Compress with scyconvert"),
    ("audio", "Adjust audio with scyconvert"),
];

/// A target format or an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Greyed text over the entries below it: "Video", "Audio only".
    Heading(String),
    Format(Item),
    Action(Item),
    Separator,
}

/// One top-level menu's entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submenu {
    pub id: String,
    pub entries: Vec<Entry>,
}

fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn valid_label(label: &str) -> bool {
    (1..=64).contains(&label.chars().count()) && !label.chars().any(char::is_control)
}

/// Reads `scyconvert targets <file> --menu`: tab-separated `menu <id>
/// <title>` lines, each followed by its `heading <text>`, `format <id>
/// <name>`, `action <id> <label>` and `separator` lines. Anything else
/// rejects the whole output, so a broken CLI shows no menu rather than a
/// wrong one.
pub fn parse_menus(text: &str) -> Option<Vec<Submenu>> {
    let mut menus: Vec<Submenu> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let fields: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        let entry = match fields[..] {
            ["menu", id, title] if valid_id(id) && valid_label(title) => {
                menus.push(Submenu {
                    id: id.into(),
                    entries: Vec::new(),
                });
                continue;
            }
            ["heading", text] if valid_label(text) => Entry::Heading(text.into()),
            ["format", id, label] if valid_id(id) && valid_label(label) => Entry::Format(Item {
                id: id.into(),
                label: label.into(),
            }),
            ["action", id, label] if valid_id(id) && valid_label(label) => Entry::Action(Item {
                id: id.into(),
                label: label.into(),
            }),
            ["separator"] => Entry::Separator,
            _ => return None,
        };
        menus.last_mut()?.entries.push(entry);
    }
    Some(menus)
}

/// Drops headings with nothing under them and separators with nothing on
/// one side.
fn tidy(entries: Vec<Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for entry in entries {
        if matches!(entry, Entry::Separator | Entry::Heading(_)) {
            // A heading or separator right before another one has nothing
            // under it.
            if let Some(Entry::Heading(_)) = out.last() {
                out.pop();
            }
            if matches!(entry, Entry::Separator)
                && matches!(out.last(), None | Some(Entry::Separator))
            {
                continue;
            }
        }
        out.push(entry);
    }
    while matches!(out.last(), Some(Entry::Separator | Entry::Heading(_))) {
        out.pop();
    }
    out
}

/// What every file in a selection can do, in the first file's order and
/// with its labels.
pub fn common_menus(per_file: &[Vec<Submenu>]) -> Vec<Submenu> {
    let Some((first, rest)) = per_file.split_first() else {
        return Vec::new();
    };
    let has = |menus: &Vec<Submenu>, menu: &str, entry: &Entry| {
        menus.iter().filter(|m| m.id == menu).any(|m| {
            m.entries.iter().any(|e| match (e, entry) {
                (Entry::Format(a), Entry::Format(b)) | (Entry::Action(a), Entry::Action(b)) => {
                    a.id == b.id
                }
                _ => false,
            })
        })
    };
    first
        .iter()
        .map(|menu| Submenu {
            id: menu.id.clone(),
            entries: tidy(
                menu.entries
                    .iter()
                    .filter(|e| match e {
                        Entry::Format(_) | Entry::Action(_) => {
                            rest.iter().all(|m| has(m, &menu.id, e))
                        }
                        _ => true,
                    })
                    .cloned()
                    .collect(),
            ),
        })
        .filter(|m| !m.entries.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: &str = "menu\tconvert\tConvert with scyconvert\nheading\tVideo\nformat\tmkv\tMKV\nformat\twmv\tWMV\nseparator\nheading\tAudio only\nformat\tmp3\tMP3\nmenu\tcompress\tCompress with scyconvert\naction\tcompress-half\tTo about half the size\n";

    #[test]
    fn reads_the_cli_menus() {
        let menus = parse_menus(VIDEO).unwrap();
        assert_eq!(menus.len(), 2);
        assert_eq!(menus[0].id, "convert");
        assert_eq!(menus[0].entries[0], Entry::Heading("Video".into()));
        assert_eq!(menus[0].entries[3], Entry::Separator);
        assert!(matches!(&menus[1].entries[0], Entry::Action(a) if a.id == "compress-half"));
        assert_eq!(parse_menus("").unwrap(), vec![]);
        for bad in [
            "format\tmp4\tMP4",
            "menu\tconvert\tConvert\naction\tCOMPRESS\tCompress",
            "menu\tconvert\tConvert\naction\tcompress",
            "something\telse",
            "menu\tconvert\tConvert\nheading\t",
        ] {
            assert!(parse_menus(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn mixed_selection_keeps_what_every_file_can_do() {
        let video = parse_menus(VIDEO).unwrap();
        // An MKV can't become MKV, and has no MP3 here.
        let mkv = parse_menus(
            "menu\tconvert\tConvert\nheading\tVideo\nformat\twmv\tWMV\nmenu\tcompress\tCompress\naction\tcompress-half\tHalf\n",
        )
        .unwrap();
        let common = common_menus(&[video.clone(), mkv]);
        assert_eq!(
            common[0].entries,
            [
                Entry::Heading("Video".into()),
                Entry::Format(Item {
                    id: "wmv".into(),
                    label: "WMV".into()
                })
            ]
        );
        assert_eq!(common[1].id, "compress");
        assert!(common_menus(&[video, vec![]]).is_empty());
        assert!(common_menus(&[]).is_empty());
    }
}
