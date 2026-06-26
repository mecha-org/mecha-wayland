use std::io::{self, Write, stdout};
use std::time::{Duration, Instant};

use animation::{Animated, AnimationConfig, Easing, monotonic_now};
use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal;
use crossterm::{cursor, execute};

fn bar(label: &str, val: f32, width: usize) -> String {
    let n = (val.clamp(0.0, 1.0) * width as f32).round() as usize;
    format!(
        "{label:<12}[{}{}] {val:.2}",
        "█".repeat(n),
        "·".repeat(width - n)
    )
}

fn color_bar(val: f32, width: usize) -> String {
    let (r, g, b) = animation::lerp_color((1.0, 0.2, 0.2), (0.2, 0.3, 1.0), val);
    let bw = (val.clamp(0.0, 1.0) * width as f32).round() as usize;
    let r8 = (r * 255.0) as u8;
    let g8 = (g * 255.0) as u8;
    let b8 = (b * 255.0) as u8;
    format!(
        "\x1b[48;2;{r8};{g8};{b8}m{}\x1b[0m{}",
        " ".repeat(bw),
        "\u{b7}".repeat(width - bw),
    )
}

macro_rules! L { ($($arg:tt)*) => { format!($($arg)*) + "\r\n" }; }

fn maybe_bar(opt: &Option<Animated<f32>>, label: &str, wb: usize) -> String {
    match opt {
        Some(a) => bar(label, a.get(animation::monotonic_now()), wb),
        None => bar("[cancelled] ", 0.0, wb),
    }
}

fn bump_bar(opt: &Option<Animated<f32>>, wb: usize) -> String {
    match opt {
        Some(a) => {
            let raw = a.get(animation::monotonic_now());
            bar(&format!("7 raw:{raw:.2}"), (raw / 3.0).clamp(0.0, 1.0), wb)
        }
        None => bar("[cancelled] ", 0.0, wb),
    }
}

fn maybe_color(opt: &Option<Animated<f32>>, wb: usize) -> String {
    match opt {
        Some(a) => format!(
            "9           {}",
            color_bar(a.get(animation::monotonic_now()), wb)
        ),
        None => bar("[cancelled] ", 0.0, wb),
    }
}

fn any_active(ids: &TrackIds) -> bool {
    let now = animation::monotonic_now();
    ids.lin.as_ref().map_or(false, |a| a.is_animating(now))
        || ids.ein.as_ref().map_or(false, |a| a.is_animating(now))
        || ids.eout.as_ref().map_or(false, |a| a.is_animating(now))
        || ids.eio.as_ref().map_or(false, |a| a.is_animating(now))
        || ids.one.as_ref().map_or(false, |a| a.is_animating(now))
        || ids.pp.as_ref().map_or(false, |a| a.is_animating(now))
        || ids.by.as_ref().map_or(false, |a| a.is_animating(now))
        || ids.del.as_ref().map_or(false, |a| a.is_animating(now))
        || ids.clr.as_ref().map_or(false, |a| a.is_animating(now))
}

fn draw(
    w: &mut impl Write,
    ids: &TrackIds,
    start: Instant,
    cols: u16,
    mode: Mode,
) -> io::Result<()> {
    let wb = cols.saturating_sub(16).min(60) as usize;
    let t = start.elapsed().as_secs_f32();
    let mode_hint = match mode {
        Mode::Normal => "",
        Mode::Cancel => " [CANCEL: press digit, c=all]",
        Mode::Restart => " [RESTART: press digit, r=all]",
    };

    let out = format!(
        "\x1b[H{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
        L!(
            "{:<w$}",
            format!(
                "ANIMATION TUI  t={t:.1}s  active={}{mode_hint}",
                any_active(ids)
            ),
            w = cols as usize
        ),
        L!("═══ EASING — 4s one-shot, delay=0  [keys 1-4]"),
        L!("{}", maybe_bar(&ids.lin, "1 Linear", wb)),
        L!("{}", maybe_bar(&ids.ein, "2 EaseIn", wb)),
        L!("{}", maybe_bar(&ids.eout, "3 EaseOut", wb)),
        L!("{}", maybe_bar(&ids.eio, "4 EaseInOut", wb)),
        L!("═══ ONE-SHOT — 3s, EaseInOut, delay=0  [key 5]"),
        L!("{}", maybe_bar(&ids.one, "5", wb)),
        L!("═══ PING-PONG — 1.5s/1.5s, EaseInOut, 2s idle, delay=0  [key 6]"),
        L!("{}", maybe_bar(&ids.pp, "6", wb)),
        L!("═══ ANIMATE-BY — ±0.3 bump, EaseOut 300ms, cap 3.0  [+/- bump, key 7]"),
        L!("{}", bump_bar(&ids.by, wb)),
        L!("═══ DELAY — 2s delay, 3s EaseInOut one-shot  [key 8]"),
        L!("{}", maybe_bar(&ids.del, "8", wb)),
        L!("═══ COLOR LERP — red→blue, 4s EaseInOut ping-pong, 1s idle  [key 9]"),
        L!("{}", maybe_color(&ids.clr, wb)),
        L!("─── +/-:bump  c:cancel  r:restart  q:quit ───"),
    );

    write!(w, "{out}")?;
    w.flush()
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    Cancel,
    Restart,
}

struct TrackIds {
    lin: Option<Animated<f32>>,
    ein: Option<Animated<f32>>,
    eout: Option<Animated<f32>>,
    eio: Option<Animated<f32>>,
    one: Option<Animated<f32>>,
    pp: Option<Animated<f32>>,
    by: Option<Animated<f32>>,
    del: Option<Animated<f32>>,
    clr: Option<Animated<f32>>,
}

impl TrackIds {
    fn at(&mut self, digit: char) -> Option<&mut Option<Animated<f32>>> {
        match digit {
            '1' => Some(&mut self.lin),
            '2' => Some(&mut self.ein),
            '3' => Some(&mut self.eout),
            '4' => Some(&mut self.eio),
            '5' => Some(&mut self.one),
            '6' => Some(&mut self.pp),
            '7' => Some(&mut self.by),
            '8' => Some(&mut self.del),
            '9' => Some(&mut self.clr),
            _ => None,
        }
    }
}

fn create_one(d: char) -> Animated<f32> {
    let now = monotonic_now();
    match d {
        '1' => Animated::new(
            0.0,
            1.0,
            AnimationConfig::new(Duration::from_secs(4), Easing::Linear),
            now,
        ),
        '2' => Animated::new(
            0.0,
            1.0,
            AnimationConfig::new(Duration::from_secs(4), Easing::EaseIn),
            now,
        ),
        '3' => Animated::new(
            0.0,
            1.0,
            AnimationConfig::new(Duration::from_secs(4), Easing::EaseOut),
            now,
        ),
        '4' => Animated::new(
            0.0,
            1.0,
            AnimationConfig::new(Duration::from_secs(4), Easing::EaseInOut),
            now,
        ),
        '5' => Animated::new(
            0.0,
            1.0,
            AnimationConfig::new(Duration::from_secs(3), Easing::EaseInOut),
            now,
        ),
        '6' => Animated::new_pingpong(
            0.0,
            1.0,
            Duration::from_millis(1500),
            Easing::EaseInOut,
            Duration::from_secs(2),
            now,
        ),
        '7' => Animated::new(
            0.0,
            0.0,
            AnimationConfig::new(Duration::from_secs(1000), Easing::Linear),
            now,
        ),
        '8' => Animated::new(
            0.0,
            1.0,
            AnimationConfig {
                duration: Duration::from_secs(3),
                easing: Easing::EaseInOut,
                delay: Duration::from_secs(2),
            },
            now,
        ),
        '9' => Animated::new_pingpong(
            0.0,
            1.0,
            Duration::from_secs(4),
            Easing::EaseInOut,
            Duration::from_secs(1),
            now,
        ),
        _ => unreachable!(),
    }
}

fn create_all() -> TrackIds {
    TrackIds {
        lin: Some(create_one('1')),
        ein: Some(create_one('2')),
        eout: Some(create_one('3')),
        eio: Some(create_one('4')),
        one: Some(create_one('5')),
        pp: Some(create_one('6')),
        by: Some(create_one('7')),
        del: Some(create_one('8')),
        clr: Some(create_one('9')),
    }
}

fn main() -> io::Result<()> {
    let mut stdout = stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let (cols, _rows) = terminal::size()?;

    let mut ids = create_all();
    let start = Instant::now();
    let mut mode = Mode::Normal;

    loop {
        draw(&mut stdout, &ids, start, cols, mode)?;

        while event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(k) => match (mode, k.code) {
                    (_, KeyCode::Char('q')) | (_, KeyCode::Esc) => {
                        execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
                        terminal::disable_raw_mode()?;
                        return Ok(());
                    }
                    (Mode::Normal, KeyCode::Char('c')) => mode = Mode::Cancel,
                    (Mode::Normal, KeyCode::Char('r')) => mode = Mode::Restart,
                    (Mode::Normal, KeyCode::Char('+')) => {
                        if let Some(ref mut a) = ids.by {
                            let now = animation::monotonic_now();
                            let v = a.get(now);
                            if v < 3.0 {
                                a.animate_to(
                                    now,
                                    v + 0.3,
                                    AnimationConfig::new(
                                        Duration::from_millis(300),
                                        Easing::EaseOut,
                                    ),
                                );
                            }
                        }
                    }
                    (Mode::Normal, KeyCode::Char('-')) => {
                        if let Some(ref mut a) = ids.by {
                            let now = animation::monotonic_now();
                            let v = a.get(now);
                            if v > 0.0 {
                                a.animate_to(
                                    now,
                                    v - 0.3,
                                    AnimationConfig::new(
                                        Duration::from_millis(300),
                                        Easing::EaseOut,
                                    ),
                                );
                            }
                        }
                    }

                    (Mode::Cancel, KeyCode::Char('c')) => {
                        ids = TrackIds {
                            lin: None,
                            ein: None,
                            eout: None,
                            eio: None,
                            one: None,
                            pp: None,
                            by: None,
                            del: None,
                            clr: None,
                        };
                        mode = Mode::Normal;
                    }
                    (Mode::Cancel, KeyCode::Char(d)) if d.is_ascii_digit() => {
                        if let Some(slot) = ids.at(d) {
                            *slot = None;
                        }
                        mode = Mode::Normal;
                    }
                    (Mode::Cancel, _) => mode = Mode::Normal,

                    (Mode::Restart, KeyCode::Char('r')) => {
                        ids = create_all();
                        mode = Mode::Normal;
                    }
                    (Mode::Restart, KeyCode::Char(d)) if d.is_ascii_digit() => {
                        if let Some(slot) = ids.at(d) {
                            *slot = Some(create_one(d));
                        }
                        mode = Mode::Normal;
                    }
                    (Mode::Restart, _) => mode = Mode::Normal,

                    _ => {}
                },
                _ => {}
            }
        }

        std::thread::sleep(Duration::from_millis(33));
    }
}
