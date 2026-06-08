use super::*;
use assets::SpriteRegion;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BatteryState {
    pub pct: u8,
    pub charging: bool,
    pub show_percentage: bool,
}

impl Default for BatteryState {
    fn default() -> Self {
        Self {
            pct: 100,
            charging: false,
            show_percentage: false,
        }
    }
}

#[derive(Debug)]
pub struct BatteryChanged;
impl app::Event for BatteryChanged {}

#[derive(Debug)]
pub struct BatteryUpdate {
    pub pct: u8,
    pub charging: bool,
}
impl app::Event for BatteryUpdate {}

#[derive(Debug)]
pub struct BatteryWidget {
    pub state: BatteryState,
    pub pct_text: String,
}

impl Default for BatteryWidget {
    fn default() -> Self {
        Self {
            state: BatteryState::default(),
            pct_text: String::from("100"),
        }
    }
}

impl BatteryWidget {
    pub fn new() -> Self {
        Self::default()
    }

    fn sprite_arrays() -> ([&'static SpriteRegion; 11], [&'static SpriteRegion; 11]) {
        (
            [
                &UI_BATTERY_0,
                &UI_BATTERY_10,
                &UI_BATTERY_20,
                &UI_BATTERY_30,
                &UI_BATTERY_40,
                &UI_BATTERY_50,
                &UI_BATTERY_60,
                &UI_BATTERY_70,
                &UI_BATTERY_80,
                &UI_BATTERY_90,
                &UI_BATTERY_100,
            ],
            [
                &UI_BATTERY_0_CHARGING,
                &UI_BATTERY_10_CHARGING,
                &UI_BATTERY_20_CHARGING,
                &UI_BATTERY_30_CHARGING,
                &UI_BATTERY_40_CHARGING,
                &UI_BATTERY_50_CHARGING,
                &UI_BATTERY_60_CHARGING,
                &UI_BATTERY_70_CHARGING,
                &UI_BATTERY_80_CHARGING,
                &UI_BATTERY_90_CHARGING,
                &UI_BATTERY_100_CHARGING,
            ],
        )
    }

    // REMOVE: charging overlay — restore charging sprite selection here
    pub fn sprite_region(&self) -> &'static SpriteRegion {
        let (normal, _charging) = Self::sprite_arrays();
        let idx = (self.state.pct / 10).min(10) as usize;
        &normal[idx]
    }
    // END REMOVE: charging overlay

    pub fn slot_width(&self) -> f32 {
        24.0
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<BatteryWidget, AppState> {
    app::Module::new().on(|w: &mut BatteryWidget, ev: &BatteryUpdate| {
        if w.state.pct != ev.pct || w.state.charging != ev.charging {
            w.state.pct = ev.pct;
            w.state.charging = ev.charging;
            w.pct_text = format!("{}", ev.pct);
            return Some(BatteryChanged);
        }
        None
    })
}
