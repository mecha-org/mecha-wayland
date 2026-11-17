//! `xdg_activation_v1`: tokens that let a window we start take focus.
//!
//! A launcher asks for a token before starting an app and hands it to the
//! app (`XDG_ACTIVATION_TOKEN`, or D-Bus activation's platform data). The
//! app passes it back to the compositor with its first window, which may
//! then be focused.

use app::prelude::*;
use wayland::prelude::*;

use crate::Toplevels;

/// A token asked for with [`Toplevels::request_activation_token`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TokenRequest(XdgActivationTokenV1);

/// The compositor's answer to a [`TokenRequest`].
#[derive(Clone, Debug)]
pub struct ActivationToken {
    pub request: TokenRequest,
    pub token: String,
}
impl Signal for ActivationToken {}

impl Toplevels {
    /// Whether the compositor offers `xdg_activation_v1`.
    pub fn activation_available(&self) -> bool {
        self.activation.is_some()
    }

    /// Ask for a token for starting the app `app_id`. Its answer arrives
    /// as an [`ActivationToken`] for the returned request. `None` when
    /// the compositor doesn't offer `xdg_activation_v1`.
    pub fn request_activation_token(&self, wl: &mut Wayland, app_id: &str) -> Option<TokenRequest> {
        let activation = self.activation?;
        let token = activation.get_activation_token(wl);
        token.set_app_id(wl, app_id);
        token.commit(wl);
        Some(TokenRequest(token))
    }
}

/// A token is ready: its object is done with, and the answer goes out.
pub(crate) fn on_token(app: &mut App, e: &XdgActivationTokenV1Event) {
    match e {
        XdgActivationTokenV1Event::Done {
            activation_token,
            token,
        } => {
            activation_token.destroy(&mut app.resource_mut::<Wayland>());
            app.signal(ActivationToken {
                request: TokenRequest(*activation_token),
                token: token.clone(),
            });
        }
    }
}
