use ratatui::{
    backend::{Backend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
    style::{Color, Modifier},
};
use std::io;
use unicode_width::UnicodeWidthStr;

/// A ratatui [`Backend`] that renders terminal frames as ANSI escape-code strings
/// suitable for display in a web-based terminal emulator such as xterm.js.
///
/// After every call to [`ratatui::Terminal::draw`] the resulting frame can be
/// retrieved with [`WebBackend::get_ansi_output`] and written directly to an
/// xterm.js instance.
pub struct WebBackend {
    width: u16,
    height: u16,
    /// Flat, row-major cell buffer (index = y * width + x).
    cells: Vec<Cell>,
    cursor_x: u16,
    cursor_y: u16,
    cursor_visible: bool,
    /// Last serialised ANSI frame, updated on every [`Backend::flush`].
    ansi_output: String,
}

impl WebBackend {
    /// Create a new backend with the given terminal dimensions (columns × rows).
    pub fn new(width: u16, height: u16) -> Self {
        WebBackend {
            width,
            height,
            cells: vec![Cell::default(); usize::from(width) * usize::from(height)],
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: true,
            ansi_output: String::new(),
        }
    }

    /// Return the ANSI escape-code string produced by the most recent frame flush.
    pub fn get_ansi_output(&self) -> &str {
        &self.ansi_output
    }

    /// Resize the internal cell buffer to new dimensions.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.cells = vec![Cell::default(); usize::from(width) * usize::from(height)];
    }

    /// Serialise the current cell buffer into a complete ANSI escape-code string.
    fn render_to_ansi(&self) -> String {
        let capacity = usize::from(self.width) * usize::from(self.height) * 4;
        let mut out = String::with_capacity(capacity);

        // Hide cursor during render to avoid flicker.
        // Full dirty frames, not a byte stream: reset external state and disable autowrap
        // so writing the bottom-right cell cannot scroll the screen.
        out.push_str("\x1b[?25l\x1b[?7l\x1b[0m\x1b[2J");

        let mut prev_fg = Color::Reset;
        let mut prev_bg = Color::Reset;
        let mut prev_modifier = Modifier::empty();

        for y in 0..self.height {
            // Move cursor to start of row (1-based ANSI coordinates).
            out.push_str("\x1b[");
            push_u16(&mut out, y + 1);
            out.push_str(";1H");

            let mut x = 0;
            while x < self.width {
                let cell = &self.cells[usize::from(y) * usize::from(self.width) + usize::from(x)];
                let fg = cell.fg;
                let bg = cell.bg;
                let modifier = cell.modifier;

                if fg != prev_fg || bg != prev_bg || modifier != prev_modifier {
                    out.push_str("\x1b[0m");

                    if modifier.contains(Modifier::BOLD) {
                        out.push_str("\x1b[1m");
                    }
                    if modifier.contains(Modifier::DIM) {
                        out.push_str("\x1b[2m");
                    }
                    if modifier.contains(Modifier::ITALIC) {
                        out.push_str("\x1b[3m");
                    }
                    if modifier.contains(Modifier::UNDERLINED) {
                        out.push_str("\x1b[4m");
                    }
                    if modifier.contains(Modifier::SLOW_BLINK)
                        || modifier.contains(Modifier::RAPID_BLINK)
                    {
                        out.push_str("\x1b[5m");
                    }
                    if modifier.contains(Modifier::REVERSED) {
                        out.push_str("\x1b[7m");
                    }
                    if modifier.contains(Modifier::HIDDEN) {
                        out.push_str("\x1b[8m");
                    }
                    if modifier.contains(Modifier::CROSSED_OUT) {
                        out.push_str("\x1b[9m");
                    }

                    if fg != Color::Reset {
                        push_fg_color(&mut out, fg);
                    }
                    if bg != Color::Reset {
                        push_bg_color(&mut out, bg);
                    }

                    prev_fg = fg;
                    prev_bg = bg;
                    prev_modifier = modifier;
                }

                let symbol = cell.symbol();
                let width = UnicodeWidthStr::width(symbol).max(1);
                // Ratatui reserves the cells following a wide grapheme. Never emit them.
                // A standalone zero-width symbol, clipped wide glyph, or terminal control
                // is replaced with a blank rather than changing terminal state.
                if width > usize::from(self.width - x)
                    || UnicodeWidthStr::width(symbol) == 0
                    || symbol.chars().any(char::is_control)
                {
                    out.push(' ');
                    x += 1;
                } else {
                    out.push_str(symbol);
                    x += width as u16;
                }
            }
        }

        out.push_str("\x1b[0m\x1b[?7h");

        // Reposition cursor.
        out.push_str("\x1b[");
        push_u16(
            &mut out,
            self.cursor_y.min(self.height.saturating_sub(1)) + 1,
        );
        out.push(';');
        push_u16(
            &mut out,
            self.cursor_x.min(self.width.saturating_sub(1)) + 1,
        );
        out.push('H');

        if self.cursor_visible {
            out.push_str("\x1b[?25h");
        }

        out
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Append a `u16` to a `String` without allocating an intermediate `String`.
fn push_u16(s: &mut String, n: u16) {
    if n >= 10000 {
        s.push((b'0' + (n / 10000) as u8) as char);
    }
    if n >= 1000 {
        s.push((b'0' + (n / 1000 % 10) as u8) as char);
    }
    if n >= 100 {
        s.push((b'0' + (n / 100 % 10) as u8) as char);
    }
    if n >= 10 {
        s.push((b'0' + (n / 10 % 10) as u8) as char);
    }
    s.push((b'0' + (n % 10) as u8) as char);
}

fn push_fg_color(out: &mut String, color: Color) {
    match color {
        Color::Reset => out.push_str("\x1b[39m"),
        Color::Black => out.push_str("\x1b[30m"),
        Color::Red => out.push_str("\x1b[31m"),
        Color::Green => out.push_str("\x1b[32m"),
        Color::Yellow => out.push_str("\x1b[33m"),
        Color::Blue => out.push_str("\x1b[34m"),
        Color::Magenta => out.push_str("\x1b[35m"),
        Color::Cyan => out.push_str("\x1b[36m"),
        Color::Gray => out.push_str("\x1b[37m"),
        Color::DarkGray => out.push_str("\x1b[90m"),
        Color::LightRed => out.push_str("\x1b[91m"),
        Color::LightGreen => out.push_str("\x1b[92m"),
        Color::LightYellow => out.push_str("\x1b[93m"),
        Color::LightBlue => out.push_str("\x1b[94m"),
        Color::LightMagenta => out.push_str("\x1b[95m"),
        Color::LightCyan => out.push_str("\x1b[96m"),
        Color::White => out.push_str("\x1b[97m"),
        Color::Rgb(r, g, b) => {
            out.push_str("\x1b[38;2;");
            push_u16(out, r as u16);
            out.push(';');
            push_u16(out, g as u16);
            out.push(';');
            push_u16(out, b as u16);
            out.push('m');
        }
        Color::Indexed(n) => {
            out.push_str("\x1b[38;5;");
            push_u16(out, n as u16);
            out.push('m');
        }
    }
}

fn push_bg_color(out: &mut String, color: Color) {
    match color {
        Color::Reset => out.push_str("\x1b[49m"),
        Color::Black => out.push_str("\x1b[40m"),
        Color::Red => out.push_str("\x1b[41m"),
        Color::Green => out.push_str("\x1b[42m"),
        Color::Yellow => out.push_str("\x1b[43m"),
        Color::Blue => out.push_str("\x1b[44m"),
        Color::Magenta => out.push_str("\x1b[45m"),
        Color::Cyan => out.push_str("\x1b[46m"),
        Color::Gray => out.push_str("\x1b[47m"),
        Color::DarkGray => out.push_str("\x1b[100m"),
        Color::LightRed => out.push_str("\x1b[101m"),
        Color::LightGreen => out.push_str("\x1b[102m"),
        Color::LightYellow => out.push_str("\x1b[103m"),
        Color::LightBlue => out.push_str("\x1b[104m"),
        Color::LightMagenta => out.push_str("\x1b[105m"),
        Color::LightCyan => out.push_str("\x1b[106m"),
        Color::White => out.push_str("\x1b[107m"),
        Color::Rgb(r, g, b) => {
            out.push_str("\x1b[48;2;");
            push_u16(out, r as u16);
            out.push(';');
            push_u16(out, g as u16);
            out.push(';');
            push_u16(out, b as u16);
            out.push('m');
        }
        Color::Indexed(n) => {
            out.push_str("\x1b[48;5;");
            push_u16(out, n as u16);
            out.push('m');
        }
    }
}

// ── Backend impl ─────────────────────────────────────────────────────────────

impl Backend for WebBackend {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            if x < self.width && y < self.height {
                let idx = usize::from(y) * usize::from(self.width) + usize::from(x);
                self.cells[idx] = cell.clone();
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(Position::new(self.cursor_x, self.cursor_y))
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        self.cursor_x = position.x;
        self.cursor_y = position.y;
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        for cell in &mut self.cells {
            *cell = Cell::default();
        }
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(Size::new(self.width, self.height))
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: Size {
                width: self.width,
                height: self.height,
            },
            pixels: Size::default(),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.ansi_output = self.render_to_ansi();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{layout::Rect, style::Style, text::Span, widgets::Paragraph, Terminal};

    #[test]
    fn actual_terminal_parser_preserves_wide_cells_transitions_and_bottom_edge() {
        let mut terminal = Terminal::new(WebBackend::new(8, 2)).unwrap();
        let mut parser = vt100::Parser::new(2, 8, 0);
        terminal
            .draw(|f| {
                f.render_widget(Paragraph::new("界e\u{301}abcZ\n12345678"), f.area());
                f.set_cursor_position((7, 1));
            })
            .unwrap();
        parser.process(terminal.backend().get_ansi_output().as_bytes());
        assert_eq!(parser.screen().cell(0, 0).unwrap().contents(), "界");
        assert!(parser.screen().cell(0, 1).unwrap().is_wide_continuation());
        assert_eq!(parser.screen().cell(0, 2).unwrap().contents(), "e\u{301}");
        assert_eq!(parser.screen().cell(1, 7).unwrap().contents(), "8");
        assert_eq!(parser.screen().cursor_position(), (1, 7));
        assert!(!parser.screen().hide_cursor());
        terminal
            .draw(|f| f.render_widget(Paragraph::new("ab界"), f.area()))
            .unwrap();
        parser.process(terminal.backend().get_ansi_output().as_bytes());
        assert_eq!(parser.screen().cell(0, 0).unwrap().contents(), "a");
        assert_eq!(parser.screen().cell(0, 2).unwrap().contents(), "界");
        assert!(parser
            .screen()
            .cell(1, 7)
            .unwrap()
            .contents()
            .trim()
            .is_empty());
        assert!(parser.screen().hide_cursor());
        terminal.backend_mut().resize(4, 1);
        terminal.resize(Rect::new(0, 0, 4, 1)).unwrap();
        parser.set_size(1, 4);
        terminal
            .draw(|f| f.render_widget(Paragraph::new("abc界"), f.area()))
            .unwrap();
        parser.process(terminal.backend().get_ansi_output().as_bytes());
        assert_eq!(parser.screen().contents(), "abc ");
    }

    #[test]
    fn control_symbols_cannot_inject_terminal_commands() {
        let mut backend = WebBackend::new(4, 1);
        let mut cell = Cell::default();
        cell.set_symbol("\x1b]52;c;bad\x07")
            .set_style(Style::default().add_modifier(Modifier::HIDDEN));
        backend.draw(std::iter::once((0, 0, &cell))).unwrap();
        backend.flush().unwrap();
        assert!(!backend.get_ansi_output().contains("]52"));
        assert!(backend.get_ansi_output().contains("\x1b[8m"));
    }

    #[test]
    fn backend_size_matches_constructor() {
        let b = WebBackend::new(80, 24);
        let rect = b.size().unwrap();
        assert_eq!(rect.width, 80);
        assert_eq!(rect.height, 24);
    }

    #[test]
    fn flush_produces_ansi_output() {
        let backend = WebBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let widget = Paragraph::new("hello");
                f.render_widget(widget, f.area());
            })
            .unwrap();
        let ansi = terminal.backend().get_ansi_output();
        assert!(!ansi.is_empty(), "expected non-empty ANSI output");
        assert!(
            ansi.contains("hello"),
            "expected cell content in ANSI output"
        );
    }

    #[test]
    fn resize_updates_dimensions() {
        let mut backend = WebBackend::new(40, 10);
        backend.resize(80, 24);
        let rect = backend.size().unwrap();
        assert_eq!(rect.width, 80);
        assert_eq!(rect.height, 24);
        assert_eq!(
            backend.cells.len(),
            80 * 24,
            "cell buffer length should match new dimensions"
        );
    }

    #[test]
    fn clear_resets_cells() {
        let mut backend = WebBackend::new(10, 5);
        // Manually set a cell.
        backend.cells[0] = {
            let mut c = Cell::default();
            c.set_symbol("X");
            c.clone()
        };
        backend.clear().unwrap();
        for cell in &backend.cells {
            assert_eq!(cell.symbol(), " ", "all cells should be blank after clear");
        }
    }

    #[test]
    fn color_and_style_appear_in_output() {
        let backend = WebBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let widget = Paragraph::new(Span::styled(
                    "styled",
                    Style::default().fg(Color::Red).bg(Color::Blue),
                ));
                f.render_widget(widget, f.area());
            })
            .unwrap();
        let ansi = terminal.backend().get_ansi_output();
        // Red fg = ESC[31m, Blue bg = ESC[44m
        assert!(
            ansi.contains("\x1b[31m"),
            "expected red foreground escape code"
        );
        assert!(
            ansi.contains("\x1b[44m"),
            "expected blue background escape code"
        );
    }
}
