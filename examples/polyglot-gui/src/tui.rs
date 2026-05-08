//! Equilibrium Polyglot Calculator — interactive TUI
//!
//! Press ← / → (or h/l) to change n, q to quit.
//! Every keystroke triggers live FFI calls to C, C++, Zig, Nim, V, D, Odin, and Rust.

use crepuscularity_tui::{render_template, TemplateContext, TemplateValue};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
        MouseEventKind,
    },
    execute,
    style::{Color as CrosstermColor, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    DefaultTerminal, Frame,
};
use std::io;

// ── C FFI (always linked) ────────────────────────────────────────────────────
// ── C++ FFI (always linked) ──────────────────────────────────────────────────
// ── Zig FFI (linked when zig was found at build time) ────────────────────────
// ── Nim FFI ──────────────────────────────────────────────────────────────────
// ── V FFI ─────────────────────────────────────────────────────────────────────
// ── D FFI ─────────────────────────────────────────────────────────────────────
// ── Odin FFI ──────────────────────────────────────────────────────────────────
// ── Rust native ───────────────────────────────────────────────────────────────
mod polyglot;

// ── App state ─────────────────────────────────────────────────────────────────
struct App {
    n: i64,
    mode: Mode,
    constellation: polyglot::ConstellationState,
    pipeline_scroll: usize,
    log: Vec<LogEntry>,
}

const TUI_HEADER_ROW_MAX: u16 = 4;
const CONSTELLATION_BUTTON_COL_START: u16 = 1;
const CONSTELLATION_BUTTON_COL_END: u16 = 26;
const PIPELINE_BUTTON_COL_START: u16 = 28;
const PIPELINE_BUTTON_COL_END: u16 = 47;

struct LogEntry {
    lang: &'static str,
    text: String,
    color: &'static str,
}

impl App {
    fn new() -> Self {
        Self {
            n: 7,
            mode: Mode::Constellation,
            constellation: polyglot::ConstellationState::default(),
            pipeline_scroll: 0,
            log: Vec::new(),
        }
    }

    fn push_log(&mut self, lang: &'static str, text: impl Into<String>, color: &'static str) {
        self.log.push(LogEntry {
            lang,
            text: text.into(),
            color,
        });
        if self.log.len() > 80 {
            self.log.drain(0..self.log.len() - 80);
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
    fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Dashboard => Mode::Constellation,
            Mode::Constellation => Mode::Dashboard,
        };
        self.push_log(
            "UI",
            match self.mode {
                Mode::Dashboard => "view pipeline",
                Mode::Constellation => "view constellation",
            },
            "text-zinc-200",
        );
    }
    fn set_dashboard(&mut self) {
        self.mode = Mode::Dashboard;
        self.push_log("UI", "view pipeline", "text-zinc-200");
    }
    fn set_constellation(&mut self) {
        self.mode = Mode::Constellation;
        self.push_log("UI", "view constellation", "text-zinc-200");
    }
    fn click(&mut self, column: u16, row: u16) {
        if row <= TUI_HEADER_ROW_MAX
            && (CONSTELLATION_BUTTON_COL_START..=CONSTELLATION_BUTTON_COL_END).contains(&column)
        {
            self.set_constellation();
        } else if row <= TUI_HEADER_ROW_MAX
            && (PIPELINE_BUTTON_COL_START..=PIPELINE_BUTTON_COL_END).contains(&column)
        {
            self.set_dashboard();
        }
    }
    fn scroll_pipeline(&mut self, amount: isize) {
        self.pipeline_scroll = self.pipeline_scroll.saturating_add_signed(amount).min(7);
    }
    fn burst_center(&mut self) {
        self.constellation.burst(0.5, 0.5, self.n as u32);
        self.push_log("UI", "constellation burst", "text-zinc-200");
    }
    fn advance(&mut self) {
        if self.constellation.advance() {
            self.push_log("Zig", "shooting star", "text-amber-400");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Dashboard,
    Constellation,
}

// ── Rendering ─────────────────────────────────────────────────────────────────

const TEMPLATE: &str = include_str!("../templates/polyglot.crepus");

fn ui(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::default().fg(Color::White).bg(Color::Black));

    render_header(frame, app, area);

    let content_y = area.y.saturating_add(TUI_HEADER_ROW_MAX);
    let content_height = area.height.saturating_sub(TUI_HEADER_ROW_MAX);
    if content_height == 0 {
        return;
    }
    let content_area = Rect {
        x: area.x,
        y: content_y,
        width: area.width,
        height: content_height,
    };

    let snapshot = polyglot::snapshot(app.n);
    let constellation_frame = app.constellation.frame(
        content_area.width.saturating_sub(2).max(20) as usize,
        content_area.height.saturating_sub(2).max(6) as usize,
    );
    let preview_frame = app.constellation.frame(58, 12);
    let _preview_plain_rows = polyglot::constellation_lines(&preview_frame);
    let mut ctx = TemplateContext::new();
    ctx.set("n", snapshot.n);
    ctx.set("mode", app.mode.name());
    ctx.set("is_gui", false);
    ctx.set("is_tui", true);
    ctx.set("is_dashboard", app.mode == Mode::Dashboard);
    ctx.set("is_constellation", app.mode == Mode::Constellation);
    ctx.set("linked_count", snapshot.linked_count);
    ctx.set("missing_count", snapshot.missing_count);
    ctx.set("pipeline_scroll", app.pipeline_scroll as i64);
    ctx.set("tui_rows", TemplateValue::List(result_rows(&snapshot.rows)));
    ctx.set(
        "tui_constellation_rows",
        TemplateValue::List(constellation_rows(&constellation_frame)),
    );
    ctx.set(
        "tui_constellation_preview_rows",
        TemplateValue::List(constellation_rows(&preview_frame)),
    );
    ctx.set("tui_log_rows", TemplateValue::List(log_rows(app)));

    let _ = render_template(TEMPLATE, &ctx, frame, content_area);
    render_constellation_overlays(frame, app, content_area);
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let row = area.y;
    let c_width =
        (CONSTELLATION_BUTTON_COL_END - CONSTELLATION_BUTTON_COL_START + 1).min(area.width);
    let p_width = (PIPELINE_BUTTON_COL_END - PIPELINE_BUTTON_COL_START + 1).min(area.width);
    let c_rect = Rect {
        x: area.x + CONSTELLATION_BUTTON_COL_START.saturating_sub(1),
        y: row,
        width: c_width,
        height: TUI_HEADER_ROW_MAX,
    };
    let p_rect = Rect {
        x: area.x + PIPELINE_BUTTON_COL_START.saturating_sub(1),
        y: row,
        width: p_width,
        height: TUI_HEADER_ROW_MAX,
    };
    let q_x = area
        .x
        .saturating_add(PIPELINE_BUTTON_COL_END.saturating_add(2))
        .min(area.x.saturating_add(area.width.saturating_sub(1)));
    let q_rect = Rect {
        x: q_x,
        y: row + 1,
        width: area.width.saturating_sub(q_x.saturating_sub(area.x)),
        height: 1,
    };

    let c_style = if app.mode == Mode::Constellation {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::Cyan).bg(Color::Black)
    };
    let p_style = if app.mode == Mode::Dashboard {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::Cyan).bg(Color::Black)
    };

    frame.render_widget(
        Paragraph::new("[1] constellation")
            .style(c_style)
            .block(Block::default().borders(Borders::ALL).style(c_style)),
        c_rect,
    );
    frame.render_widget(
        Paragraph::new("[2] pipeline")
            .style(p_style)
            .block(Block::default().borders(Borders::ALL).style(p_style)),
        p_rect,
    );
    frame.render_widget(
        Paragraph::new("[q] quit").style(Style::default().fg(Color::DarkGray)),
        q_rect,
    );
}

fn render_constellation_overlays(frame: &mut Frame, app: &App, area: Rect) {
    if app.mode != Mode::Constellation || area.width < 10 || area.height < 4 {
        return;
    }

    let log_w = 38u16.min(area.width.saturating_sub(8));
    let stars_w = area.width.saturating_sub(log_w + 1);
    if stars_w < 8 {
        return;
    }

    let stars_rect = Rect {
        x: area.x,
        y: area.y,
        width: stars_w,
        height: area.height,
    };
    let log_rect = Rect {
        x: area.x + stars_w + 1,
        y: area.y,
        width: log_w,
        height: area.height,
    };

    let frame_data = app
        .constellation
        .frame(stars_rect.width as usize, stars_rect.height as usize);
    let buf = frame.buffer_mut();
    paint_constellation(buf, stars_rect, &frame_data);
    paint_live_log(buf, log_rect, app);
}

fn paint_constellation(buf: &mut Buffer, area: Rect, frame: &polyglot::ConstellationFrame) {
    for (row_idx, row) in frame.rows.iter().enumerate() {
        if row_idx as u16 >= area.height {
            break;
        }
        let y = area.y + row_idx as u16;
        for (col, (byte_idx, ch)) in row.text.char_indices().enumerate() {
            if col as u16 >= area.width {
                break;
            }
            let x = area.x + col as u16;
            let color = span_color_at(&row.spans, byte_idx).unwrap_or(0x334155);
            let cell = buf.cell_mut((x, y)).expect("cell in bounds");
            cell.set_symbol(&ch.to_string());
            cell.set_style(Style::default().fg(rgb_color(color)));
        }
    }
}

fn paint_live_log(buf: &mut Buffer, area: Rect, app: &App) {
    if area.width < 4 || area.height < 4 {
        return;
    }

    // Title
    let title = "LIVE LOG";
    for (i, ch) in title.chars().enumerate() {
        if i as u16 >= area.width {
            break;
        }
        let cell = buf
            .cell_mut((area.x + i as u16, area.y))
            .expect("cell in bounds");
        cell.set_symbol(&ch.to_string());
        cell.set_style(Style::default().fg(Color::White));
    }

    let mut lines: Vec<(&str, String, u32)> = vec![
        (
            "Rust",
            format!(
                "field={} drift={:.3}",
                app.constellation.stars.len(),
                app.constellation.shooting_star.life
            ),
            0xe879f9,
        ),
        ("C", "bright nebula dust".to_string(), 0x10b981),
        ("C++", "deep shadow dust".to_string(), 0x38bdf8),
        ("Zig", "shooting star trail".to_string(), 0xf59e0b),
        ("Nim", "cyan shimmer glyphs".to_string(), 0x22d3ee),
        ("V", "mint shimmer glyphs".to_string(), 0x4ade80),
        ("D", "even burst rings".to_string(), 0x60a5fa),
        ("Odin", "odd burst rings".to_string(), 0xfb7185),
    ];
    let start = app.log.len().saturating_sub(7);
    lines.extend(
        app.log
            .iter()
            .skip(start)
            .map(|entry| (entry.lang, entry.text.clone(), class_color(entry.color))),
    );

    let mut y = area.y.saturating_add(2);
    for (lang, text, color) in lines {
        if y >= area.y + area.height {
            break;
        }
        draw_text(buf, area.x, y, area.width.min(7), lang, rgb_color(color));
        if area.width > 8 {
            draw_text(
                buf,
                area.x + 8,
                y,
                area.width - 8,
                &text,
                Color::Rgb(190, 190, 190),
            );
        }
        y += 1;
    }
}

fn draw_text(buf: &mut Buffer, x: u16, y: u16, max_width: u16, text: &str, color: Color) {
    for (i, ch) in text.chars().enumerate() {
        if i as u16 >= max_width {
            break;
        }
        let cell = buf.cell_mut((x + i as u16, y)).expect("cell in bounds");
        cell.set_symbol(&ch.to_string());
        cell.set_style(Style::default().fg(color));
    }
}

fn span_color_at(spans: &[polyglot::ConstellationSpan], byte_idx: usize) -> Option<u32> {
    spans
        .iter()
        .find(|span| span.start <= byte_idx && byte_idx < span.end)
        .map(|span| span.color)
}

fn rgb_color(color: u32) -> Color {
    Color::Rgb(
        ((color >> 16) & 0xff) as u8,
        ((color >> 8) & 0xff) as u8,
        (color & 0xff) as u8,
    )
}

fn class_color(class: &str) -> u32 {
    match class {
        "text-emerald-400" => 0x10b981,
        "text-sky-400" => 0x38bdf8,
        "text-amber-400" => 0xf59e0b,
        "text-cyan-400" => 0x22d3ee,
        "text-green-400" => 0x4ade80,
        "text-blue-400" => 0x60a5fa,
        "text-rose-400" => 0xfb7185,
        "text-fuchsia-400" => 0xe879f9,
        _ => 0xe5e7eb,
    }
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

fn constellation_rows(frame: &polyglot::ConstellationFrame) -> Vec<TemplateContext> {
    frame
        .rows
        .iter()
        .map(|row| {
            let mut ctx = TemplateContext::new();
            ctx.set("line", row.text.clone());
            ctx.set("color", row_color_class(row));
            ctx
        })
        .collect()
}

fn row_color_class(row: &polyglot::ConstellationRow) -> String {
    debug_assert!(row
        .spans
        .iter()
        .all(|span| span.start <= span.end && span.end <= row.text.len()));
    let color = row
        .spans
        .iter()
        .find(|span| span.color != 0x334155)
        .or_else(|| row.spans.first())
        .map_or(0x22d3ee, |span| span.color);
    color_class(color).to_string()
}

fn color_class(color: u32) -> &'static str {
    match color {
        0x10b981 => "text-emerald-400",
        0x38bdf8 => "text-sky-400",
        0xf59e0b => "text-amber-400",
        0x22d3ee => "text-cyan-400",
        0x4ade80 => "text-green-400",
        0x60a5fa => "text-blue-400",
        0xfb7185 => "text-rose-400",
        0xe879f9 => "text-fuchsia-400",
        _ => "text-cyan-400",
    }
}

fn log_rows(app: &App) -> Vec<TemplateContext> {
    let mut rows = vec![
        (
            "Rust",
            format!(
                "field={} drift={:.3}",
                app.constellation.stars.len(),
                app.constellation.shooting_star.life
            ),
            "text-fuchsia-400",
        ),
        ("C", "bright nebula dust".to_string(), "text-emerald-400"),
        ("C++", "deep shadow dust".to_string(), "text-sky-400"),
        ("Zig", "shooting star trail".to_string(), "text-amber-400"),
        ("Nim", "cyan shimmer glyphs".to_string(), "text-cyan-400"),
        ("V", "mint shimmer glyphs".to_string(), "text-green-400"),
        ("D", "even burst rings".to_string(), "text-blue-400"),
        ("Odin", "odd burst rings".to_string(), "text-rose-400"),
    ];
    let start = app.log.len().saturating_sub(7);
    rows.extend(
        app.log
            .iter()
            .skip(start)
            .map(|entry| (entry.lang, entry.text.clone(), entry.color)),
    );

    rows.into_iter()
        .map(|(lang, text, color)| {
            let mut ctx = TemplateContext::new();
            ctx.set("lang", lang);
            ctx.set("text", text);
            ctx.set("color", color);
            ctx
        })
        .collect()
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Dashboard => "dashboard",
            Mode::Constellation => "constellation",
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────
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
                        KeyCode::Enter | KeyCode::Char(' ') => app.burst_center(),
                        KeyCode::Up | KeyCode::Char('k') => app.scroll_pipeline(-1),
                        KeyCode::Down | KeyCode::Char('j') => app.scroll_pipeline(1),
                        KeyCode::Tab | KeyCode::Char('m') => app.toggle_mode(),
                        KeyCode::Char('1') => app.set_constellation(),
                        KeyCode::Char('2') => app.set_dashboard(),
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                        app.click(mouse.column, mouse.row);
                    }
                }
                _ => {}
            }
        }
        app.advance();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn renders_dashboard_from_shared_crepus_layout() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();
        app.set_dashboard();

        terminal.draw(|frame| ui(frame, &app)).unwrap();

        let buffer = terminal.backend().buffer();
        assert!(buffer.content.iter().all(|cell| cell.style().bg.is_some()));

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
        for result in ["Pipeline", "Math Display"] {
            assert!(
                text.contains(result),
                "missing shared layout text: {result}"
            );
        }
    }

    #[test]
    fn constellation_uses_shared_crepus_layout_and_row_colors() {
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
        let colors = buffer
            .content
            .iter()
            .filter_map(|cell| cell.style().fg)
            .collect::<std::collections::HashSet<_>>();

        assert!(text.contains("LIVE LOG"));
        assert!(text.contains("Zig"));
        assert!(colors.len() >= 6);
    }

    #[test]
    fn mode_buttons_handle_mouse_hits() {
        let mut app = App::new();
        app.set_dashboard();

        app.click(4, 1);
        assert_eq!(app.mode, Mode::Constellation);

        app.click(30, 1);
        assert_eq!(app.mode, Mode::Dashboard);
    }
}
