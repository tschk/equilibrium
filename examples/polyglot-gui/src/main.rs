use std::time::Duration;

use crepuscularity_gpui::prelude::*;
use gpui::{
    div, px, rgb, size, AnyElement, App, Application, AsyncApp, Bounds, Div, FontWeight,
    HighlightStyle, MouseButton, MouseDownEvent, StyledText, Task, Timer, WeakEntity, Window,
    WindowBounds, WindowOptions,
};

include!(concat!(env!("OUT_DIR"), "/polyglot_gui_template.rs"));

mod polyglot;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Constellation,
    Dashboard,
}

impl Mode {
    fn template_name(self) -> &'static str {
        match self {
            Mode::Constellation => "constellation",
            Mode::Dashboard => "dashboard",
        }
    }
}

struct CrepusShellParts {
    mode_switcher: Div,
    pipeline_controls: Div,
    constellation_canvas: AnyElement,
    log_panel: AnyElement,
}

struct PolyglotTemplate<R, P> {
    parts: CrepusShellParts,
    n: i64,
    mode: &'static str,
    is_gui: bool,
    is_tui: bool,
    is_dashboard: bool,
    is_constellation: bool,
    linked_count: i64,
    missing_count: i64,
    gui_rows: R,
    gui_constellation_preview_rows: P,
    pipeline_scroll: i64,
}

struct LogEntry {
    lang: &'static str,
    text: String,
    color: u32,
}

struct Dashboard {
    _animation: Task<()>,
    n: i64,
    mode: Mode,
    constellation: polyglot::ConstellationState,
    log: Vec<LogEntry>,
}

impl Dashboard {
    fn new(cx: &mut Context<Self>) -> Self {
        let animation = cx.spawn(
            async move |this: WeakEntity<Dashboard>, cx: &mut AsyncApp| loop {
                Timer::after(Duration::from_millis(50)).await;
                if this
                    .update(cx, |this, cx| {
                        this.advance(cx);
                    })
                    .is_err()
                {
                    break;
                }
            },
        );

        Self {
            _animation: animation,
            n: 7,
            mode: Mode::Constellation,
            constellation: polyglot::ConstellationState::default(),
            log: Vec::new(),
        }
    }

    fn push_log(&mut self, lang: &'static str, text: impl Into<String>, color: u32) {
        self.log.push(LogEntry {
            lang,
            text: text.into(),
            color,
        });
        if self.log.len() > 80 {
            self.log.drain(0..self.log.len() - 80);
        }
    }

    fn burst(&mut self, x: f32, y: f32, seed: u32, cx: &mut Context<Self>) {
        self.constellation.burst(x, y, seed);
        self.push_log("UI", "constellation burst", 0xf8fafc);
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.push_log(
            "UI",
            match mode {
                Mode::Constellation => "view constellation",
                Mode::Dashboard => "view pipeline",
            },
            0xf8fafc,
        );
        cx.notify();
    }

    fn shift(&mut self, amount: i64, cx: &mut Context<Self>) {
        self.n = (self.n + amount).clamp(0, 1_000_000);
        cx.notify();
    }

    fn dec_1(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift(-1, cx);
    }
    fn inc_1(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift(1, cx);
    }
    fn double(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.n = (self.n * 2).clamp(0, 1_000_000);
        cx.notify();
    }
    fn halve(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.n = (self.n / 2).clamp(0, 1_000_000);
        cx.notify();
    }
    fn reset(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.n = 7;
        cx.notify();
    }

    fn advance(&mut self, cx: &mut Context<Self>) {
        if self.constellation.advance() {
            self.push_log("Zig", "shooting star", 0xf59e0b);
        }
        cx.notify();
    }

    fn on_ascii_mouse_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let bounds = window.bounds();
        let x = ((ev.position.x.to_f64() - bounds.origin.x.to_f64()) / bounds.size.width.to_f64())
            .clamp(0.0, 1.0) as f32;
        let y = ((ev.position.y.to_f64() - bounds.origin.y.to_f64()) / bounds.size.height.to_f64())
            .clamp(0.0, 1.0) as f32;
        self.burst(x, y, ev.click_count as u32, cx);
    }
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let n = self.n;
        let snapshot = polyglot::snapshot(n);
        let mode_switcher = mode_switcher(self.mode, cx);
        let pipeline_controls = control_strip(cx);
        let bounds = window.bounds();
        let width = bounds.size.width.to_f64() as f32;
        let height = bounds.size.height.to_f64() as f32;
        let constellation_canvas = ascii_constellation(width, height, &self.constellation, cx);
        let preview_frame = self.constellation.frame(58, 12);
        let preview_constellation_rows = polyglot::constellation_lines(&preview_frame);
        let log_panel = constellation_log_panel(&self.constellation, &self.log).into_any_element();
        let rows = snapshot
            .rows
            .iter()
            .map(|row| {
                (
                    row.lang.to_string(),
                    row.linked,
                    row.result.clone(),
                    if row.linked { "LINKED" } else { "MISSING" },
                    row.accent,
                )
            })
            .collect::<Vec<_>>()
            .into_iter();
        let preview_rows = preview_constellation_rows.into_iter();

        render_crepus_shell(PolyglotTemplate {
            parts: CrepusShellParts {
                mode_switcher,
                pipeline_controls,
                constellation_canvas: constellation_canvas.into_any_element(),
                log_panel,
            },
            n: snapshot.n,
            mode: self.mode.template_name(),
            is_gui: true,
            is_tui: false,
            is_dashboard: self.mode == Mode::Dashboard,
            is_constellation: self.mode == Mode::Constellation,
            linked_count: snapshot.linked_count,
            missing_count: snapshot.missing_count,
            gui_rows: rows,
            gui_constellation_preview_rows: preview_rows,
            pipeline_scroll: 0,
        })
    }
}

fn ascii_constellation(
    width: f32,
    height: f32,
    constellation: &polyglot::ConstellationState,
    cx: &mut Context<Dashboard>,
) -> Div {
    let layer = div()
        .w_full()
        .h_full()
        .overflow_hidden()
        .bg(rgb(0x050507))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(Dashboard::on_ascii_mouse_down),
        );

    let row_height = 8.0;
    let rows = ((height * 1.45 / row_height).ceil() as usize).clamp(64, 360);
    let cols = ((width * 1.75 / 3.7).ceil() as usize).clamp(180, 920);
    let frame = constellation.frame(cols, rows);
    let mut rows_view = div()
        .w_full()
        .h_full()
        .overflow_hidden()
        .font_family("monospace")
        .text_size(px(row_height))
        .flex()
        .flex_col();

    for row in frame.rows {
        rows_view = rows_view.child(ascii_row(row, row_height));
    }

    layer.child(rows_view)
}

fn ascii_row(row: polyglot::ConstellationRow, row_height: f32) -> Div {
    let highlights = row
        .spans
        .into_iter()
        .map(|span| (span.start..span.end, rgb(span.color).into()))
        .collect::<Vec<(std::ops::Range<usize>, HighlightStyle)>>();

    div()
        .w_full()
        .h(px(row_height))
        .text_size(px(row_height))
        .line_height(px(row_height))
        .whitespace_nowrap()
        .child(StyledText::new(row.text).with_highlights(highlights))
}

fn constellation_log_panel(
    constellation: &polyglot::ConstellationState,
    log: &[LogEntry],
) -> impl IntoElement {
    let mut panel = div()
        .id("constellation-log")
        .w(px(340.0))
        .h_full()
        .overflow_y_scroll()
        .scrollbar_width(px(8.0))
        .rounded_lg()
        .border_1()
        .border_color(rgb(0x1f2937))
        .bg(rgb(0x07080c))
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(0x94a3b8))
                .child("LIVE LOG"),
        )
        .child(status_line(
            "Rust",
            format!(
                "field={} drift={:.3}",
                constellation.stars.len(),
                constellation.shooting_star.life
            ),
            0xe879f9,
        ))
        .child(status_line("C", "bright nebula dust".to_string(), 0x10b981))
        .child(status_line("C++", "deep shadow dust".to_string(), 0x38bdf8))
        .child(status_line(
            "Zig",
            "shooting star trail".to_string(),
            0xf59e0b,
        ))
        .child(status_line(
            "Nim",
            "cyan shimmer glyphs".to_string(),
            0x22d3ee,
        ))
        .child(status_line(
            "V",
            "mint shimmer glyphs".to_string(),
            0x4ade80,
        ))
        .child(status_line("D", "even burst rings".to_string(), 0x60a5fa))
        .child(status_line("Odin", "odd burst rings".to_string(), 0xfb7185));

    let start = log.len().saturating_sub(7);
    for line in log.iter().skip(start) {
        panel = panel.child(status_line(line.lang, line.text.clone(), line.color));
    }

    panel
}

fn status_line(lang: &str, text: String, color: u32) -> Div {
    div()
        .text_sm()
        .text_color(rgb(color))
        .child(format!("{lang}: {text}"))
}

fn mode_switcher(mode: Mode, cx: &mut Context<Dashboard>) -> Div {
    let constellation = pill(
        "Constellation",
        cx.listener(|this, _, _, cx| this.set_mode(Mode::Constellation, cx)),
    )
    .when(mode == Mode::Constellation, |cx| {
        cx.bg(rgb(0x1d4ed8)).text_color(rgb(0xffffff))
    });
    let pipeline = pill(
        "Pipeline",
        cx.listener(|this, _, _, cx| this.set_mode(Mode::Dashboard, cx)),
    )
    .when(mode == Mode::Dashboard, |cx| {
        cx.bg(rgb(0x7c3aed)).text_color(rgb(0xffffff))
    });

    div()
        .absolute()
        .left(px(28.0))
        .top(px(24.0))
        .flex()
        .gap_2()
        .child(constellation)
        .child(pipeline)
}

fn control_strip(cx: &mut Context<Dashboard>) -> Div {
    div()
        .w_full()
        .flex_wrap()
        .flex()
        .gap_1()
        .items_start()
        .justify_start()
        .child(control_pill("-1", cx.listener(Dashboard::dec_1)))
        .child(control_pill("+1", cx.listener(Dashboard::inc_1)))
        .child(control_pill("x2", cx.listener(Dashboard::double)))
        .child(control_pill("/2", cx.listener(Dashboard::halve)))
        .child(control_pill("reset", cx.listener(Dashboard::reset)))
}

fn pill(label: &str, on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_full()
        .bg(rgb(0x18181b))
        .border_1()
        .border_color(rgb(0x27272a))
        .cursor_pointer()
        .child(label.to_string())
        .on_mouse_down(MouseButton::Left, on_click)
}

fn control_pill(
    label: &str,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded_full()
        .bg(rgb(0x18181b))
        .border_1()
        .border_color(rgb(0x27272a))
        .text_xs()
        .cursor_pointer()
        .child(label.to_string())
        .on_mouse_down(MouseButton::Left, on_click)
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1440.0), px(900.0)), cx);
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(Dashboard::new),
        );
        cx.activate(true);
    });
}
