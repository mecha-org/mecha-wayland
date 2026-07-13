use super::UI_FONT_INTER_16;
use crate::time::{self, Precision};

#[derive(Debug)]
pub struct ClockChanged;
impl app::Event for ClockChanged {}

#[derive(Debug)]
pub struct ClockUpdate(pub u32, pub u32, pub u32, pub u32, pub u32); // h, m, s, day, mon
impl app::Event for ClockUpdate {}

#[derive(Debug)]
pub struct ClockWidget {
    pub time_str: String,
    pub format_24h: bool,
    pub show_date: bool,
    pub show_seconds: bool,
}

impl ClockWidget {
    pub fn new() -> Self {
        let (h, m, s, day, mon) = time::local_time();
        let mut w = Self {
            time_str: String::new(),
            format_24h: false,
            show_date: true,
            show_seconds: false,
        };
        w.time_str = w.formatted_text(h, m, s, day, mon);
        w
    }

    fn formatted_text(&self, h: u32, m: u32, s: u32, day: u32, mon: u32) -> String {
        let hour = if self.format_24h {
            h
        } else {
            ((h + 11) % 12) + 1
        };

        let mut text = if self.show_seconds {
            format!("{:02}:{:02}:{:02}", hour, m, s)
        } else {
            format!("{:02}:{:02}", hour, m)
        };

        if !self.format_24h {
            text.push_str(if h < 12 { " AM" } else { " PM" });
        }

        if self.show_date {
            const MONTHS: &[&str] = &[
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            text = format!("{} {}  {}", day, MONTHS[mon as usize], text);
        }

        text
    }

    pub fn precision(&self) -> Precision {
        if self.show_seconds {
            Precision::Seconds
        } else {
            Precision::Minutes
        }
    }

    pub fn slot_width(&self) -> f32 {
        let template = match (self.show_date, self.format_24h, self.show_seconds) {
            (true, true, true) => "00 Xxx  00:00:00",
            (true, true, false) => "00 Xxx  00:00",
            (true, false, true) => "00 Xxx  00:00:00 AM",
            (true, false, false) => "00 Xxx  00:00 AM",
            (false, true, true) => "00:00:00",
            (false, true, false) => "00:00",
            (false, false, true) => "00:00:00 AM",
            (false, false, false) => "00:00 AM",
        };
        UI_FONT_INTER_16.measure_width(template) + 8.0
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<ClockWidget, AppState> {
    app::Module::new().on(|w: &mut ClockWidget, ev: &ClockUpdate| {
        let new = w.formatted_text(ev.0, ev.1, ev.2, ev.3, ev.4);
        if w.time_str != new {
            w.time_str = new;
            return Some(ClockChanged);
        }
        None
    })
}
