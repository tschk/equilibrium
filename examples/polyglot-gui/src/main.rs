use std::time::Duration;

use crepuscularity_gpui::prelude::*;
use gpui::{
    div, px, rgb, size, AnyElement, App, Application, AsyncApp, Bounds, Div, FontWeight,
    HighlightStyle, MouseButton, MouseDownEvent, StyledText, Task, Timer, WeakEntity, Window,
    WindowBounds, WindowOptions,
};
use rayon::prelude::*;

include!(concat!(env!("OUT_DIR"), "/polyglot_gui_template.rs"));

#[cfg(has_c)]
mod c_ffi {
    include!(concat!(env!("OUT_DIR"), "/c_bindings.rs"));
}
#[cfg(has_cpp)]
mod cpp_ffi {
    include!(concat!(env!("OUT_DIR"), "/cpp_bindings.rs"));
}

#[cfg(has_zig)]
extern "C" {
    fn zig_square(n: i64) -> i64;
    fn zig_spiral_sum(n: i64) -> i64;
    fn zig_chaos_fold(n: i64) -> i64;
}

#[cfg(not(has_zig))]
unsafe fn zig_square(_: i64) -> i64 {
    0
}
#[cfg(not(has_zig))]
unsafe fn zig_spiral_sum(_: i64) -> i64 {
    0
}
#[cfg(not(has_zig))]
unsafe fn zig_chaos_fold(_: i64) -> i64 {
    0
}

#[cfg(has_nim)]
extern "C" {
    fn nim_popcount(n: u32) -> i32;
    fn nim_reverse_bits(n: u32) -> u32;
    fn nim_rotate_left(n: u32, shift: u32) -> u32;
}

#[cfg(not(has_nim))]
unsafe fn nim_popcount(_: u32) -> i32 {
    0
}
#[cfg(not(has_nim))]
unsafe fn nim_reverse_bits(n: u32) -> u32 {
    n.reverse_bits()
}
#[cfg(not(has_nim))]
unsafe fn nim_rotate_left(n: u32, shift: u32) -> u32 {
    n.rotate_left(shift)
}

#[cfg(has_v)]
extern "C" {
    fn v_celsius_to_fahrenheit(c: f64) -> f64;
    fn v_km_to_miles(km: f64) -> f64;
    fn v_kelvin_to_rankine(k: f64) -> f64;
}

#[cfg(not(has_v))]
unsafe fn v_celsius_to_fahrenheit(c: f64) -> f64 {
    c.mul_add(9.0 / 5.0, 32.0)
}
#[cfg(not(has_v))]
unsafe fn v_km_to_miles(km: f64) -> f64 {
    km * 0.621_371
}
#[cfg(not(has_v))]
unsafe fn v_kelvin_to_rankine(k: f64) -> f64 {
    k * 9.0 / 5.0
}

#[cfg(has_d)]
extern "C" {
    fn d_abs(n: i32) -> i32;
    fn d_triangular(n: i32) -> i64;
    fn d_clamp(n: i32, lo: i32, hi: i32) -> i32;
    fn d_collatz_steps(n: i32) -> i32;
}

#[cfg(not(has_d))]
unsafe fn d_abs(_: i32) -> i32 {
    0
}
#[cfg(not(has_d))]
unsafe fn d_triangular(_: i32) -> i64 {
    0
}
#[cfg(not(has_d))]
unsafe fn d_clamp(n: i32, _: i32, _: i32) -> i32 {
    n
}
#[cfg(not(has_d))]
unsafe fn d_collatz_steps(_: i32) -> i32 {
    0
}

#[cfg(has_odin)]
extern "C" {
    fn odin_abs(n: i32) -> i32;
    fn odin_min(a: i32, b: i32) -> i32;
    fn odin_max(a: i32, b: i32) -> i32;
    fn odin_mix(a: i32, b: i32) -> i32;
    fn odin_clamp(n: i32, lo: i32, hi: i32) -> i32;
}

#[cfg(not(has_odin))]
unsafe fn odin_abs(_: i32) -> i32 {
    0
}
#[cfg(not(has_odin))]
unsafe fn odin_min(a: i32, b: i32) -> i32 {
    a.min(b)
}
#[cfg(not(has_odin))]
unsafe fn odin_max(a: i32, b: i32) -> i32 {
    a.max(b)
}
#[cfg(not(has_odin))]
unsafe fn odin_mix(a: i32, b: i32) -> i32 {
    (a * 31) ^ (b * 17)
}
#[cfg(not(has_odin))]
unsafe fn odin_clamp(n: i32, lo: i32, hi: i32) -> i32 {
    n.clamp(lo, hi)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Constellation,
    Pipeline,
}

struct Star {
    x: f32,
    y: f32,
    z: f32,
    phase: f32,
}

struct ShootingStar {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
}

struct Burst {
    x: f32,
    y: f32,
    age: f32,
    seed: u32,
}

#[derive(Clone, Copy)]
struct AsciiCell {
    ch: char,
    color: u32,
}

struct AsciiRow {
    text: String,
    highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
}

struct LogEntry {
    lang: &'static str,
    text: String,
    color: u32,
}

struct CrepusShellParts {
    background: AnyElement,
    mode_switcher: Div,
    show_log: bool,
    log_panel: AnyElement,
    show_pipeline_header: bool,
    pipeline_title: Div,
    pipeline_value: Div,
    pipeline_controls: Div,
    mode_content: AnyElement,
}

struct Dashboard {
    _animation: Task<()>,
    n: i64,
    mode: Mode,
    tick: f32,
    stars: Vec<Star>,
    shooting_star: ShootingStar,
    bursts: Vec<Burst>,
    log: Vec<LogEntry>,
}

impl Dashboard {
    fn new(cx: &mut Context<Self>) -> Self {
        let stars = (0..420)
            .map(|i| Star {
                x: ((i * 73 + i * i * 17) % 1000) as f32 / 1000.0,
                y: ((i * 191 + i * i * 11) % 1000) as f32 / 1000.0,
                z: 0.12 + ((i * 37) % 100) as f32 / 100.0,
                phase: ((i * 29) % 360) as f32,
            })
            .collect();
        let animation = cx.spawn(
            async move |this: WeakEntity<Dashboard>, cx: &mut AsyncApp| loop {
                Timer::after(Duration::from_millis(33)).await;
                if this
                    .update(cx, |this, cx| {
                        if this.mode == Mode::Constellation {
                            this.advance(cx);
                        }
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
            tick: 0.0,
            stars,
            shooting_star: ShootingStar {
                x: 0.2,
                y: 0.15,
                vx: 0.004,
                vy: 0.0025,
                life: 1.0,
            },
            bursts: Vec::new(),
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
        self.bursts.push(Burst {
            x,
            y,
            age: 0.0,
            seed,
        });
        self.push_log("UI", "constellation burst", 0xf8fafc);
        cx.notify();
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.push_log(
            "UI",
            match mode {
                Mode::Constellation => "view constellation",
                Mode::Pipeline => "view pipeline",
            },
            0xf8fafc,
        );
        cx.notify();
    }

    fn shift(&mut self, amount: i64, cx: &mut Context<Self>) {
        self.n = (self.n + amount).clamp(0, 1_000_000);
        cx.notify();
    }

    fn dec_100(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift(-100, cx);
    }
    fn dec_10(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift(-10, cx);
    }
    fn dec_1(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift(-1, cx);
    }
    fn inc_1(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift(1, cx);
    }
    fn inc_10(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift(10, cx);
    }
    fn inc_100(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.shift(100, cx);
    }
    fn reset(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.n = 7;
        cx.notify();
    }

    fn advance(&mut self, cx: &mut Context<Self>) {
        self.tick += 0.065;
        for (i, star) in self.stars.iter_mut().enumerate() {
            let drift = ((self.tick * 0.6 + star.phase + i as f32 * 0.03).sin()) * 0.00065;
            star.x = (star.x + drift + 1.0) % 1.0;
            star.y = (star.y + drift * 0.72 + 1.0) % 1.0;
            star.phase += 0.015 + star.z * 0.005;
        }

        self.shooting_star.x += self.shooting_star.vx;
        self.shooting_star.y += self.shooting_star.vy;
        self.shooting_star.life -= 0.01;
        if self.shooting_star.x > 1.15
            || self.shooting_star.y > 1.15
            || self.shooting_star.life <= 0.0
        {
            self.shooting_star = ShootingStar {
                x: -0.1,
                y: 0.12 + (self.tick.sin() * 0.25 + 0.25),
                vx: 0.006,
                vy: 0.0022,
                life: 1.0,
            };
            self.push_log("Zig", "shooting star", 0xf59e0b);
        }

        for burst in &mut self.bursts {
            burst.age += 0.02;
        }
        self.bursts.retain(|burst| burst.age < 1.2);

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
        let bounds = window.bounds();
        let width = bounds.size.width.to_f64() as f32;
        let height = bounds.size.height.to_f64() as f32;

        let mode_content: AnyElement = match self.mode {
            Mode::Constellation => div().into_any_element(),
            Mode::Pipeline => pipeline_scene(pipeline_entries(n)).into_any_element(),
        };
        let background: AnyElement = match self.mode {
            Mode::Constellation => background_scene(
                width,
                height,
                self.tick,
                &self.stars,
                &self.shooting_star,
                &self.bursts,
                cx,
            )
            .into_any_element(),
            Mode::Pipeline => div()
                .absolute()
                .left(px(0.0))
                .top(px(0.0))
                .right(px(0.0))
                .bottom(px(0.0))
                .into_any_element(),
        };
        let mode_switcher = mode_switcher(self.mode, cx);
        let show_log = self.mode == Mode::Constellation;
        let log_panel: AnyElement = if show_log {
            constellation_log_panel(&self.stars, &self.shooting_star, &self.log).into_any_element()
        } else {
            div().into_any_element()
        };
        let show_pipeline_header = self.mode == Mode::Pipeline;
        let pipeline_title = title(self.mode);
        let pipeline_value = value_badge(self.n);
        let pipeline_controls = control_strip(cx);

        render_crepus_shell(CrepusShellParts {
            background,
            mode_switcher,
            show_log,
            log_panel,
            show_pipeline_header,
            pipeline_title,
            pipeline_value,
            pipeline_controls,
            mode_content,
        })
    }
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
        cx.listener(|this, _, _, cx| this.set_mode(Mode::Pipeline, cx)),
    )
    .when(mode == Mode::Pipeline, |cx| {
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

fn constellation_log_panel(
    stars: &[Star],
    shooting_star: &ShootingStar,
    log: &[LogEntry],
) -> impl IntoElement {
    let mut panel = div()
        .id("constellation-log")
        .absolute()
        .right(px(24.0))
        .bottom(px(24.0))
        .w(px(340.0))
        .max_h(px(260.0))
        .overflow_y_scroll()
        .scrollbar_width(px(8.0))
        .rounded_2xl()
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
            format!("field={} drift={:.3}", stars.len(), shooting_star.life),
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

fn control_strip(cx: &mut Context<Dashboard>) -> Div {
    div()
        .w_full()
        .max_w(px(430.0))
        .flex()
        .flex_nowrap()
        .gap_1()
        .items_end()
        .justify_end()
        .overflow_hidden()
        .mt(px(12.0))
        .child(control_pill("-100", cx.listener(Dashboard::dec_100)))
        .child(control_pill("-10", cx.listener(Dashboard::dec_10)))
        .child(control_pill("-1", cx.listener(Dashboard::dec_1)))
        .child(control_pill("+1", cx.listener(Dashboard::inc_1)))
        .child(control_pill("+10", cx.listener(Dashboard::inc_10)))
        .child(control_pill("+100", cx.listener(Dashboard::inc_100)))
        .child(control_pill("reset", cx.listener(Dashboard::reset)))
}

fn title(mode: Mode) -> Div {
    div()
        .text_2xl()
        .font_weight(FontWeight::BOLD)
        .child(match mode {
            Mode::Constellation => "Constellation Engine",
            Mode::Pipeline => "Math Pipeline",
        })
}

fn value_badge(n: i64) -> Div {
    div()
        .mt(px(4.0))
        .text_2xl()
        .font_weight(FontWeight::BOLD)
        .child(format!("n = {n}"))
}

fn pipeline_entries(n: i64) -> [(&'static str, String, u32); 8] {
    let c_body = if cfg!(has_c) {
        unsafe {
            format!(
                "add={}; gcd={}; fib={}; wave={}; orbit={}",
                c_ffi::c_add(n as _, n as _),
                c_ffi::c_gcd(n as _, (n + 1) as _),
                c_ffi::c_fibonacci(n as _),
                c_ffi::c_wave_hash(n as _),
                c_ffi::c_collatz_steps(n as _),
            )
        }
    } else {
        "C not linked".into()
    };

    let cpp_body = if cfg!(has_cpp) {
        let safe = n.min(20) as _;
        unsafe {
            format!(
                "len={}; factorial={}; primorial={}; digit_sum={}; prime={}",
                cpp_ffi::cpp_strlen(c"equilibrium".as_ptr() as _),
                cpp_ffi::cpp_factorial(safe),
                cpp_ffi::cpp_primorial(safe),
                cpp_ffi::cpp_digit_sum(n as _),
                cpp_ffi::cpp_is_prime(n as _) != 0,
            )
        }
    } else {
        "C++ not linked".into()
    };

    let zig_body = if cfg!(has_zig) {
        unsafe {
            format!(
                "square={}; spiral={}; chaos_fold={}",
                zig_square(n),
                zig_spiral_sum(n),
                zig_chaos_fold(n),
            )
        }
    } else {
        "Zig not linked".into()
    };

    let nim_body = if cfg!(has_nim) {
        unsafe {
            format!(
                "popcount={}; reverse={:#010x}; rotate={:#010x}",
                nim_popcount(n as u32),
                nim_reverse_bits(n as u32),
                nim_rotate_left(n as u32, (n as u32) & 31),
            )
        }
    } else {
        "Nim not linked".into()
    };

    let v_body = if cfg!(has_v) {
        unsafe {
            format!(
                "f_to_f={:.1}; km_to_mi={:.2}; k_to_r={:.1}",
                v_celsius_to_fahrenheit(n as f64),
                v_km_to_miles(n as f64),
                v_kelvin_to_rankine(n as f64 + 273.15),
            )
        }
    } else {
        "V not linked".into()
    };

    let d_body = unsafe {
        format!(
            "abs={}; triangular={}; clamp={}; collatz={}",
            d_abs(-(n as i32)),
            d_triangular(n as i32),
            d_clamp(n as i32, 3, 13),
            d_collatz_steps(n as i32),
        )
    };

    let odin_body = unsafe {
        format!(
            "abs={}; min={}; max={}; mix={}; clamp={}",
            odin_abs(-(n as i32)),
            odin_min(n as i32, (n + 3) as i32),
            odin_max(n as i32, (n + 3) as i32),
            odin_mix(n as i32, (n * 3 + 1) as i32),
            odin_clamp(n as i32, 5, 55),
        )
    };

    let rust_body = format!(
        "prime={}; next_prime={}; digit_sum={}; collatz={}",
        rust_is_prime(n as _),
        rust_next_prime(n as _),
        rust_digit_sum(n),
        rust_collatz_steps(n),
    );

    [
        ("C", c_body, 0x10b981),
        ("C++", cpp_body, 0x38bdf8),
        ("Zig", zig_body, 0xf59e0b),
        ("Nim", nim_body, 0x22d3ee),
        ("V", v_body, 0x4ade80),
        ("D", d_body, 0x60a5fa),
        ("Odin", odin_body, 0xfb7185),
        ("Rust", rust_body, 0xe879f9),
    ]
}

fn background_scene(
    width: f32,
    height: f32,
    tick: f32,
    stars: &[Star],
    shooting_star: &ShootingStar,
    bursts: &[Burst],
    cx: &mut Context<Dashboard>,
) -> Div {
    ascii_constellation(width, height, tick, stars, shooting_star, bursts, cx)
}

fn ascii_constellation(
    width: f32,
    height: f32,
    tick: f32,
    stars: &[Star],
    shooting_star: &ShootingStar,
    bursts: &[Burst],
    cx: &mut Context<Dashboard>,
) -> Div {
    let layer = div()
        .absolute()
        .left(px(0.0))
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .bg(rgb(0x050507))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(Dashboard::on_ascii_mouse_down),
        );

    let row_height = 8.0;
    let rows = ((height * 1.45 / row_height).ceil() as usize).clamp(64, 360);
    let cols = ((width * 1.75 / 3.7).ceil() as usize).clamp(180, 920);
    let tick_bucket = (tick * 6.0) as usize;
    let mut grid: Vec<Vec<AsciiCell>> = (0..rows)
        .into_par_iter()
        .map(|y| ascii_base_row(y, rows, cols, tick, tick_bucket))
        .collect();

    for star in stars {
        let x = (star.x * cols as f32).clamp(0.0, (cols - 1) as f32) as usize;
        let y = (star.y * rows as f32).clamp(0.0, (rows - 1) as f32) as usize;
        let pulse = ((star.phase + tick * (0.35 + star.z)).sin() + 1.0) * 0.5;
        let ch = match ((pulse * 5.0 + star.z * 3.0) as usize).min(7) {
            0 => '.',
            1 => '·',
            2 => ':',
            3 => '*',
            4 => '+',
            5 => 'o',
            6 => '✦',
            _ => '✧',
        };
        grid[y][x] = AsciiCell {
            ch,
            color: language_color(7),
        };
    }

    for i in 0..24 {
        let fade = i as f32 / 24.0;
        let tx = shooting_star.x - shooting_star.vx * i as f32 * 9.0;
        let ty = shooting_star.y - shooting_star.vy * i as f32 * 9.0;
        if (0.0..=1.0).contains(&tx) && (0.0..=1.0).contains(&ty) {
            let x = (tx * cols as f32).clamp(0.0, (cols - 1) as f32) as usize;
            let y = (ty * rows as f32).clamp(0.0, (rows - 1) as f32) as usize;
            grid[y][x] = AsciiCell {
                ch: if fade < 0.2 {
                    '✦'
                } else if fade < 0.45 {
                    '/'
                } else if fade < 0.7 {
                    '·'
                } else {
                    '.'
                },
                color: 0xf59e0b,
            };
        }
    }

    for burst in bursts {
        let bx = (burst.x * cols as f32).clamp(0.0, (cols - 1) as f32) as i32;
        let by = (burst.y * rows as f32).clamp(0.0, (rows - 1) as f32) as i32;
        let radius = 1 + (burst.age * 4.0) as i32;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let x = bx + dx;
                let y = by + dy;
                if x >= 0
                    && y >= 0
                    && (x as usize) < cols
                    && (y as usize) < rows
                    && dx.abs() + dy.abs() <= radius
                {
                    grid[y as usize][x as usize] = AsciiCell {
                        ch: match burst.seed % 4 {
                            0 => '@',
                            1 => '#',
                            2 => '%',
                            _ => '&',
                        },
                        color: if burst.seed % 2 == 0 {
                            language_color(5)
                        } else {
                            language_color(6)
                        },
                    };
                }
            }
        }
    }

    let mut rows_view = div()
        .absolute()
        .left(px(0.0))
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(0.0))
        .w_full()
        .h_full()
        .overflow_hidden()
        .font_family("monospace")
        .text_size(px(row_height))
        .flex()
        .flex_col();

    let rendered_rows: Vec<AsciiRow> = grid.into_par_iter().map(ascii_row_data).collect();
    for row in rendered_rows {
        rows_view = rows_view.child(ascii_row(row, row_height));
    }

    layer.child(rows_view)
}

fn ascii_base_row(
    y: usize,
    rows: usize,
    cols: usize,
    tick: f32,
    tick_bucket: usize,
) -> Vec<AsciiCell> {
    let fy = y as f32 / rows as f32;
    let y_wave = (fy * 10.0 - tick * 0.25).cos();
    (0..cols)
        .map(|x| {
            let fx = x as f32 / cols as f32;
            let ribbon =
                ((fx * 12.0 + tick * 0.35).sin() + y_wave + ((fx + fy) * 18.0).sin() * 0.45) / 2.45;
            let shimmer = (x * 17 + y * 29 + tick_bucket).is_multiple_of(113);
            let ch = if shimmer {
                ':'
            } else if ribbon > 0.78 {
                '.'
            } else if ribbon > 0.62 {
                '·'
            } else if ribbon < -0.86 {
                ','
            } else {
                ' '
            };
            let color = match ch {
                ':' => {
                    if (x + y + tick_bucket).is_multiple_of(2) {
                        language_color(3)
                    } else {
                        language_color(4)
                    }
                }
                '.' | '·' => language_color(0),
                ',' => language_color(1),
                _ => 0x334155,
            };
            AsciiCell { ch, color }
        })
        .collect()
}

fn language_color(index: usize) -> u32 {
    match index % 8 {
        0 => 0x10b981,
        1 => 0x38bdf8,
        2 => 0xf59e0b,
        3 => 0x22d3ee,
        4 => 0x4ade80,
        5 => 0x60a5fa,
        6 => 0xfb7185,
        _ => 0xe879f9,
    }
}

fn ascii_row_data(row: Vec<AsciiCell>) -> AsciiRow {
    let mut text = String::with_capacity(row.len() * 2);
    let mut highlights = Vec::new();
    let mut run_start = 0usize;
    let mut run_color = row.first().map_or(0x334155, |cell| cell.color);

    for cell in row {
        let start = text.len();
        if cell.color != run_color && start > run_start {
            highlights.push((run_start..start, rgb(run_color).into()));
            run_start = start;
            run_color = cell.color;
        }
        text.push(cell.ch);
    }

    if text.len() > run_start {
        highlights.push((run_start..text.len(), rgb(run_color).into()));
    }

    AsciiRow { text, highlights }
}

fn ascii_row(row: AsciiRow, row_height: f32) -> Div {
    div()
        .w_full()
        .h(px(row_height))
        .text_size(px(row_height))
        .line_height(px(row_height))
        .whitespace_nowrap()
        .child(StyledText::new(row.text).with_highlights(row.highlights))
}

fn status_line(lang: &str, text: String, color: u32) -> Div {
    div()
        .text_sm()
        .text_color(rgb(color))
        .child(format!("{lang}: {text}"))
}

fn pipeline_scene(entries: [(&str, String, u32); 8]) -> impl IntoElement {
    let mut panel = div()
        .id("pipeline-scroll")
        .absolute()
        .left(px(28.0))
        .top(px(170.0))
        .right(px(28.0))
        .bottom(px(24.0))
        .overflow_y_scroll()
        .scrollbar_width(px(8.0))
        .flex()
        .flex_col()
        .gap_3();

    for (title, body, accent) in entries {
        panel = panel.child(card(title, body, accent));
    }

    panel
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

fn card(title: &str, body: String, accent: u32) -> Div {
    div()
        .w_full()
        .rounded_2xl()
        .border_1()
        .border_color(rgb(0x27272a))
        .bg(rgb(0x111113))
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(accent))
                .child(title.to_string()),
        )
        .child(div().text_sm().text_color(rgb(0xe5e7eb)).child(body))
}

fn rust_is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    if n == 2 {
        return true;
    }
    if n.is_multiple_of(2) {
        return false;
    }
    let mut i = 3u64;
    while i * i <= n {
        if n.is_multiple_of(i) {
            return false;
        }
        i += 2;
    }
    true
}

fn rust_next_prime(after: u64) -> u64 {
    let mut n = after + 1;
    while !rust_is_prime(n) {
        n += 1;
    }
    n
}

fn rust_digit_sum(mut n: i64) -> i64 {
    let mut sum = 0;
    n = n.abs();
    while n > 0 {
        sum += n % 10;
        n /= 10;
    }
    sum
}

fn rust_collatz_steps(mut n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }

    let mut steps = 0;
    while n != 1 {
        if n % 2 == 0 {
            n /= 2;
        } else {
            n = n * 3 + 1;
        }
        steps += 1;
    }
    steps
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
