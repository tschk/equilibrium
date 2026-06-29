//! Equilibrium Polyglot Calculator — interactive TUI
//!
//! Press ← / → (or h/l) to change n, q to quit.

use crepuscularity_tui::{render_template, TemplateContext, TemplateValue};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    style::{Color as CrosstermColor, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use ratatui::{style::{Color, Style}, DefaultTerminal, Frame};
use std::io;

mod polyglot;

struct App {
    n: i64,
    pipeline_scroll: usize,
}

impl App {
    fn new() -> Self {
        Self {
            n: 7,
            pipeline_scroll: 0,
        }
    }

    fn increment(&mut self) {
        self.n += 1;
    }
    fn decrement(&mut self) {
        self.n = (self.n - 1).max(0);
    }
    fn double(&mut self) {
        self.n = (self.n * 2).min(9_999_999);
    }
    fn halve(&mut self) {
        self.n = (self.n / 2).max(0);
    }
    fn reset(&mut self) {
        self.n = 7;
    }
    fn scroll_pipeline(&mut self, amount: isize) {
        self.pipeline_scroll = self.pipeline_scroll.saturating_add_signed(amount).min(7);
    }
}

const TEMPLATE: &str = include_str!("../templates/polyglot.crepus");

fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::default().fg(Color::White).bg(Color::Black));

    let snapshot = polyglot::snapshot(app.n);
    let mut ctx = TemplateContext::new();
    ctx.set("n", snapshot.n);
    ctx.set("is_gui", false);
    ctx.set("is_tui", true);
    ctx.set("linked_count", snapshot.linked_count);
    ctx.set("missing_count", snapshot.missing_count);
    ctx.set("pipeline_scroll", app.pipeline_scroll as i64);
    ctx.set("tui_rows", TemplateValue::List(result_rows(&snapshot.rows)));

    let _ = render_template(TEMPLATE, &ctx, frame, area);
}

fn result_rows(rows: &[polyglot::ResultRow]) -> Vec<TemplateContext> {
    rows.iter().map(result_row).collect()
}

fn result_row(row: &polyglot::ResultRow) -> TemplateContext {
    let mut ctx = TemplateContext::new();
    ctx.set("lang", row.lang);
    ctx.set("linked", row.linked);
    ctx.set("result", row.result.clone());
    ctx.set("status", if row.linked { "LINKED" } else { "MISSING" });
    ctx.set("accent", row.accent);
    ctx
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    execute!(
        io::stdout(),
        EnableMouseCapture,
        SetBackgroundColor(CrosstermColor::Black),
        SetForegroundColor(CrosstermColor::White),
        Clear(ClearType::All)
    )?;
    terminal.clear()?;
    let result = run(terminal);
    ratatui::restore();
    execute!(io::stdout(), DisableMouseCapture, ResetColor)?;
    result
}

fn run(mut terminal: DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();

    loop {
        terminal.draw(|frame| ui(frame, &app))?;

        if event::poll(std::time::Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Right | KeyCode::Char('l') => app.increment(),
                        KeyCode::Left | KeyCode::Char('h') => app.decrement(),
                        KeyCode::Char('d') => app.double(),
                        KeyCode::Char('s') => app.halve(),
                        KeyCode::Char('r') => app.reset(),
                        KeyCode::Up | KeyCode::Char('k') => app.scroll_pipeline(-1),
                        KeyCode::Down | KeyCode::Char('j') => app.scroll_pipeline(1),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_pipeline_from_shared_crepus_layout() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let app = App::new();

        terminal.draw(|frame| ui(frame, &app)).unwrap();

        let buffer = terminal.backend().buffer();
        let width = buffer.area.width as usize;
        let text = buffer
            .content
            .chunks(width)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");

        for lang in ["C", "C++", "Zig", "Nim", "V", "D", "Odin", "Rust"] {
            assert!(text.contains(lang), "missing language row: {lang}");
        }
        assert!(text.contains("Pipeline"));
    }
}