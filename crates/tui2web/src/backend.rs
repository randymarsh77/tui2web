use ratatui::{
    backend::{Backend, WindowSize},
    buffer::Cell,
    layout::{Rect, Size},
    style::{Color, Modifier},
};
use std::io;

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
        out.push_str("\x1b[?25l");

        let mut prev_fg = Color::Reset;
        let mut prev_bg = Color::Reset;
        let mut prev_modifier = Modifier::empty();

        for y in 0..self.height {
            // Move cursor to start of row (1-based ANSI coordinates).
            out.push_str("\x1b[");
            push_u16(&mut out, y + 1);
            out.push_str(";1H");

            for x in 0..self.width {
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

                out.push_str(cell.symbol());
            }
        }

        out.push_str("\x1b[0m");

        // Reposition cursor.
        out.push_str("\x1b[");
        push_u16(&mut out, self.cursor_y + 1);
        out.push(';');
        push_u16(&mut out, self.cursor_x + 1);
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

    fn get_cursor(&mut self) -> io::Result<(u16, u16)> {
        Ok((self.cursor_x, self.cursor_y))
    }

    fn set_cursor(&mut self, x: u16, y: u16) -> io::Result<()> {
        self.cursor_x = x;
        self.cursor_y = y;
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        for cell in &mut self.cells {
            *cell = Cell::default();
        }
        Ok(())
    }

    fn size(&self) -> io::Result<Rect> {
        Ok(Rect::new(0, 0, self.width, self.height))
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
    use ratatui::{
        style::Style,
        text::Span,
        widgets::Paragraph,
        Terminal,
    };

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
                f.render_widget(widget, f.size());
            })
            .unwrap();
        let ansi = terminal.backend().get_ansi_output();
        assert!(!ansi.is_empty(), "expected non-empty ANSI output");
        assert!(ansi.contains("hello"), "expected cell content in ANSI output");
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
                f.render_widget(widget, f.size());
            })
            .unwrap();
        let ansi = terminal.backend().get_ansi_output();
        // Red fg = ESC[31m, Blue bg = ESC[44m
        assert!(ansi.contains("\x1b[31m"), "expected red foreground escape code");
        assert!(ansi.contains("\x1b[44m"), "expected blue background escape code");
    }
}
