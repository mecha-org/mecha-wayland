use super::{UI_WIFI_HIGH, UI_WIFI_LOW, UI_WIFI_MEDIUM, UI_WIFI_NONE, UI_WIFI_X};
use assets::SpriteRegion;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WifiState {
    High,
    Medium,
    Low,
    None,
    X,
}

#[derive(Debug)]
pub struct WifiChanged;
impl app::Event for WifiChanged {}

#[derive(Debug)]
pub struct WifiUpdate(pub WifiState);
impl app::Event for WifiUpdate {}

#[derive(Debug)]
pub struct WifiWidget {
    pub state: WifiState,
}

impl WifiWidget {
    pub fn new() -> Self {
        Self {
            state: WifiState::High,
        }
    }

    pub fn sprite_region(&self) -> &'static SpriteRegion {
        match self.state {
            WifiState::High => &UI_WIFI_HIGH,
            WifiState::Medium => &UI_WIFI_MEDIUM,
            WifiState::Low => &UI_WIFI_LOW,
            WifiState::None => &UI_WIFI_NONE,
            WifiState::X => &UI_WIFI_X,
        }
    }

    pub fn slot_width(&self) -> f32 {
        24.0
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<WifiWidget, AppState> {
    app::Module::new().on(|w: &mut WifiWidget, ev: &WifiUpdate| {
        if w.state != ev.0 {
            w.state = ev.0;
            return Some(WifiChanged);
        }
        None
    })
}
