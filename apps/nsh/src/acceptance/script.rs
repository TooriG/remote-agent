//! The step language of an acceptance scenario: one step per line, `#` starts a
//! comment, and an argument with spaces is quoted (`"..."`, with `\"` and `\\`).
//!
//! - `fixture <path>`: posts `<path>` to the fixture Host's control server
//!   (`crates/host-fixture/src/pairing.rs`). The runner posts leading ones before
//!   the app starts; the driver posts later ones when it reaches them
//!   (`/release-inputs` while a turn waits).
//! - `press <label> [after <anchor>] [timeout-ms]`: presses the first element
//!   whose label, title, value, identifier or placeholder is `<label>`, or an item
//!   of an open menu; a link in text is clicked. A `*` at either end of `<label>` matches any text there
//!   (`"*作業しました"`). With `after`, only elements after the first one labelled
//!   `<anchor>` in reading order count (a file row below a panel's header, not the
//!   same name in the conversation).
//! - `type <label> <text> [after <anchor>] [timeout-ms]`: focuses that element
//!   and inserts `<text>`.
//! - `key <name>`: a key with optional modifiers, as `return`, `escape` or `cmd+,`.
//! - `expect <text> [after <anchor>] [timeout-ms]` / `expect_absent ...`: waits
//!   until some element's text contains `<text>`, or none does.
//! - `scroll <label> <lines> [after <anchor>] [timeout-ms]`: a mouse wheel over
//!   that element; positive lines move the content up, toward its end.
//! - `select <label> <text> [after <anchor>] [timeout-ms]`: selects the first
//!   `<text>` inside that element's text, as a drag over it would.
//! - `menu <label> [after <anchor>] [timeout-ms]`: opens that element's context
//!   menu, as a right click on it; `press` then chooses its items.
//! - `choose <path>`: answers the open panel the app shows with `<path>`,
//!   waiting for one to appear. A leading `$AGENT_STATE_DIR` is the fixture Host's
//!   directory, whose `project` folder is the fixture's project.
//! - `expect_clipboard <text> [timeout-ms]`: waits until the clipboard's text
//!   contains `<text>`. The driver restores the clipboard when the scenario ends.
//! - `copy <text>` / `copy_image [text]`: puts `<text>` on the clipboard, or an
//!   image with `[text]` beside it, as a screenshot tool or a page copies one.
//! - `paste <label> [after <anchor>] [timeout-ms]`: focuses that element and
//!   pastes the clipboard into it, as Edit > Paste does (a posted ⌘V reaches
//!   no field while the app is in the background).
//! - `undo <label> [after <anchor>] [timeout-ms]`: focuses that element and
//!   undoes its last edit, as Edit > Undo does.
//! - `wait <ms>`, `screenshot <name>`, `dump <name>` (the accessibility tree as text).
// Off the Mac only the tests read it.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]
use std::time::Duration;

/// What a `choose` path may begin with: the fixture Host's directory.
pub(crate) const STATE_DIRECTORY: &str = "$AGENT_STATE_DIR";

/// How long a step waits for its element or text unless the line says otherwise.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    /// A control path the runner posted before the app started.
    Fixture(String),
    /// A control path the driver posts when it reaches the step.
    Control(String),
    Press(Find),
    Type(Find, String),
    Key(Key),
    Expect(Find),
    ExpectAbsent(Find),
    /// A wheel of this many lines over the element.
    Scroll(Find, i32),
    Select(Find, String),
    Menu(Find),
    Choose(String),
    ExpectClipboard(String, Duration),
    /// Text for the clipboard, with an image when `image`.
    Copy {
        text: String,
        image: bool,
    },
    Paste(Find),
    Undo(Find),
    Wait(Duration),
    Screenshot(String),
    Dump(String),
}

/// What a step looks for, and how long it waits for it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Find {
    pub text: String,
    /// Only elements after the first one labelled this count.
    pub after: Option<String>,
    pub timeout: Duration,
}

/// A key as AppKit reports it: its characters and virtual key code.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Key {
    pub characters: String,
    pub code: u16,
    pub command: bool,
    pub shift: bool,
    pub option: bool,
    pub control: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub number: usize,
    pub source: String,
    pub step: Step,
}

pub(crate) fn parse(script: &str) -> Result<Vec<Line>, String> {
    let mut lines = Vec::new();
    for (index, source) in script.lines().enumerate() {
        let number = index + 1;
        let source = source.trim();
        if source.is_empty() || source.starts_with('#') {
            continue;
        }
        let mut step = words(source)
            .and_then(|words| step(&words))
            .map_err(|reason| format!("line {number}: {reason}"))?;
        if let Step::Fixture(path) = &step
            && lines
                .iter()
                .any(|line: &Line| !matches!(line.step, Step::Fixture(_)))
        {
            step = Step::Control(path.clone());
        }
        lines.push(Line {
            number,
            source: source.to_owned(),
            step,
        });
    }
    if lines.is_empty() {
        return Err("the scenario has no steps".into());
    }
    Ok(lines)
}

fn words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut characters = line.chars().peekable();
    while let Some(&first) = characters.peek() {
        if first.is_whitespace() {
            characters.next();
            continue;
        }
        let mut word = String::new();
        if first == '"' {
            characters.next();
            loop {
                match characters.next() {
                    Some('"') => break,
                    Some('\\') => word.push(characters.next().ok_or("unfinished escape")?),
                    Some(character) => word.push(character),
                    None => return Err("unclosed quote".into()),
                }
            }
        } else {
            while let Some(&character) = characters.peek() {
                if character.is_whitespace() {
                    break;
                }
                word.push(character);
                characters.next();
            }
        }
        words.push(word);
    }
    Ok(words)
}

fn step(words: &[String]) -> Result<Step, String> {
    let arguments: Vec<&str> = words[1..].iter().map(String::as_str).collect();
    Ok(match (words[0].as_str(), arguments.as_slice()) {
        ("fixture", [path]) if path.starts_with('/') => Step::Fixture((*path).to_owned()),
        ("press", [target, rest @ ..]) => Step::Press(find(target, rest)?),
        ("type", [target, text, rest @ ..]) => Step::Type(find(target, rest)?, (*text).to_owned()),
        ("key", [name]) => Step::Key(key(name)?),
        ("expect", [text, rest @ ..]) => Step::Expect(find(text, rest)?),
        ("expect_absent", [text, rest @ ..]) => Step::ExpectAbsent(find(text, rest)?),
        ("scroll", [target, lines, rest @ ..]) => Step::Scroll(
            find(target, rest)?,
            lines
                .parse()
                .map_err(|_| format!("{lines:?} is not a line count"))?,
        ),
        ("select", [target, text, rest @ ..]) => {
            Step::Select(find(target, rest)?, (*text).to_owned())
        }
        ("menu", [target, rest @ ..]) => Step::Menu(find(target, rest)?),
        ("choose", [path]) if path.starts_with('/') || path.starts_with(STATE_DIRECTORY) => {
            Step::Choose((*path).to_owned())
        }
        ("expect_clipboard", [text]) => Step::ExpectClipboard((*text).to_owned(), DEFAULT_TIMEOUT),
        ("expect_clipboard", [text, timeout]) => {
            Step::ExpectClipboard((*text).to_owned(), milliseconds(timeout)?)
        }
        ("copy", [text]) => Step::Copy {
            text: (*text).to_owned(),
            image: false,
        },
        ("copy_image", []) => Step::Copy {
            text: String::new(),
            image: true,
        },
        ("copy_image", [text]) => Step::Copy {
            text: (*text).to_owned(),
            image: true,
        },
        ("paste", [target, rest @ ..]) => Step::Paste(find(target, rest)?),
        ("undo", [target, rest @ ..]) => Step::Undo(find(target, rest)?),
        ("wait", [milliseconds]) => Step::Wait(self::milliseconds(milliseconds)?),
        ("screenshot", [name]) => Step::Screenshot(artifact(name)?),
        ("dump", [name]) => Step::Dump(artifact(name)?),
        (name, _) => return Err(format!("unknown step or wrong arguments: {name}")),
    })
}

/// Whether an element's `text` is the `label` a step names, `*` at either end of
/// it matching any text there.
pub(crate) fn labels(text: &str, label: &str) -> bool {
    let text = text.trim();
    match (label.strip_prefix('*'), label.strip_suffix('*')) {
        (Some(rest), Some(_)) if !rest.is_empty() => text.contains(&rest[..rest.len() - 1]),
        (Some(rest), _) => text.ends_with(rest),
        (None, Some(rest)) => text.starts_with(rest),
        (None, None) => text == label,
    }
}

fn find(text: &str, rest: &[&str]) -> Result<Find, String> {
    let (after, rest) = match rest {
        ["after", anchor, rest @ ..] => (Some((*anchor).to_owned()), rest),
        _ => (None, rest),
    };
    let timeout = match rest {
        [] => DEFAULT_TIMEOUT,
        [value] => milliseconds(value)?,
        _ => return Err("too many arguments".into()),
    };
    Ok(Find {
        text: text.to_owned(),
        after,
        timeout,
    })
}

fn milliseconds(value: &str) -> Result<Duration, String> {
    value
        .parse()
        .map(Duration::from_millis)
        .map_err(|_| format!("{value:?} is not a number of milliseconds"))
}

/// A file name of the scenario's output directory.
fn artifact(name: &str) -> Result<String, String> {
    if !name.is_empty()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
    {
        Ok(name.to_owned())
    } else {
        Err(format!(
            "{name:?} must be letters, digits, '-' or '_' to name a file"
        ))
    }
}

fn key(spec: &str) -> Result<Key, String> {
    let mut parts: Vec<&str> = spec.split('+').collect();
    let name = parts
        .pop()
        .filter(|name| !name.is_empty())
        .ok_or("no key")?;
    let mut key = named(name).ok_or_else(|| format!("unknown key {name:?}"))?;
    for modifier in parts {
        let flag = match modifier {
            "cmd" => &mut key.command,
            "shift" => &mut key.shift,
            "option" | "alt" => &mut key.option,
            "ctrl" => &mut key.control,
            other => return Err(format!("unknown modifier {other:?}")),
        };
        *flag = true;
    }
    Ok(key)
}

/// ANSI virtual key codes of the keys scenarios name.
fn named(name: &str) -> Option<Key> {
    let function = |code: u16, unicode: u32| (char::from_u32(unicode).unwrap().to_string(), code);
    let (characters, code) = match name {
        "return" => ("\r".to_owned(), 36),
        "tab" => ("\t".to_owned(), 48),
        "space" => (" ".to_owned(), 49),
        "delete" => ("\u{7f}".to_owned(), 51),
        "escape" => ("\u{1b}".to_owned(), 53),
        "left" => function(123, 0xF702),
        "right" => function(124, 0xF703),
        "down" => function(125, 0xF701),
        "up" => function(126, 0xF700),
        _ => {
            let mut characters = name.chars();
            let character = characters.next()?;
            if characters.next().is_some() {
                return None;
            }
            const CODES: &str = "asdfhgzxcv\0bqweryt123465=97-80]ou[ip\0lj'k;\\,/nm.";
            let code = CODES.find(character.to_ascii_lowercase())?;
            (character.to_ascii_lowercase().to_string(), code as u16)
        }
    };
    Some(Key {
        characters,
        code,
        command: false,
        shift: false,
        option: false,
        control: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_take_quoted_labels_and_optional_timeouts() {
        let lines = parse(
            "# Comment\nfixture /external-conversation\n\npress \"新しい チャット\"\n\
             type メッセージ \"say \\\"hi\\\"\" 500\nexpect done after 見出し 20000\nexpect_absent x\n\
             scroll 会話 -3\nselect 回答 Needle\nmenu 回答 after 見出し\nchoose /tmp/a.txt\nchoose $AGENT_STATE_DIR/project\n\
             expect_clipboard \"a b\" 500\ncopy \"a b\"\ncopy_image\ncopy_image 説明\npaste 入力欄 after 見出し\nundo 入力欄 500\n\
             fixture /release-inputs\nwait 250\nscreenshot after-send\ndump tree_1\n",
        )
        .unwrap();
        let steps: Vec<_> = lines.iter().map(|line| line.step.clone()).collect();
        assert_eq!(
            steps,
            [
                Step::Fixture("/external-conversation".into()),
                Step::Press(Find {
                    text: "新しい チャット".into(),
                    after: None,
                    timeout: DEFAULT_TIMEOUT
                }),
                Step::Type(
                    Find {
                        text: "メッセージ".into(),
                        after: None,
                        timeout: Duration::from_millis(500)
                    },
                    "say \"hi\"".into()
                ),
                Step::Expect(Find {
                    text: "done".into(),
                    after: Some("見出し".into()),
                    timeout: Duration::from_secs(20)
                }),
                Step::ExpectAbsent(Find {
                    text: "x".into(),
                    after: None,
                    timeout: DEFAULT_TIMEOUT
                }),
                Step::Scroll(
                    Find {
                        text: "会話".into(),
                        after: None,
                        timeout: DEFAULT_TIMEOUT
                    },
                    -3
                ),
                Step::Select(
                    Find {
                        text: "回答".into(),
                        after: None,
                        timeout: DEFAULT_TIMEOUT
                    },
                    "Needle".into()
                ),
                Step::Menu(Find {
                    text: "回答".into(),
                    after: Some("見出し".into()),
                    timeout: DEFAULT_TIMEOUT
                }),
                Step::Choose("/tmp/a.txt".into()),
                Step::Choose("$AGENT_STATE_DIR/project".into()),
                Step::ExpectClipboard("a b".into(), Duration::from_millis(500)),
                Step::Copy {
                    text: "a b".into(),
                    image: false
                },
                Step::Copy {
                    text: String::new(),
                    image: true
                },
                Step::Copy {
                    text: "説明".into(),
                    image: true
                },
                Step::Paste(Find {
                    text: "入力欄".into(),
                    after: Some("見出し".into()),
                    timeout: DEFAULT_TIMEOUT
                }),
                Step::Undo(Find {
                    text: "入力欄".into(),
                    after: None,
                    timeout: Duration::from_millis(500)
                }),
                Step::Control("/release-inputs".into()),
                Step::Wait(Duration::from_millis(250)),
                Step::Screenshot("after-send".into()),
                Step::Dump("tree_1".into()),
            ]
        );
        assert_eq!(lines[0].number, 2);
        assert_eq!(lines[1].source, "press \"新しい チャット\"");
    }

    #[test]
    fn keys_carry_modifiers_and_appkit_codes() {
        let Step::Key(key) = step(&["key".into(), "cmd+shift+n".into()]).unwrap() else {
            panic!("not a key");
        };
        assert_eq!((key.characters.as_str(), key.code), ("n", 45));
        assert!(key.command && key.shift && !key.option && !key.control);
        for (name, characters, code) in [
            ("return", "\r", 36),
            ("escape", "\u{1b}", 53),
            ("a", "a", 0),
            (",", ",", 43),
            ("down", "\u{F701}", 125),
        ] {
            let key = super::key(name).unwrap();
            assert_eq!((key.characters.as_str(), key.code), (characters, code));
        }
        assert!(super::key("cmd+").is_err());
        assert!(super::key("hyper+a").is_err());
        assert!(super::key("f13").is_err());
    }

    #[test]
    fn labels_match_whole_text_unless_starred() {
        for (text, label, expected) in [
            ("送信", "送信", true),
            (" 送信 ", "送信", true),
            ("送信中", "送信", false),
            ("3秒 作業しました", "*作業しました", true),
            ("3秒 作業しました", "3秒*", true),
            ("3秒 作業しました", "*秒 作*", true),
            ("3秒 作業しました", "*作業中*", false),
            ("作業", "*", true),
            ("x", "**", true),
        ] {
            assert_eq!(labels(text, label), expected, "{text:?} {label:?}");
        }
    }

    #[test]
    fn malformed_scenarios_name_the_line() {
        for (script, reason) in [
            ("", "the scenario has no steps"),
            ("press", "line 1: unknown step or wrong arguments: press"),
            ("tap x", "line 1: unknown step or wrong arguments: tap"),
            ("expect \"open", "line 1: unclosed quote"),
            (
                "wait soon",
                "line 1: \"soon\" is not a number of milliseconds",
            ),
            ("expect a 1 2", "line 1: too many arguments"),
            (
                "press a after",
                "line 1: \"after\" is not a number of milliseconds",
            ),
            (
                "screenshot ../escape",
                "line 1: \"../escape\" must be letters, digits, '-' or '_' to name a file",
            ),
            (
                "choose a.txt",
                "line 1: unknown step or wrong arguments: choose",
            ),
            (
                "expect_clipboard a b c",
                "line 1: unknown step or wrong arguments: expect_clipboard",
            ),
            ("copy", "line 1: unknown step or wrong arguments: copy"),
            (
                "copy_image a b",
                "line 1: unknown step or wrong arguments: copy_image",
            ),
            (
                "fixture release-inputs",
                "line 1: unknown step or wrong arguments: fixture",
            ),
        ] {
            assert_eq!(parse(script).unwrap_err(), reason, "{script}");
        }
    }
}
