//! `ext_session_lock_v1`: the session lock and its surfaces.
//!
//! - [`SessionLock::lock`] asks the compositor to lock the session.
//!   Windows spawned with [`Role::Lock`](crate::Role::Lock) after it become
//!   the lock surfaces, one per output; the compositor configures each to
//!   its output's size as it is made (no initial commit: committing before
//!   the ack is a protocol error) and sends `locked` once every output
//!   shows one. [`SessionLocked`] follows.
//! - [`SessionLock::unlock`] ends the lock: `unlock_and_destroy` once
//!   `locked` arrived. Asked before then, the unlock waits for the answer
//!   (destroying the lock is a protocol error once `locked` was sent, and
//!   it may be in flight): `locked` is answered with `unlock_and_destroy`
//!   at once, `finished` with `destroy`. Neither is reported. Despawn the
//!   lock windows after the unlock: a lock surface destroyed while locked
//!   leaves its output a solid colour.
//! - `finished` without an unlock asked for means the compositor will not
//!   lock, or no longer does, for this client: [`SessionLockFinished`].
//!   The lock is destroyed; despawn the lock windows.

use app::prelude::*;
use wayland::prelude::*;

/// Where the session lock stands for this client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LockState {
    #[default]
    Unlocked,
    /// Asked for; the compositor has not confirmed it. An unlock asked for
    /// meanwhile waits for the answer.
    Locking,
    /// The compositor confirmed it: every output shows a lock surface.
    Locked,
}

/// The compositor confirmed the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLocked;
impl Signal for SessionLocked {}

/// The compositor ended the lock, or refused it, without an unlock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLockFinished;
impl Signal for SessionLockFinished {}

/// The session lock, if the compositor offers `ext_session_lock_manager_v1`.
pub struct SessionLock {
    manager: Option<ExtSessionLockManagerV1>,
    state: State,
}
impl Resource for SessionLock {}

enum State {
    Unlocked,
    Locking {
        lock: ExtSessionLockV1,
        unlock: bool,
    },
    Locked(ExtSessionLockV1),
}

impl SessionLock {
    /// Whether the compositor offers `ext_session_lock_manager_v1`.
    pub fn available(&self) -> bool {
        self.manager.is_some()
    }

    pub fn state(&self) -> LockState {
        match self.state {
            State::Unlocked => LockState::Unlocked,
            State::Locking { .. } => LockState::Locking,
            State::Locked(_) => LockState::Locked,
        }
    }

    /// Ask the compositor to lock the session, then spawn a
    /// [`Role::Lock`](crate::Role::Lock) window per output. False, and
    /// nothing sent, without the manager or with a lock already held or
    /// asked for (an unlock still waiting for the compositor's answer
    /// counts).
    pub fn lock(&mut self, wl: &mut Wayland) -> bool {
        let (Some(manager), State::Unlocked) = (self.manager, &self.state) else {
            return false;
        };
        self.state = State::Locking {
            lock: manager.lock(wl),
            unlock: false,
        };
        true
    }

    /// End the lock. Despawn the lock windows after it. Nothing to end
    /// does nothing.
    pub fn unlock(&mut self, wl: &mut Wayland) {
        match &mut self.state {
            State::Unlocked => {}
            State::Locking { unlock, .. } => *unlock = true,
            State::Locked(lock) => {
                lock.unlock_and_destroy(wl);
                self.state = State::Unlocked;
            }
        }
    }

    /// The lock a [`Role::Lock`](crate::Role::Lock) window's surface is
    /// made with.
    pub(crate) fn object(&self) -> Option<ExtSessionLockV1> {
        match self.state {
            State::Unlocked => None,
            State::Locking { lock, .. } | State::Locked(lock) => Some(lock),
        }
    }
}

pub(crate) fn install(app: &mut App) {
    let manager = {
        let (globals, mut wl) = app.query::<(Res<Globals>, ResMut<Wayland>)>();
        globals.bind_if_offered(&mut wl, 1)
    };
    app.insert_resource(SessionLock {
        manager,
        state: State::Unlocked,
    });
    app.system(on_session_lock);
}

fn on_session_lock(app: &mut App, e: &ExtSessionLockV1Event) {
    let (lock, locked) = match e {
        ExtSessionLockV1Event::Locked { session_lock } => (*session_lock, true),
        ExtSessionLockV1Event::Finished { session_lock } => (*session_lock, false),
    };
    let unlock_asked = {
        let (mut session, mut wl) = app.query::<(ResMut<SessionLock>, ResMut<Wayland>)>();
        if session.object() != Some(lock) {
            return;
        }
        let unlock_asked = matches!(session.state, State::Locking { unlock: true, .. });
        session.state = State::Unlocked;
        match (locked, unlock_asked) {
            (true, false) => session.state = State::Locked(lock),
            (true, true) => lock.unlock_and_destroy(&mut wl),
            (false, _) => lock.destroy(&mut wl),
        }
        unlock_asked
    };
    // An unlock asked for while locking is not reported either way.
    match (locked, unlock_asked) {
        (_, true) => {}
        (true, false) => app.signal(SessionLocked),
        (false, false) => app.signal(SessionLockFinished),
    }
}
