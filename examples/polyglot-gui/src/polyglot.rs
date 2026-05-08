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
    fn zig_sum_1_to_n(n: i64) -> i64;
    fn zig_is_power_of_two(n: u64) -> bool;
    fn zig_spiral_sum(n: i64) -> i64;
    fn zig_chaos_fold(n: i64) -> i64;
}

#[cfg(not(has_zig))]
unsafe fn zig_square(_: i64) -> i64 {
    0
}

#[cfg(not(has_zig))]
unsafe fn zig_sum_1_to_n(_: i64) -> i64 {
    0
}

#[cfg(not(has_zig))]
unsafe fn zig_is_power_of_two(_: u64) -> bool {
    false
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

#[derive(Clone)]
pub struct ResultRow {
    pub lang: &'static str,
    pub linked: bool,
    pub result: String,
    pub accent: &'static str,
}

pub struct Snapshot {
    pub n: i64,
    pub rows: Vec<ResultRow>,
    pub linked_count: i64,
    pub missing_count: i64,
}

#[derive(Clone)]
pub struct Star {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub phase: f32,
}

#[derive(Clone)]
pub struct ShootingStar {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32,
}

#[derive(Clone)]
pub struct Burst {
    pub x: f32,
    pub y: f32,
    pub age: f32,
    pub seed: u32,
}

#[derive(Clone)]
pub struct ConstellationState {
    pub tick: f32,
    pub stars: Vec<Star>,
    pub shooting_star: ShootingStar,
    pub bursts: Vec<Burst>,
}

#[derive(Clone, Copy)]
pub struct ConstellationCell {
    pub ch: char,
    pub color: u32,
}

pub struct ConstellationSpan {
    pub start: usize,
    pub end: usize,
    pub color: u32,
}

pub struct ConstellationRow {
    pub text: String,
    pub spans: Vec<ConstellationSpan>,
}

pub struct ConstellationFrame {
    pub rows: Vec<ConstellationRow>,
}

impl Default for ConstellationState {
    fn default() -> Self {
        let stars = (0..420)
            .map(|i| Star {
                x: ((i * 73 + i * i * 17) % 1000) as f32 / 1000.0,
                y: ((i * 191 + i * i * 11) % 1000) as f32 / 1000.0,
                z: 0.12 + ((i * 37) % 100) as f32 / 100.0,
                phase: ((i * 29) % 360) as f32,
            })
            .collect();

        Self {
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
        }
    }
}

impl ConstellationState {
    pub fn advance(&mut self) -> bool {
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
        let reset_shooting_star = self.shooting_star.x > 1.15
            || self.shooting_star.y > 1.15
            || self.shooting_star.life <= 0.0;
        if reset_shooting_star {
            self.shooting_star = ShootingStar {
                x: -0.1,
                y: 0.12 + (self.tick.sin() * 0.25 + 0.25),
                vx: 0.006,
                vy: 0.0022,
                life: 1.0,
            };
        }

        for burst in &mut self.bursts {
            burst.age += 0.02;
        }
        self.bursts.retain(|burst| burst.age < 1.2);

        reset_shooting_star
    }

    pub fn burst(&mut self, x: f32, y: f32, seed: u32) {
        self.bursts.push(Burst {
            x,
            y,
            age: 0.0,
            seed,
        });
    }

    pub fn frame(&self, width: usize, height: usize) -> ConstellationFrame {
        constellation_frame(
            width,
            height,
            self.tick,
            &self.stars,
            &self.shooting_star,
            &self.bursts,
        )
    }
}

pub fn snapshot(n: i64) -> Snapshot {
    let rows = result_rows(n);
    let linked_count = rows.iter().filter(|row| row.linked).count() as i64;
    let missing_count = rows.len() as i64 - linked_count;

    Snapshot {
        n,
        rows,
        linked_count,
        missing_count,
    }
}

pub fn result_rows(n: i64) -> Vec<ResultRow> {
    let mut rows = Vec::with_capacity(8);

    #[cfg(has_c)]
    let c_result = unsafe {
        format!(
            "add={}; gcd={}; fib={}; wave={}; orbit={}",
            c_ffi::c_add(n as _, n as _),
            c_ffi::c_gcd(n as _, (n + 1) as _),
            c_ffi::c_fibonacci(n as _),
            c_ffi::c_wave_hash(n as _),
            c_ffi::c_collatz_steps(n as _),
        )
    };
    #[cfg(not(has_c))]
    let c_result = String::from("C not linked");
    rows.push(result_row("C", cfg!(has_c), c_result, 0x10b981));

    #[cfg(has_cpp)]
    let cpp_result = {
        let safe = n.min(20) as _;
        unsafe {
            format!(
                "factorial={}; len={}; primorial={}; digit_sum={}; prime={}",
                cpp_ffi::cpp_factorial(safe),
                cpp_ffi::cpp_strlen(c"equilibrium".as_ptr() as _),
                cpp_ffi::cpp_primorial(safe),
                cpp_ffi::cpp_digit_sum(n as _),
                cpp_ffi::cpp_is_prime(n as _) != 0,
            )
        }
    };
    #[cfg(not(has_cpp))]
    let cpp_result = String::from("C++ not linked");
    rows.push(result_row("C++", cfg!(has_cpp), cpp_result, 0x38bdf8));

    let zig_result = if cfg!(has_zig) {
        unsafe {
            format!(
                "square={}; sum={}; pow2={}; spiral={}; chaos={}",
                zig_square(n),
                zig_sum_1_to_n(n),
                zig_is_power_of_two(n as _),
                zig_spiral_sum(n),
                zig_chaos_fold(n),
            )
        }
    } else {
        "Zig not linked".into()
    };
    rows.push(result_row("Zig", cfg!(has_zig), zig_result, 0xf59e0b));

    let nim_result = if cfg!(has_nim) {
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
    rows.push(result_row("Nim", cfg!(has_nim), nim_result, 0x22d3ee));

    let v_result = if cfg!(has_v) {
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
    rows.push(result_row("V", cfg!(has_v), v_result, 0x4ade80));

    let d_result = if cfg!(has_d) {
        unsafe {
            format!(
                "abs={}; triangular={}; clamp={}; collatz={}",
                d_abs(-(n as i32)),
                d_triangular(n as i32),
                d_clamp(n as i32, 3, 13),
                d_collatz_steps(n as i32),
            )
        }
    } else {
        "D not linked".into()
    };
    rows.push(result_row("D", cfg!(has_d), d_result, 0x60a5fa));

    let odin_result = if cfg!(has_odin) {
        unsafe {
            format!(
                "abs={}; min={}; max={}; mix={}; clamp={}",
                odin_abs(-(n as i32)),
                odin_min(n as i32, (n + 3) as i32),
                odin_max(n as i32, (n + 3) as i32),
                odin_mix(n as i32, (n * 3 + 1) as i32),
                odin_clamp(n as i32, 5, 55),
            )
        }
    } else {
        "Odin not linked".into()
    };
    rows.push(result_row("Odin", cfg!(has_odin), odin_result, 0xfb7185));

    let rust_result = format!(
        "prime={}; next_prime={}; digit_sum={}; collatz={}",
        rust_is_prime(n as _),
        rust_next_prime(n as _),
        rust_digit_sum(n),
        rust_collatz_steps(n),
    );
    rows.push(result_row("Rust", true, rust_result, 0xe879f9));

    rows
}

fn result_row(lang: &'static str, linked: bool, result: String, color: u32) -> ResultRow {
    let _ = color;
    ResultRow {
        lang,
        linked,
        result,
        accent: language_accent(lang),
    }
}

fn language_accent(lang: &str) -> &'static str {
    match lang {
        "C" => "text-emerald-400",
        "C++" => "text-sky-400",
        "Zig" => "text-amber-400",
        "Nim" => "text-cyan-400",
        "V" => "text-green-400",
        "D" => "text-blue-400",
        "Odin" => "text-rose-400",
        _ => "text-fuchsia-400",
    }
}

pub fn constellation_lines(frame: &ConstellationFrame) -> Vec<String> {
    frame
        .rows
        .iter()
        .map(|row| {
            debug_assert!(row.spans.iter().all(|span| span.start <= span.end
                && span.end <= row.text.len()
                && span.color <= 0x00ff_ffff));
            row.text.clone()
        })
        .collect()
}

fn constellation_frame(
    width: usize,
    height: usize,
    tick: f32,
    stars: &[Star],
    shooting_star: &ShootingStar,
    bursts: &[Burst],
) -> ConstellationFrame {
    let rows = height.clamp(6, 360);
    let cols = width.clamp(20, 920);
    let tick_bucket = (tick * 6.0) as usize;
    let mut grid: Vec<Vec<ConstellationCell>> = (0..rows)
        .map(|y| constellation_base_row(y, rows, cols, tick, tick_bucket))
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
        grid[y][x] = ConstellationCell {
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
            grid[y][x] = ConstellationCell {
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
                    grid[y as usize][x as usize] = ConstellationCell {
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

    ConstellationFrame {
        rows: grid.into_iter().map(constellation_row_data).collect(),
    }
}

fn constellation_base_row(
    y: usize,
    rows: usize,
    cols: usize,
    tick: f32,
    tick_bucket: usize,
) -> Vec<ConstellationCell> {
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
            ConstellationCell { ch, color }
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

fn constellation_row_data(row: Vec<ConstellationCell>) -> ConstellationRow {
    let mut text = String::with_capacity(row.len() * 2);
    let mut spans = Vec::new();
    let mut run_start = 0usize;
    let mut run_color = row.first().map_or(0x334155, |cell| cell.color);

    for cell in row {
        let start = text.len();
        if cell.color != run_color && start > run_start {
            spans.push(ConstellationSpan {
                start: run_start,
                end: start,
                color: run_color,
            });
            run_start = start;
            run_color = cell.color;
        }
        text.push(cell.ch);
    }

    if text.len() > run_start {
        spans.push(ConstellationSpan {
            start: run_start,
            end: text.len(),
            color: run_color,
        });
    }

    ConstellationRow { text, spans }
}

pub fn rust_is_prime(n: u64) -> bool {
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
