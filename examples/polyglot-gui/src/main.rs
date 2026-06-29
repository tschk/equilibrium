use crepuscularity_gpui::prelude::*;
use gpui::{
    div, px, rgb, size, App, Application, Bounds, Div, MouseButton, MouseDownEvent, Window,
    WindowBounds, WindowOptions,
};

include!(concat!(env!("OUT_DIR"), "/polyglot_gui_template.rs"));

mod polyglot;

struct CrepusShellParts {
    pipeline_controls: Div,
}

struct PolyglotTemplate<R> {
    parts: CrepusShellParts,
    n: i64,
    is_gui: bool,
    is_tui: bool,
    linked_count: i64,
    missing_count: i64,
    gui_rows: R,
    pipeline_scroll: i64,
}

struct Dashboard {
    n: i64,
}

impl Dashboard {
    fn new(_cx: &mut Context<Self>) -> Self {
        Self { n: 7 }
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
}

impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = polyglot::snapshot(self.n);
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

        render_crepus_shell(PolyglotTemplate {
            parts: CrepusShellParts {
                pipeline_controls: control_strip(cx),
            },
            n: snapshot.n,
            is_gui: true,
            is_tui: false,
            linked_count: snapshot.linked_count,
            missing_count: snapshot.missing_count,
            gui_rows: rows,
            pipeline_scroll: 0,
        })
    }
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