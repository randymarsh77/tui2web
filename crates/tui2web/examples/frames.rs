//! Real backend output consumed by the headless xterm regression test.
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Terminal,
};
use tui2web::WebBackend;

fn main() {
    let mut terminal = Terminal::new(WebBackend::new(10, 3)).unwrap();
    let mut frames = Vec::new();
    terminal
        .draw(|f| {
            f.render_widget(
                Paragraph::new(vec![
                    Line::from("界e\u{301}abc"),
                    Line::from(Span::styled(
                        "hidden",
                        Style::default()
                            .fg(Color::Rgb(1, 2, 3))
                            .bg(Color::Indexed(42))
                            .add_modifier(Modifier::HIDDEN | Modifier::BOLD),
                    )),
                    Line::from("123456789Z"),
                ]),
                f.area(),
            );
            f.set_cursor_position((9, 2));
        })
        .unwrap();
    frames.push(
        serde_json::json!({"columns":10, "rows":3, "ansi":terminal.backend().get_ansi_output()}),
    );
    terminal
        .draw(|f| f.render_widget(Paragraph::new("ab界"), f.area()))
        .unwrap();
    frames.push(
        serde_json::json!({"columns":10, "rows":3, "ansi":terminal.backend().get_ansi_output()}),
    );
    for (width, height, text) in [(4, 1, "abc界"), (12, 4, "grown")] {
        terminal.backend_mut().resize(width, height);
        terminal.resize(Rect::new(0, 0, width, height)).unwrap();
        terminal
            .draw(|f| f.render_widget(Paragraph::new(text), f.area()))
            .unwrap();
        frames.push(serde_json::json!({"columns":width, "rows":height, "ansi":terminal.backend().get_ansi_output()}));
    }
    println!("{}", serde_json::to_string(&frames).unwrap());
}
