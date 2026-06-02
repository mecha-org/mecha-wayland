use std::marker::PhantomData;

use frunk::{HCons, HNil};

use crate::dispatch::{ModuleList, MountedModule};
use crate::event::Event;
use crate::module::{Lens, RegisteredModule};

/// The top-level application runtime.
///
/// `App` owns the root state `S` and a list of [`Module`]s attached via
/// [`mount`](App::mount). Call [`dispatch`](App::dispatch) to broadcast an
/// event; every registered handler whose event type matches will run, and any
/// events they emit are re-dispatched automatically.
///
/// # Example
///
/// ```rust
/// use app::prelude::*;
///
/// #[derive(Debug)] struct Reset; impl Event for Reset {}
///
/// let module = Module::new().on(|s: &mut u32, _: &Reset| *s = 0);
///
/// let mut app = App::new(42u32).mount(module);
/// app.dispatch(&Reset);
/// assert_eq!(*app.state(), 0);
/// ```
pub struct App<S, Modules> {
    pub(crate) state: S,
    pub(crate) modules: Modules,
}

impl<S> App<S, HNil> {
    /// Create a new `App` with the given initial state and no modules.
    pub fn new(state: S) -> Self {
        Self {
            state,
            modules: HNil,
        }
    }
}

impl<S, Modules> App<S, Modules> {
    /// Return a shared reference to the application state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Attach a module to the app.
    ///
    /// `S` must implement [`Lens<SubState>`](crate::Lens) to tell the runtime
    /// how to extract the module's state slice from the root state. Use the
    /// blanket identity impl when the module owns the whole state, or write a
    /// manual impl per field otherwise.
    ///
    /// Mount order matters for event propagation: emitted events are
    /// re-dispatched across **all** modules from the beginning of the list,
    /// so modules mounted earlier will see events emitted by later ones.
    /// Mount shared consumers (e.g. a notification queue) before producers.
    #[allow(private_bounds)]
    pub fn mount<SubState, M>(
        self,
        module: M,
    ) -> App<
        S,
        HCons<MountedModule<S, SubState, M::Emitted, M::Handlers, M::SubModules>, Modules>,
    >
    where
        S: Lens<SubState>,
        M: RegisteredModule<SubState, S>,
        HCons<MountedModule<S, SubState, M::Emitted, M::Handlers, M::SubModules>, Modules>:
            ModuleList<S>,
    {
        let module = module.into_module();
        App {
            state: self.state,
            modules: HCons {
                head: MountedModule {
                    module,
                    _phantom: PhantomData::<(S, M::Emitted)>,
                },
                tail: self.modules,
            },
        }
    }

    /// Dispatch an event to all mounted modules.
    ///
    /// Each handler registered for `E` runs in mount order. If a handler
    /// emits events, they are re-dispatched immediately (depth-first) before
    /// the next handler in the current pass runs.
    ///
    /// Infinite emit cycles (a handler always emitting the same event) will
    /// overflow the stack.
    #[inline(always)]
    #[allow(private_bounds)]
    pub fn dispatch<E: Event>(&mut self, event: &E)
    where
        Modules: ModuleList<S>,
    {
        let modules = &self.modules;
        modules.dispatch(event, &mut self.state);
    }
}
