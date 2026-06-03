use super::UI_FONT_INTER_16;

#[derive(Debug)]
pub struct ClockChanged;
impl app::Event for ClockChanged {}

#[derive(Debug)]
pub struct ClockUpdate(pub u32, pub u32);
impl app::Event for ClockUpdate {}

#[derive(Debug)]
pub struct ClockWidget {
    pub time_str: String,
    pub format_24h: bool,
    pub show_date: bool,
}

impl ClockWidget {
    pub fn new() -> Self {
        let (h, m) = local_time();
        let mut s = Self {
            time_str: String::new(),
            format_24h: false,
            show_date: true,
        };
        s.time_str = s.formatted_text(h, m);
        s
    }

    fn formatted_text(&self, h: u32, m: u32) -> String {
        let time = if self.format_24h {
            format!("{:02}:{:02}", h, m)
        } else {
            let ampm = if h < 12 { "AM" } else { "PM" };
            let hour = if h == 0 {
                12
            } else if h > 12 {
                h - 12
            } else {
                h
            };
            format!("{:02}:{:02} {}", hour, m, ampm)
        };
        if self.show_date {
            let (_, _, day, mon) = local_full();
            let months = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            format!("{} {}  {}", day, months[mon as usize], time)
        } else {
            time
        }
    }

    pub fn slot_width(&self) -> f32 {
        let template = if self.show_date {
            if self.format_24h { "00 Xxx  00:00" } else { "00 Xxx  00:00 PM" }
        } else {
            if self.format_24h { "00:00" } else { "00:00 PM" }
        };
        UI_FONT_INTER_16.measure_width(template) + 8.0
    }
}

// SAFETY: time(NULL) and localtime_r are thread-safe on single-threaded use.
pub fn local_time() -> (u32, u32) {
    let (h, m, _, _) = local_full();
    (h, m)
}

fn local_full() -> (u32, u32, u32, u32) {
    let mut result = (0, 0, 1, 0);
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let ptr = unsafe { libc::localtime_r(&now, &mut tm) };
    if !ptr.is_null() {
        result = (
            tm.tm_hour as u32,
            tm.tm_min as u32,
            tm.tm_mday as u32,
            tm.tm_mon as u32,
        );
    }
    result
}

pub fn module<AppState>() -> impl app::RegisteredModule<ClockWidget, AppState> {
    app::Module::new().on(|w: &mut ClockWidget, ev: &ClockUpdate| {
        let new = w.formatted_text(ev.0, ev.1);
        if w.time_str != new {
            w.time_str = new;
            return Some(ClockChanged);
        }
        None
    })
}
