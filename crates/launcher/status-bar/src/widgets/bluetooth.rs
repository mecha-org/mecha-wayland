use super::{UI_BLUETOOTH_CONNECTED, UI_BLUETOOTH_ON};
use assets::SpriteRegion;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BluetoothState {
    #[default]
    Off,
    On,
    Connected,
}

#[derive(Debug)]
pub struct BluetoothChanged;
impl app::Event for BluetoothChanged {}

#[derive(Debug)]
pub struct BluetoothUpdate(pub BluetoothState);
impl app::Event for BluetoothUpdate {}

#[derive(Debug)]
pub struct BluetoothWidget {
    pub state: BluetoothState,
}

impl Default for BluetoothWidget {
    fn default() -> Self {
        Self {
            state: BluetoothState::default(),
        }
    }
}

impl BluetoothWidget {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn visible(&self) -> bool {
        self.state != BluetoothState::Off
    }

    pub fn sprite_region(&self) -> &'static SpriteRegion {
        match self.state {
            BluetoothState::Connected => &UI_BLUETOOTH_CONNECTED,
            _ => &UI_BLUETOOTH_ON,
        }
    }

    pub fn slot_width(&self) -> f32 {
        if self.visible() { 24.0 } else { 0.0 }
    }
}

pub fn module<AppState>() -> impl app::RegisteredModule<BluetoothWidget, AppState> {
    app::Module::new().on(|w: &mut BluetoothWidget, ev: &BluetoothUpdate| {
        if w.state != ev.0 {
            w.state = ev.0;
            return Some(BluetoothChanged);
        }
        None
    })
}
