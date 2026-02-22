//! Example ratatui application compiled to WebAssembly via tui2web.
//!
//! Demonstrates a simple counter TUI with keyboard navigation.
//! Build with: `wasm-pack build --target web --out-dir ../web/pkg`

use std::collections::VecDeque;

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Terminal,
};
use tui2web::WebBackend;
use wasm_bindgen::prelude::*;

/// The WebAssembly-exported application struct.
///
/// JavaScript usage:
/// ```js
/// import init, { App } from './pkg/tui2web_example.js';
/// await init();
/// const app = new App(80, 24);
/// app.tick();                 // initial render
/// term.write(app.get_frame()); // write to xterm.js
///
/// term.onKey(({ domEvent }) => {
///     app.push_key(domEvent.key);
///     app.tick();
///     term.write(app.get_frame());
/// });
/// ```
#[wasm_bindgen]
pub struct App {
    terminal: Terminal<WebBackend>,
    key_queue: VecDeque<String>,
    counter: i32,
    max_value: i32,
    should_quit: bool,
    status_message: String,
}

#[wasm_bindgen]
impl App {
    /// Create a new application with the given terminal dimensions (columns × rows).
    #[wasm_bindgen(constructor)]
    pub fn new(width: u16, height: u16) -> App {
        // Redirect Rust panics to the browser console.
        console_error_panic_hook::set_once();

        let backend = WebBackend::new(width, height);
        let terminal = Terminal::new(backend).unwrap();

        App {
            terminal,
            key_queue: VecDeque::new(),
            counter: 0,
            max_value: 100,
            should_quit: false,
            status_message: String::from(
                "Press j/↓ to increment · k/↑ to decrement · r to reset · q to quit",
            ),
        }
    }

    /// Enqueue a keyboard event from JavaScript.
    ///
    /// Pass the value of `KeyboardEvent.key` (e.g. `"j"`, `"ArrowUp"`, `"Escape"`).
    pub fn push_key(&mut self, key: String) {
        self.key_queue.push_back(key);
    }

    /// Process all pending key events, re-render the frame, and return `true`
    /// while the application is still running.
    pub fn tick(&mut self) -> bool {
        while let Some(key) = self.key_queue.pop_front() {
            self.handle_input(&key);
        }

        if !self.should_quit {
            self.render();
        }

        !self.should_quit
    }

    /// Return the latest ANSI-encoded terminal frame as a JavaScript string.
    ///
    /// Call this after [`tick`] and write the result to xterm.js:
    /// ```js
    /// term.write(app.get_frame());
    /// ```
    pub fn get_frame(&self) -> String {
        self.terminal.backend().get_ansi_output().to_string()
    }

    /// Notify the application that the terminal has been resized.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.terminal.backend_mut().resize(width, height);
        let _ = self.terminal
            .resize(ratatui::layout::Rect::new(0, 0, width, height));
    }

    /// Return `true` when the user has requested to quit.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

impl App {
    fn handle_input(&mut self, key: &str) {
        match key {
            "q" | "Escape" => {
                self.should_quit = true;
            }
            "j" | "ArrowDown" => {
                if self.counter < self.max_value {
                    self.counter += 1;
                }
                self.status_message =
                    format!("Counter: {}/{}", self.counter, self.max_value);
            }
            "k" | "ArrowUp" => {
                if self.counter > 0 {
                    self.counter -= 1;
                }
                self.status_message =
                    format!("Counter: {}/{}", self.counter, self.max_value);
            }
            "r" => {
                self.counter = 0;
                self.status_message = String::from("Counter reset to 0");
            }
            _ => {}
        }
    }

    fn render(&mut self) {
        let counter = self.counter;
        let max_value = self.max_value;
        let status = self.status_message.clone();

        self.terminal
            .draw(|frame| {
                let area = frame.size();

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // title
                        Constraint::Length(3), // gauge
                        Constraint::Min(0),    // content / key bindings
                        Constraint::Length(3), // status bar
                    ])
                    .split(area);

                // ── Title ────────────────────────────────────────────────────
                let title = Paragraph::new("tui2web — Interactive TUI demo in the browser")
                    .block(Block::default().borders(Borders::ALL).title(" tui2web "))
                    .style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    );
                frame.render_widget(title, chunks[0]);

                // ── Progress gauge ───────────────────────────────────────────
                let percent = if max_value > 0 {
                    (counter * 100 / max_value).clamp(0, 100) as u16
                } else {
                    0
                };
                let gauge = Gauge::default()
                    .block(Block::default().borders(Borders::ALL).title(" Progress "))
                    .gauge_style(
                        Style::default()
                            .fg(Color::Green)
                            .bg(Color::Black),
                    )
                    .percent(percent)
                    .label(format!("{}/{}", counter, max_value));
                frame.render_widget(gauge, chunks[1]);

                // ── Counter and key bindings ─────────────────────────────────
                let lines = vec![
                    Line::from(vec![
                        Span::raw("  Current value: "),
                        Span::styled(
                            counter.to_string(),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled(
                            "  j / ↓",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  Increment"),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            "  k / ↑",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  Decrement"),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            "  r    ",
                            Style::default()
                                .fg(Color::Blue)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  Reset to 0"),
                    ]),
                    Line::from(vec![
                        Span::styled(
                            "  q    ",
                            Style::default()
                                .fg(Color::Red)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw("  Quit"),
                    ]),
                ];
                let content = Paragraph::new(lines)
                    .block(Block::default().borders(Borders::ALL).title(" Counter "));
                frame.render_widget(content, chunks[2]);

                // ── Status bar ───────────────────────────────────────────────
                let status_widget = Paragraph::new(Span::styled(
                    format!(" {}", status),
                    Style::default().fg(Color::Gray),
                ))
                .block(Block::default().borders(Borders::ALL));
                frame.render_widget(status_widget, chunks[3]);
            })
            .unwrap();
    }
}
