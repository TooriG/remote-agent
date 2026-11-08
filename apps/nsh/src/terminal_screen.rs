//! The terminal off Apple platforms: the Host streams a terminal's bytes,
//! alacritty's parser keeps the screen they describe here, and NSH draws its
//! rows as styled text over an input that turns the keyboard back into the
//! bytes the terminal reads. Apple platforms hand the same bytes to SwiftTerm.
use agent_core::state::TerminalOutput;
use agent_protocol::operations::{TerminalSize, terminal_color};
use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::Dimensions,
    index::{Column, Line},
    term::{Config, TermMode, cell::Flags},
    vte::ansi::{Color, Processor},
};

/// The font the grid is drawn in, and the cell it lays each character in.
pub(crate) const FONT_SIZE: f32 = 13.0;
pub(crate) const LINE_HEIGHT: f32 = 18.0;
/// Consolas' advance is 0.55 of its size.
pub(crate) const CELL_WIDTH: f32 = FONT_SIZE * 0.55;

#[derive(Clone, Copy)]
struct Grid(TerminalSize);

impl Dimensions for Grid {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }
    fn screen_lines(&self) -> usize {
        usize::from(self.0.rows)
    }
    fn columns(&self) -> usize {
        usize::from(self.0.cols)
    }
}

/// The screen ignores what the parser reports back (titles, bells, replies):
/// the Host's terminal already answers its program.
struct Quiet;

impl EventListener for Quiet {
    fn send_event(&self, _: Event) {}
}

/// Characters drawn alike, side by side on one row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Run {
    pub text: String,
    pub foreground: u32,
    pub background: Option<u32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

/// What the grid shows: its rows' runs and the cursor's cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Frame {
    pub rows: Vec<Vec<Run>>,
    pub cursor: Option<(usize, usize)>,
}

pub(crate) struct Screen {
    term: Term<Quiet>,
    parser: Processor,
    size: TerminalSize,
    sequence: u64,
}

impl Screen {
    pub(crate) fn new(size: TerminalSize) -> Self {
        Self {
            term: Term::new(Config::default(), &Grid(size), Quiet),
            parser: Processor::new(),
            size,
            sequence: 0,
        }
    }

    /// Takes the chunks after the last one taken; a chunk with a reset size
    /// replaces the screen first, as the Host's replay of its checkpoint does.
    /// The last sequence taken, when it moved, is the acknowledgment to send.
    pub(crate) fn feed(&mut self, chunks: &[TerminalOutput]) -> Option<u64> {
        let before = self.sequence;
        for chunk in chunks.iter().filter(|chunk| chunk.sequence > before) {
            if let Some(size) = chunk.reset_size {
                self.term = Term::new(Config::default(), &Grid(size), Quiet);
                self.parser = Processor::new();
            }
            self.parser.advance(&mut self.term, &chunk.data);
            self.sequence = chunk.sequence;
        }
        // A replay at the Host's size keeps the size this view lays out.
        if self.term.columns() != usize::from(self.size.cols)
            || self.term.screen_lines() != usize::from(self.size.rows)
        {
            self.term.resize(Grid(self.size));
        }
        (self.sequence > before).then_some(self.sequence)
    }

    pub(crate) fn resize(&mut self, size: TerminalSize) {
        self.size = size;
        self.term.resize(Grid(size));
    }

    pub(crate) fn mode(&self) -> Mode {
        let mode = self.term.mode();
        Mode {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        }
    }

    pub(crate) fn frame(&self) -> Frame {
        let grid = self.term.grid();
        let rows = (0..self.term.screen_lines())
            .map(|line| {
                let row = &grid[Line(line as i32)];
                let mut runs: Vec<Run> = Vec::new();
                for column in 0..self.term.columns() {
                    let cell = &row[Column(column)];
                    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                        continue;
                    }
                    let mut foreground = self.color(cell.fg);
                    let mut background = self.color(cell.bg);
                    if cell.flags.contains(Flags::INVERSE) {
                        std::mem::swap(&mut foreground, &mut background);
                    }
                    let mut text = if cell.flags.contains(Flags::HIDDEN) {
                        " ".to_owned()
                    } else {
                        cell.c.to_string()
                    };
                    if let Some(marks) = cell.zerowidth() {
                        text.extend(marks);
                    }
                    let run = Run {
                        text,
                        foreground,
                        // The panel's own background shows through the default one.
                        background: (background
                            != self.color(Color::Named(
                                alacritty_terminal::vte::ansi::NamedColor::Background,
                            )))
                        .then_some(background),
                        bold: cell.flags.contains(Flags::BOLD),
                        italic: cell.flags.contains(Flags::ITALIC),
                        underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                        strikethrough: cell.flags.contains(Flags::STRIKEOUT),
                    };
                    match runs.last_mut() {
                        Some(last)
                            if Run {
                                text: last.text.clone(),
                                ..run.clone()
                            } == *last =>
                        {
                            last.text.push_str(&run.text)
                        }
                        _ => runs.push(run),
                    }
                }
                // Trailing blanks in the default look draw nothing.
                if let Some(last) = runs.last_mut()
                    && last.background.is_none()
                    && !last.underline
                    && !last.strikethrough
                {
                    last.text.truncate(last.text.trim_end_matches(' ').len());
                    if last.text.is_empty() {
                        runs.pop();
                    }
                }
                runs
            })
            .collect();
        let cursor = self.term.renderable_content().cursor.point;
        Frame {
            rows,
            cursor: self.term.mode().contains(TermMode::SHOW_CURSOR).then_some((
                usize::try_from(cursor.line.0).unwrap_or_default(),
                cursor.column.0,
            )),
        }
    }

    fn color(&self, color: Color) -> u32 {
        let index = match color {
            Color::Spec(rgb) => {
                return (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b);
            }
            Color::Indexed(index) => usize::from(index),
            Color::Named(name) => name as usize,
        };
        match self.term.colors()[index] {
            Some(rgb) => (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b),
            None => terminal_color(index as u16),
        }
    }
}

/// The terminal modes that change what keys and pastes send.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Mode {
    pub app_cursor: bool,
    pub bracketed_paste: bool,
}

/// The keys the terminal takes beside the text typed into its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Backspace,
    Escape,
    Tab,
    BackTab,
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    /// Ctrl and a letter: its control character.
    Control(char),
}

impl Key {
    /// The keystrokes NSH reports for each key, as its keymap names them.
    /// Ctrl+Q stays the platform's quit key and Ctrl+V the input's paste.
    pub(crate) fn bindings() -> Vec<(String, Self)> {
        let mut keys = vec![
            ("escape".to_owned(), Self::Escape),
            ("tab".into(), Self::Tab),
            ("shift+tab".into(), Self::BackTab),
            ("up".into(), Self::Up),
            ("down".into(), Self::Down),
            ("right".into(), Self::Right),
            ("left".into(), Self::Left),
            ("home".into(), Self::Home),
            ("end".into(), Self::End),
            ("pageup".into(), Self::PageUp),
            ("pagedown".into(), Self::PageDown),
            ("delete".into(), Self::Delete),
        ];
        keys.extend(
            ('a'..='z')
                .filter(|letter| !matches!(letter, 'q' | 'v'))
                .map(|letter| (format!("ctrl+{letter}"), Self::Control(letter))),
        );
        keys
    }

    /// The bytes a VT terminal reads for the key.
    pub(crate) fn bytes(self, mode: Mode) -> Vec<u8> {
        let cursor = |suffix: char| {
            if mode.app_cursor {
                format!("\x1bO{suffix}")
            } else {
                format!("\x1b[{suffix}")
            }
            .into_bytes()
        };
        match self {
            Self::Backspace => b"\x7f".to_vec(),
            Self::Escape => b"\x1b".to_vec(),
            Self::Tab => b"\t".to_vec(),
            Self::BackTab => b"\x1b[Z".to_vec(),
            Self::Up => cursor('A'),
            Self::Down => cursor('B'),
            Self::Right => cursor('C'),
            Self::Left => cursor('D'),
            Self::Home => cursor('H'),
            Self::End => cursor('F'),
            Self::PageUp => b"\x1b[5~".to_vec(),
            Self::PageDown => b"\x1b[6~".to_vec(),
            Self::Delete => b"\x1b[3~".to_vec(),
            Self::Control(letter) => vec![(letter as u8) & 0x1f],
        }
    }
}

/// Typed or pasted text as the terminal reads it: line breaks as returns, or
/// the whole text marked as a paste when the program asked for that. Escapes
/// cannot reach the program from pasted text.
pub(crate) fn text_bytes(text: &str, mode: Mode) -> Vec<u8> {
    let text = text.replace('\x1b', "");
    if mode.bracketed_paste && text.contains('\n') {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// The grid that fills `width` × `height` points, as the Host accepts it.
pub(crate) fn fit(width: f32, height: f32) -> Option<TerminalSize> {
    let cols = (width / CELL_WIDTH).floor();
    let rows = (height / LINE_HEIGHT).floor();
    (cols.is_finite() && rows.is_finite())
        .then(|| agent_core::presentation::terminals::terminal_grid(cols as i32, rows as i32))
        .flatten()
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub(crate) use draw::grid;

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
mod draw {
    use super::{CELL_WIDTH, FONT_SIZE, LINE_HEIGHT};
    use super::{Frame, Key, Mode, Run, Screen, fit, text_bytes};
    use crate::{shared::Shared, terminal::Report, ui::colors};
    use agent_core::state::TerminalOutput;
    use agent_protocol::operations::TerminalSize;
    use nsh_core::{element::rgb, prelude::*};
    use std::{cell::RefCell, rc::Rc, sync::Arc};

    fn row(runs: Vec<Run>, cursor: Option<usize>) -> AnyElement {
        let mut line = text("")
            .font_family(SharedString::from("monospace"))
            .text_size(px(FONT_SIZE))
            .line_height(nsh_core::LineHeight::Points(px(LINE_HEIGHT)))
            .number_of_lines(1)
            .h(px(LINE_HEIGHT));
        for run in runs {
            let mut span = text(run.text).text_color(rgb(run.foreground));
            if let Some(background) = run.background {
                span = span.bg(rgb(background));
            }
            if run.bold {
                span = span.font_weight(FontWeight::BOLD);
            }
            if run.italic {
                span = span.italic();
            }
            if run.underline {
                span = span.underline();
            }
            if run.strikethrough {
                span = span.line_through();
            }
            line = line.child(span);
        }
        view()
            .h(px(LINE_HEIGHT))
            .child(line)
            .child(cursor.map(|column| {
                view()
                    .absolute()
                    .top_0()
                    .left(px(column as f32 * CELL_WIDTH))
                    .w(px(2.0))
                    .h(px(LINE_HEIGHT))
                    .bg(colors::FOREGROUND)
            }))
            .into_element()
    }

    /// The grid of `output`, typing into the Host's terminal while `accepts`.
    pub(crate) fn grid(
        accepts: impl Fn() -> bool + Copy + 'static,
        output: impl Fn() -> Vec<TerminalOutput> + 'static,
        report: impl Fn(Report) + 'static,
    ) -> impl IntoElement {
        // Kept by the view, so each of its handlers can reach it.
        let report = stored(Rc::new(report) as Rc<dyn Fn(Report)>);
        let screen = Rc::new(RefCell::new(Screen::new(TerminalSize {
            cols: 80,
            rows: 24,
        })));
        let frame = signal(Shared(Arc::new(Frame::default())));
        let mode = signal(Mode::default());
        let focused = signal(false);
        let typed = signal(String::new());
        let field = element_ref();
        {
            let screen = screen.clone();
            effect(move || {
                let chunks = output();
                let mut screen = screen.borrow_mut();
                if let Some(sequence) = screen.feed(&chunks) {
                    frame.set(Shared(Arc::new(screen.frame())));
                    mode.set(screen.mode());
                    (report.get())(Report::Ack(sequence));
                }
            });
        }
        let send = move |bytes: Vec<u8>| {
            if !bytes.is_empty() {
                (report.get())(Report::Input(bytes));
            }
        };
        let resize = {
            let screen = screen.clone();
            move |bounds: &Bounds<Points>| {
                let Some(size) = fit(bounds.size.width.0, bounds.size.height.0) else {
                    return;
                };
                let mut screen = screen.borrow_mut();
                if screen.size != size {
                    screen.resize(size);
                    frame.set(Shared(Arc::new(screen.frame())));
                    (report.get())(Report::Resize(size));
                }
            }
        };
        view()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .p_2()
            .bg(colors::BACKGROUND)
            .label(crate::l10n::tr("ターミナル"))
            .on_press(move || field.focus())
            .child(view().flex_1().min_h_0().on_layout(resize).child(each(
                move || {
                    frame.with(|frame| {
                        frame
                            .rows
                            .iter()
                            .cloned()
                            .enumerate()
                            .map(|(index, runs)| {
                                let cursor = frame
                                    .cursor
                                    .filter(|(line, _)| *line == index && focused.get())
                                    .map(|(_, column)| column);
                                (index, runs, cursor)
                            })
                            .collect::<Vec<_>>()
                    })
                },
                |(index, runs, cursor): &(usize, Vec<Run>, Option<usize>)| {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    (runs, cursor).hash(&mut hasher);
                    (*index, hasher.finish())
                },
                |row_data, _| {
                    let (_, runs, cursor) = row_data.get_untracked();
                    row(runs, cursor)
                },
            )))
            // The keyboard's target: transparent over the grid, it takes typed
            // and pasted text, composition included. What it holds stays
            // selected, so each change is only the text just typed.
            .child(
                nsh_core::element::input()
                    .element_ref(&field)
                    .absolute()
                    .inset_0()
                    .opacity(0.0)
                    .caret(false)
                    .readonly(move || !accepts())
                    .multiline(true)
                    .auto_correct(false)
                    .auto_capitalize(AutoCapitalize::None)
                    .value(move || typed.get())
                    .on_focus(move || {
                        focused.set(true);
                        (report.get())(Report::Focus(true));
                    })
                    .on_blur(move || {
                        focused.set(false);
                        (report.get())(Report::Focus(false));
                    })
                    .on_change(move |text: &str| {
                        typed.set(text.to_owned());
                        if !text.is_empty() {
                            send(text_bytes(text, mode.get_untracked()));
                            field.set_selection(0, text.encode_utf16().count() as u32);
                        }
                    })
                    .on_key_press(move |key: &str| {
                        if key == "Backspace" {
                            send(Key::Backspace.bytes(mode.get_untracked()));
                        }
                    }),
            )
            // Keys that do not type text, bound only while the grid has the keyboard.
            .child(when(
                move || focused.get(),
                move || {
                    for (keystroke, key) in Key::bindings() {
                        on_key_with(
                            &keystroke,
                            KeyOptions {
                                when: nsh_core::keymap::When::Always,
                                repeat: true,
                            },
                            move |_| send(key.bytes(mode.get_untracked())),
                        );
                    }
                    view()
                },
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(sequence: u64, data: &[u8], reset: Option<TerminalSize>) -> TerminalOutput {
        TerminalOutput {
            sequence,
            data: data.to_vec(),
            reset_size: reset,
        }
    }

    fn text(frame: &Frame, row: usize) -> String {
        frame.rows[row]
            .iter()
            .map(|run| run.text.as_str())
            .collect()
    }

    #[test]
    fn a_screen_takes_each_chunk_once_and_acknowledges_the_last_one() {
        let mut screen = Screen::new(TerminalSize { cols: 20, rows: 3 });
        let first = [output(1, b"ab", None), output(2, b"c\r\nnext", None)];
        assert_eq!(screen.feed(&first), Some(2));
        // The publication still holds the taken chunks; nothing repeats.
        assert_eq!(screen.feed(&first), None);
        let frame = screen.frame();
        assert_eq!(text(&frame, 0), "abc");
        assert_eq!(text(&frame, 1), "next");
        assert_eq!(frame.cursor, Some((1, 4)));
    }

    #[test]
    fn a_reset_replaces_the_screen_and_keeps_the_size_laid_out() {
        let mut screen = Screen::new(TerminalSize { cols: 20, rows: 3 });
        screen.feed(&[output(1, b"old", None)]);
        let host = TerminalSize { cols: 10, rows: 2 };
        assert_eq!(screen.feed(&[output(2, b"new", Some(host))]), Some(2));
        let frame = screen.frame();
        assert_eq!(frame.rows.len(), 3);
        assert_eq!(text(&frame, 0), "new");
    }

    #[test]
    fn styles_split_runs_and_colors_resolve_through_the_shared_palette() {
        let mut screen = Screen::new(TerminalSize { cols: 20, rows: 1 });
        screen.feed(&[output(1, b"a\x1b[1;31mbc\x1b[0m\x1b[44md", None)]);
        let runs = &screen.frame().rows[0];
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[1].text, "bc");
        assert!(runs[1].bold);
        assert_eq!(runs[1].foreground, terminal_color(1));
        assert_eq!(runs[2].background, Some(terminal_color(4)));
        assert_eq!(runs[0].background, None);
    }

    #[test]
    fn keys_follow_the_cursor_mode_the_program_selected() {
        let mut screen = Screen::new(TerminalSize { cols: 20, rows: 1 });
        assert_eq!(Key::Up.bytes(screen.mode()), b"\x1b[A");
        screen.feed(&[output(1, b"\x1b[?1h", None)]);
        assert_eq!(Key::Up.bytes(screen.mode()), b"\x1bOA");
        assert_eq!(Key::Control('c').bytes(screen.mode()), [3]);
        assert!(
            Key::bindings()
                .iter()
                .all(|(keystroke, _)| keystroke != "ctrl+q" && keystroke != "ctrl+v")
        );
    }

    #[test]
    fn pasted_text_cannot_inject_escapes_and_is_marked_when_asked() {
        let plain = Mode::default();
        assert_eq!(text_bytes("a\nb\x1b[2J", plain), b"a\rb[2J");
        let marked = Mode {
            bracketed_paste: true,
            ..Mode::default()
        };
        assert_eq!(text_bytes("a\nb", marked), b"\x1b[200~a\nb\x1b[201~");
        assert_eq!(text_bytes("x", marked), b"x");
    }

    #[test]
    fn a_resized_screen_draws_its_new_grid() {
        let mut screen = Screen::new(TerminalSize { cols: 20, rows: 3 });
        screen.feed(&[output(1, b"abc", None)]);
        screen.resize(TerminalSize { cols: 10, rows: 5 });
        let frame = screen.frame();
        assert_eq!(frame.rows.len(), 5);
        assert_eq!(text(&frame, 0), "abc");
        assert_eq!(Key::Backspace.bytes(screen.mode()), [0x7f]);
    }

    #[test]
    fn the_grid_fills_whole_cells_within_the_host_limits() {
        let size = fit(CELL_WIDTH * 80.5, LINE_HEIGHT * 24.9).unwrap();
        assert_eq!((size.cols, size.rows), (80, 24));
        assert_eq!(fit(0.0, 0.0), None);
    }
}
