use std::io::{self, Write, stdout};
use std::time::{Duration, Instant};

use animation::{AnimationConfig, Animator, Easing};
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

fn maybe_bar(
    opt: Option<animation::AnimationId>,
    anim: &Animator,
    label: &str,
    wb: usize,
) -> String {
    match opt {
        Some(id) => bar(label, anim.get(id), wb),
        None => bar("[cancelled] ", 0.0, wb),
    }
}

fn bump_bar(opt: Option<animation::AnimationId>, anim: &Animator, wb: usize) -> String {
    match opt {
        Some(id) => {
            let raw = anim.get(id);
            bar(&format!("7 raw:{raw:.2}"), (raw / 3.0).clamp(0.0, 1.0), wb)
        }
        None => bar("[cancelled] ", 0.0, wb),
    }
}

fn maybe_color(opt: Option<animation::AnimationId>, anim: &Animator, wb: usize) -> String {
    match opt {
        Some(id) => format!("9           {}", color_bar(anim.get(id), wb)),
        None => bar("[cancelled] ", 0.0, wb),
    }
}

fn draw(
    w: &mut impl Write,
    anim: &Animator,
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
                anim.is_active()
            ),
            w = cols as usize
        ),
        L!("═══ EASING — 4s one-shot, delay=0  [keys 1-4]"),
        L!("{}", maybe_bar(ids.lin, anim, "1 Linear", wb)),
        L!("{}", maybe_bar(ids.ein, anim, "2 EaseIn", wb)),
        L!("{}", maybe_bar(ids.eout, anim, "3 EaseOut", wb)),
        L!("{}", maybe_bar(ids.eio, anim, "4 EaseInOut", wb)),
        L!("═══ ONE-SHOT — 3s, EaseInOut, delay=0  [key 5]"),
        L!("{}", maybe_bar(ids.one, anim, "5", wb)),
        L!("═══ PING-PONG — 1.5s/1.5s, EaseInOut, 2s idle, delay=0  [key 6]"),
        L!("{}", maybe_bar(ids.pp, anim, "6", wb)),
        L!("═══ ANIMATE-BY — \u{b1}0.3 bump, EaseOut 300ms, cap 3.0  [+/- bump, key 7]"),
        L!("{}", bump_bar(ids.by, anim, wb)),
        L!("═══ DELAY — 2s delay, 3s EaseInOut one-shot  [key 8]"),
        L!("{}", maybe_bar(ids.del, anim, "8", wb)),
        L!("═══ COLOR LERP — red\u{2192}blue, 4s EaseInOut ping-pong, 1s idle  [key 9]"),
        L!("{}", maybe_color(ids.clr, anim, wb)),
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
    lin: Option<animation::AnimationId>,
    ein: Option<animation::AnimationId>,
    eout: Option<animation::AnimationId>,
    eio: Option<animation::AnimationId>,
    one: Option<animation::AnimationId>,
    pp: Option<animation::AnimationId>,
    by: Option<animation::AnimationId>,
    del: Option<animation::AnimationId>,
    clr: Option<animation::AnimationId>,
}

impl TrackIds {
    fn at(&mut self, digit: char) -> Option<&mut Option<animation::AnimationId>> {
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

fn create_one(anim: &mut Animator, d: char) -> animation::AnimationId {
    match d {
        '1' => anim.animate(AnimationConfig::immediate(
            0.0,
            1.0,
            Duration::from_secs(4),
            Easing::Linear,
        )),
        '2' => anim.animate(AnimationConfig::immediate(
            0.0,
            1.0,
            Duration::from_secs(4),
            Easing::EaseIn,
        )),
        '3' => anim.animate(AnimationConfig::immediate(
            0.0,
            1.0,
            Duration::from_secs(4),
            Easing::EaseOut,
        )),
        '4' => anim.animate(AnimationConfig::immediate(
            0.0,
            1.0,
            Duration::from_secs(4),
            Easing::EaseInOut,
        )),
        '5' => anim.animate(AnimationConfig::immediate(
            0.0,
            1.0,
            Duration::from_secs(3),
            Easing::EaseInOut,
        )),
        '6' => anim.animate_pingpong(
            AnimationConfig::immediate(0.0, 1.0, Duration::from_millis(1500), Easing::EaseInOut),
            Duration::from_secs(2),
        ),
        '7' => anim.animate(AnimationConfig::immediate(
            0.0,
            0.0,
            Duration::from_secs(1000),
            Easing::Linear,
        )),
        '8' => anim.animate(AnimationConfig {
            from: 0.0,
            to: 1.0,
            duration: Duration::from_secs(3),
            easing: Easing::EaseInOut,
            delay: Duration::from_secs(2),
        }),
        '9' => anim.animate_pingpong(
            AnimationConfig::immediate(0.0, 1.0, Duration::from_secs(4), Easing::EaseInOut),
            Duration::from_secs(1),
        ),
        _ => unreachable!(),
    }
}

fn create_all(anim: &mut Animator) -> TrackIds {
    TrackIds {
        lin: Some(create_one(anim, '1')),
        ein: Some(create_one(anim, '2')),
        eout: Some(create_one(anim, '3')),
        eio: Some(create_one(anim, '4')),
        one: Some(create_one(anim, '5')),
        pp: Some(create_one(anim, '6')),
        by: Some(create_one(anim, '7')),
        del: Some(create_one(anim, '8')),
        clr: Some(create_one(anim, '9')),
    }
}

fn main() -> io::Result<()> {
    let mut stdout = stdout();
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;

    let (cols, _rows) = terminal::size()?;

    let mut anim = Animator::new();
    let mut ids = create_all(&mut anim);
    let start = Instant::now();
    let mut mode = Mode::Normal;

    loop {
        draw(&mut stdout, &anim, &ids, start, cols, mode)?;

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
                        if let Some(id) = ids.by {
                            let v = anim.get(id);
                            if v < 3.0 {
                                anim.animate_by(
                                    id,
                                    0.3,
                                    Duration::from_millis(300),
                                    Easing::EaseOut,
                                );
                            }
                        }
                    }
                    (Mode::Normal, KeyCode::Char('-')) => {
                        if let Some(id) = ids.by {
                            let v = anim.get(id);
                            if v > 0.0 {
                                anim.animate_by(
                                    id,
                                    -0.3,
                                    Duration::from_millis(300),
                                    Easing::EaseOut,
                                );
                            }
                        }
                    }

                    (Mode::Cancel, KeyCode::Char('c')) => {
                        for slot in [
                            &mut ids.lin,
                            &mut ids.ein,
                            &mut ids.eout,
                            &mut ids.eio,
                            &mut ids.one,
                            &mut ids.pp,
                            &mut ids.by,
                            &mut ids.del,
                            &mut ids.clr,
                        ] {
                            if let Some(id) = slot.take() {
                                anim.cancel(id);
                            }
                        }
                        mode = Mode::Normal;
                    }
                    (Mode::Cancel, KeyCode::Char(d)) if d.is_ascii_digit() => {
                        if let Some(slot) = ids.at(d) {
                            if let Some(id) = slot.take() {
                                anim.cancel(id);
                            }
                        }
                        mode = Mode::Normal;
                    }
                    (Mode::Cancel, _) => mode = Mode::Normal,

                    (Mode::Restart, KeyCode::Char('r')) => {
                        anim = Animator::new();
                        ids = create_all(&mut anim);
                        mode = Mode::Normal;
                    }
                    (Mode::Restart, KeyCode::Char(d)) if d.is_ascii_digit() => {
                        if let Some(slot) = ids.at(d) {
                            if let Some(id) = slot.take() {
                                anim.cancel(id);
                            }
                            *slot = Some(create_one(&mut anim, d));
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
