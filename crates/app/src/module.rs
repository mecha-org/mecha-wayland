use std::marker::PhantomData;

use frunk::{HCons, HNil};

use crate::dispatch::{HandleList, Handler, MountedModule, OuterDispatch, Propagate};
use crate::event::{Emit, Event};

/// A value that can be mounted onto an [`App`](crate::App).
///
/// Implemented automatically for every `Module` whose handler and sub-module
/// types satisfy the full dispatch requirements. You never implement this
/// manually; it exists so that factory functions can return
/// `-> impl RegisteredModule<S, AppState>` without naming closure types.
pub trait RegisteredModule<S, AppState> {
    type Emitted: Propagate<AppState>;
    type Handlers: HandleList<S, Self::Emitted>;
    type SubModules: OuterDispatch<S, AppState>;

    fn into_module(self) -> Module<S, Self::Emitted, Self::Handlers, Self::SubModules>;
}

impl<S, AppState, E, H, SM> RegisteredModule<S, AppState> for Module<S, E, H, SM>
where
    E: Propagate<AppState>,
    H: HandleList<S, E>,
    SM: OuterDispatch<S, AppState>,
{
    type Emitted = E;
    type Handlers = H;
    type SubModules = SM;

    fn into_module(self) -> Module<S, E, H, SM> {
        self
    }
}

/// A collection of event handlers that operate on state `S`.
///
/// A `Module` is built by chaining [`on`](Module::on) calls (one per event
/// type) and optionally nesting child modules with [`mount`](Module::mount).
/// Modules are attached to an [`App`](crate::App) or to another `Module` via
/// their respective `mount` methods.
///
/// # Type parameters
///
/// These are managed automatically as you chain builder calls — you never need
/// to write them by hand except when constructing a module without any handlers
/// (use `Module::<MyState, _, _>::new()`).
///
/// - `S` — the state slice this module owns
/// - `Emitted` — HList of event types the handlers may emit (inferred)
/// - `Handlers` — HList of registered handler closures (inferred)
/// - `SubModules` — HList of mounted child modules (inferred, defaults to `HNil`)
pub struct Module<S, Emitted, Handlers, SubModules = HNil> {
    pub(crate) handlers: Handlers,
    pub(crate) sub_modules: SubModules,
    _phantom: PhantomData<(S, Emitted)>,
}

impl<S> Module<S, (), HNil, HNil> {
    /// Create an empty module with no handlers and no sub-modules.
    pub fn new() -> Self {
        Self {
            handlers: HNil,
            sub_modules: HNil,
            _phantom: PhantomData,
        }
    }
}

impl<S, Emitted, Handlers, SubModules> Module<S, Emitted, Handlers, SubModules> {
    /// Register a handler for event `E`.
    ///
    /// `f` receives a mutable reference to this module's state and a shared
    /// reference to the event. Its return value controls what gets re-dispatched
    /// after the handler runs — see [`Emit`] for the full list of valid return
    /// types.
    ///
    /// Multiple handlers for the same event type are allowed; they all run in
    /// registration order.
    ///
    /// # Example
    ///
    /// ```rust
    /// use app::prelude::*;
    ///
    /// #[derive(Debug)] struct Clicked; impl Event for Clicked {}
    /// #[derive(Debug)] struct Hovered; impl Event for Hovered {}
    ///
    /// // Handler that emits nothing
    /// let m = Module::new().on(|count: &mut u32, _: &Clicked| *count += 1);
    ///
    /// // Handler that conditionally emits another event
    /// let m = Module::new().on(|s: &mut u32, _: &Hovered| -> Option<Clicked> {
    ///     if *s > 0 { Some(Clicked) } else { None }
    /// });
    /// ```
    pub fn on<E: Event, Ret: Emit>(
        self,
        f: impl Fn(&mut S, &E) -> Ret,
    ) -> Module<
        S,
        HCons<Ret::Output, Emitted>,
        HCons<Handler<S, E, Ret, impl Fn(&mut S, &E) -> Ret>, Handlers>,
        SubModules,
    > {
        Module {
            handlers: HCons {
                head: Handler {
                    f,
                    _phantom: PhantomData,
                },
                tail: self.handlers,
            },
            sub_modules: self.sub_modules,
            _phantom: PhantomData,
        }
    }

    /// Nest a child module whose state is a sub-field of `S`.
    ///
    /// `lens` is a closure that extracts `&mut ChildState` from `&mut S`. It
    /// must return a reference to a disjoint sub-field — overlapping lenses are
    /// undefined behaviour.
    ///
    /// Events emitted by the child propagate up to the application root and are
    /// re-dispatched across all modules there.
    ///
    /// # Example
    ///
    /// ```rust
    /// use app::prelude::*;
    ///
    /// #[derive(Debug)] struct Tick; impl Event for Tick {}
    ///
    /// struct Parent { child_val: u32 }
    ///
    /// let child = Module::new().on(|s: &mut u32, _: &Tick| *s += 1);
    /// let parent = Module::<Parent, _, _>::new()
    ///     .mount(|s: &mut Parent| &mut s.child_val, child);
    /// ```
    #[allow(private_bounds)]
    pub fn mount<ChildState, ChildEmitted, ChildHandlers, ChildSubModules, LensFn>(
        self,
        lens: LensFn,
        module: Module<ChildState, ChildEmitted, ChildHandlers, ChildSubModules>,
    ) -> Module<
        S,
        Emitted,
        Handlers,
        HCons<
            MountedModule<S, ChildState, ChildEmitted, ChildHandlers, LensFn, ChildSubModules>,
            SubModules,
        >,
    >
    where
        LensFn: Fn(&mut S) -> &mut ChildState,
        ChildHandlers: HandleList<ChildState, ChildEmitted>,
    {
        Module {
            handlers: self.handlers,
            sub_modules: HCons {
                head: MountedModule {
                    lens,
                    module,
                    _phantom: PhantomData,
                },
                tail: self.sub_modules,
            },
            _phantom: PhantomData,
        }
    }
}
