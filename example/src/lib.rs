//! The application logic runs unchanged in native tests and in the WASM worker.
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use tui2web::{
    app::{AppResult, Application, Context, Input, MouseKind, Update},
    fs::Filesystem,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub struct Editor {
    path: String,
    text: String,
    cursor: usize,
    dirty: bool,
    status: String,
}
tui2web::export_app!(Editor);

fn panes(ctx: &Context) -> (Rect, Rect, Rect) {
    let height = ctx.rows.saturating_sub(2);
    let sidebar = (ctx.columns / 3).min(26);
    (
        Rect::new(0, 0, sidebar, height),
        Rect::new(sidebar, 0, ctx.columns - sidebar, height),
        Rect::new(0, height, ctx.columns, ctx.rows - height),
    )
}
impl Editor {
    fn save(&mut self, ctx: &mut Context) -> AppResult<()> {
        ctx.fs
            .write_file(&self.path, self.text.as_bytes())
            .map_err(|e| e.to_string())?;
        self.dirty = false;
        self.status = format!("Saved {} at {:.0} ms", self.path, ctx.now_ms);
        Ok(())
    }
    fn open(&mut self, path: String, ctx: &mut Context) -> AppResult<()> {
        self.save(ctx)?;
        let text = ctx.fs.read_to_string(&path).map_err(|e| e.to_string())?;
        self.path = path;
        self.text = text;
        self.cursor = 0;
        Ok(())
    }
    fn position(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        (
            before.bytes().filter(|&b| b == b'\n').count(),
            UnicodeWidthStr::width(before.rsplit('\n').next().unwrap_or("")),
        )
    }
    fn scroll(&self, ctx: &Context) -> (usize, usize) {
        let (_, editor, _) = panes(ctx);
        let (row, col) = self.position();
        (
            row.saturating_sub(editor.height.saturating_sub(3) as usize),
            col.saturating_sub(editor.width.saturating_sub(3) as usize),
        )
    }
    fn move_to(&mut self, row: usize, column: usize) {
        let mut start = 0;
        for (index, line) in self.text.split('\n').enumerate() {
            if index == row {
                let mut width = 0;
                self.cursor = start;
                for grapheme in line.graphemes(true) {
                    let next = width + UnicodeWidthStr::width(grapheme);
                    if next > column {
                        break;
                    }
                    self.cursor += grapheme.len();
                    width = next;
                }
                return;
            }
            start += line.len() + 1;
        }
        self.cursor = self.text.len();
    }
    fn insert(&mut self, text: &str) -> AppResult<()> {
        let text = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ");
        let clean: String = text
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect();
        if self.text.len() + clean.len() > 256 * 1024 {
            return Err("editor file limit is 256 KiB".into());
        }
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
        self.dirty = true;
        Ok(())
    }
    fn handle(&mut self, input: Input, ctx: &mut Context) -> AppResult<Update> {
        let mut update = Update::render();
        match input {
            Input::Text { text } | Input::Paste { text } => self.insert(&text)?,
            Input::Key {
                key, modifiers: m, ..
            } => match key.as_str() {
                "s" | "S" if m.ctrl || m.meta => {
                    self.save(ctx)?;
                    update.files_changed = true;
                }
                "n" | "N" if m.ctrl || m.meta => {
                    self.save(ctx)?;
                    self.path = loop {
                        let path = format!("notes-{:08x}.txt", ctx.random_u32());
                        if !ctx.fs.exists(&path) {
                            break path;
                        }
                    };
                    self.text.clear();
                    self.cursor = 0;
                    self.save(ctx)?;
                    update.files_changed = true;
                }
                "q" | "Q" if m.ctrl => {
                    self.save(ctx)?;
                    update.files_changed = true;
                    update.exit = true;
                }
                "Tab" => {
                    let files = ctx.fs.list_files();
                    let index = files.iter().position(|p| p == &self.path).unwrap_or(0);
                    let next = if m.shift {
                        (index + files.len() - 1) % files.len()
                    } else {
                        (index + 1) % files.len()
                    };
                    self.open(files[next].clone(), ctx)?;
                    update.files_changed = true;
                }
                "ArrowLeft" => {
                    self.cursor = self.text[..self.cursor]
                        .grapheme_indices(true)
                        .next_back()
                        .map_or(0, |(i, _)| i);
                }
                "ArrowRight" => {
                    self.cursor += self.text[self.cursor..]
                        .graphemes(true)
                        .next()
                        .map_or(0, str::len);
                }
                "ArrowUp" | "ArrowDown" => {
                    let (row, col) = self.position();
                    self.move_to(
                        if key == "ArrowUp" {
                            row.saturating_sub(1)
                        } else {
                            row + 1
                        },
                        col,
                    );
                }
                "Home" => {
                    let (row, _) = self.position();
                    self.move_to(row, 0);
                }
                "End" => {
                    let (row, _) = self.position();
                    self.move_to(row, usize::MAX);
                }
                "Backspace" => {
                    let previous = self.text[..self.cursor]
                        .grapheme_indices(true)
                        .next_back()
                        .map_or(0, |(i, _)| i);
                    self.text.replace_range(previous..self.cursor, "");
                    self.cursor = previous;
                    self.dirty = true;
                }
                "Delete" => {
                    let next = self.text[self.cursor..]
                        .graphemes(true)
                        .next()
                        .map_or(0, str::len);
                    self.text.replace_range(self.cursor..self.cursor + next, "");
                    self.dirty = true;
                }
                "Enter" => self.insert("\n")?,
                _ => update.dirty = false,
            },
            Input::Mouse {
                kind: MouseKind::Down,
                column,
                row,
                button: 0,
                ..
            } => {
                let (files_pane, editor, _) = panes(ctx);
                let files = ctx.fs.list_files();
                let selected = files.iter().position(|p| p == &self.path).unwrap_or(0);
                let file_scroll =
                    selected.saturating_sub(files_pane.height.saturating_sub(3) as usize);
                if column > 0 && column < files_pane.width && row > 0 {
                    if let Some(path) = files.get(file_scroll + row as usize - 1) {
                        self.open(path.clone(), ctx)?;
                        update.files_changed = true;
                    }
                } else if column > editor.x && row > 0 && row < editor.height.saturating_sub(1) {
                    let (top, left) = self.scroll(ctx);
                    self.move_to(
                        top + row as usize - 1,
                        left + (column - editor.x - 1) as usize,
                    );
                }
            }
            Input::Focus { focused } => {
                self.status = if focused {
                    "Editor focused"
                } else {
                    "Editor unfocused"
                }
                .into();
            }
            _ => update.dirty = false,
        }
        Ok(update)
    }
}
impl Application for Editor {
    fn init(ctx: &mut Context) -> AppResult<Self> {
        #[cfg(target_arch = "wasm32")]
        console_error_panic_hook::set_once();
        if ctx.fs.list_files().is_empty() {
            ctx.fs
                .create_dir_all("notes/empty")
                .map_err(|e| e.to_string())?;
            ctx.fs.write_file("hello.txt", "Welcome to tui2web!\nEdit locally: 世界, café, e\u{301}.\nTab switches files. Ctrl+S saves.\n".as_bytes()).map_err(|e| e.to_string())?;
            ctx.fs
                .write_file(
                    "notes/todo.txt",
                    b"Build an adapted Ratatui app.\nNo server required.\n",
                )
                .map_err(|e| e.to_string())?;
        }
        let path = ctx.fs.list_files()[0].clone();
        let text = ctx.fs.read_to_string(&path).map_err(|e| e.to_string())?;
        Ok(Self {
            path,
            text,
            cursor: 0,
            dirty: false,
            status: ctx
                .config
                .get("greeting")
                .cloned()
                .unwrap_or_else(|| "Ready - files stay in this browser".into()),
        })
    }
    fn update(&mut self, input: Input, ctx: &mut Context) -> AppResult<Update> {
        // User file/limit errors are visible in the TUI, rather than fatal worker errors.
        match self.handle(input, ctx) {
            Ok(update) => Ok(update),
            Err(error) => {
                self.status = error;
                Ok(Update {
                    dirty: true,
                    files_changed: true,
                    ..Update::default()
                })
            }
        }
    }
    fn render(&self, frame: &mut Frame, ctx: &Context) {
        let (files_pane, editor, status) = panes(ctx);
        let files = ctx.fs.list_files();
        let selected = files.iter().position(|p| p == &self.path).unwrap_or(0);
        let file_scroll = selected.saturating_sub(files_pane.height.saturating_sub(3) as usize);
        let items: Vec<_> = files
            .iter()
            .skip(file_scroll)
            .map(|p| {
                ListItem::new(p.as_str()).style(if p == &self.path {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default()
                })
            })
            .collect();
        frame.render_widget(
            List::new(items).block(Block::default().borders(Borders::ALL).title(" Files ")),
            files_pane,
        );
        let (top, left) = self.scroll(ctx);
        // Crop before rendering: Paragraph's u16 scroll offsets cannot address large files.
        let visible: Vec<ratatui::text::Line> = self
            .text
            .split('\n')
            .skip(top)
            .take(editor.height.saturating_sub(2) as usize)
            .map(|line| {
                let mut column = 0;
                let mut text = String::new();
                for grapheme in line.graphemes(true) {
                    let end = column + UnicodeWidthStr::width(grapheme);
                    if end > left {
                        if column < left {
                            text.push_str(&" ".repeat(end - left));
                        } else {
                            text.push_str(grapheme);
                        }
                    }
                    column = end;
                    if column >= left + editor.width.saturating_sub(2) as usize {
                        break;
                    }
                }
                ratatui::text::Line::from(text)
            })
            .collect();
        frame.render_widget(
            Paragraph::new(visible).block(Block::default().borders(Borders::ALL).title(format!(
                " {}{} ",
                self.path,
                if self.dirty { " *" } else { "" }
            ))),
            editor,
        );
        frame.render_widget(
            Paragraph::new(format!(
                "{}\nCtrl+S save | Ctrl+N new | Tab file | Ctrl+Q exit",
                self.status
            )),
            status,
        );
        let (row, col) = self.position();
        if editor.width > 2 && editor.height > 2 {
            frame.set_cursor_position((editor.x + 1 + (col - left) as u16, 1 + (row - top) as u16));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui2web::app::{Command, Runner};
    #[test]
    fn scrolling_is_not_truncated_to_u16_offsets() {
        for fill in ["x", "\n"] {
            let mut runner = Runner::<Editor>::from_json(r#"{"version":1,"columns":80,"rows":24,"nowMs":0,"randomSeed":1,"config":{},"snapshot":null}"#).unwrap();
            runner.initial_output().unwrap();
            for _ in 0..2 {
                runner
                    .dispatch(Command::Event {
                        input: Input::Paste {
                            text: fill.repeat(40000),
                        },
                        now_ms: 1.0,
                    })
                    .unwrap();
            }
            let output = runner
                .dispatch(Command::Event {
                    input: Input::Text {
                        text: "TAIL".into(),
                    },
                    now_ms: 2.0,
                })
                .unwrap();
            assert!(output.frame.unwrap().contains("TAIL"));
        }
    }
    #[test]
    fn real_editor_protocol_round_trip() {
        let mut runner = Runner::<Editor>::from_json(r#"{"version":1,"columns":80,"rows":24,"nowMs":100,"randomSeed":1,"config":{},"snapshot":null}"#).unwrap();
        assert!(runner
            .initial_output()
            .unwrap()
            .frame
            .unwrap()
            .contains("hello.txt"));
        runner
            .dispatch(Command::Event {
                input: Input::Paste {
                    text: "世界e\u{301}".into(),
                },
                now_ms: 101.0,
            })
            .unwrap();
        let saved = runner.dispatch_json(r#"{"type":"event","nowMs":102,"input":{"type":"key","key":"s","code":"KeyS","repeat":false,"modifiers":{"ctrl":true,"alt":false,"meta":false,"shift":false}}}"#).unwrap();
        assert!(saved.contains("snapshot"));
        let snapshot = runner
            .dispatch(Command::Snapshot)
            .unwrap()
            .snapshot
            .unwrap();
        let bytes = &snapshot
            .files
            .iter()
            .find(|(p, _)| p == "hello.txt")
            .unwrap()
            .1;
        assert!(std::str::from_utf8(bytes)
            .unwrap()
            .starts_with("世界e\u{301}"));
        assert!(runner
            .dispatch(Command::Resize {
                columns: 0,
                rows: 1
            })
            .is_err());
        assert!(runner
            .dispatch(Command::Resize {
                columns: 40,
                rows: 10
            })
            .unwrap()
            .frame
            .is_some());
        assert!(runner
            .dispatch(Command::Event {
                input: Input::Tick,
                now_ms: 200.0
            })
            .unwrap()
            .frame
            .is_none());
        assert!(runner.dispatch(Command::Shutdown).unwrap().exited);
        assert!(runner.dispatch(Command::Snapshot).is_err());
    }
}
