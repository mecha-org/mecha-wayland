use frunk::{HCons, HNil};

/// Marker trait for types that can be dispatched as events.
///
/// Any `'static + Debug` type can implement `Event`. The `Debug` bound is
/// required for dispatch tracing.
///
/// # Example
///
/// ```rust
/// use app::Event;
///
/// #[derive(Debug)]
/// struct WindowResized { width: u32, height: u32 }
/// impl Event for WindowResized {}
/// ```
pub trait Event: 'static + std::fmt::Debug {}
impl Event for () {}


/// Wraps an iterator so a handler can emit zero or more events of the same type.
///
/// Use `Many` when the number of events to emit is only known at runtime. For a
/// statically-known set of different event types, use `hlist![…]` instead.
///
/// # Example
///
/// ```rust
/// use app::prelude::*;
///
/// #[derive(Debug)] struct Explode; impl Event for Explode {}
/// #[derive(Debug)] struct Fragment; impl Event for Fragment {}
///
/// let module = Module::new()
///     .on(|_: &mut u32, _: &Explode| Many(vec![Fragment, Fragment, Fragment]));
/// ```
pub struct Many<Iter>(pub Iter);

mod sealed {
    pub trait Sealed {}
}

/// Describes what a handler may return to emit subsequent events.
///
/// This is a sealed trait — you cannot implement it yourself. The following
/// return types are valid for a handler closure:
///
/// | Return type | Behaviour |
/// |-------------|-----------|
/// | `E` (any [`Event`]) | Always emits `E` |
/// | `Option<E>` | Emits `E` only when `Some` |
/// | [`Many<Iter>`] where `Iter::Item: Event` | Emits every item in the iterator |
/// | `hlist![T1, T2, …]` where each `Ti: Emit` | Emits each element independently |
/// | `()` | Emits nothing |
pub trait Emit: sealed::Sealed {
    type Output;
    fn emit(self) -> Self::Output;
    fn empty() -> Self::Output;
}

impl<E: Event> sealed::Sealed for E {}
impl<E: Event> Emit for E {
    type Output = Option<E>;
    fn emit(self) -> Option<E> { Some(self) }
    fn empty() -> Option<E> { None }
}

impl<E: Event> sealed::Sealed for Option<E> {}
impl<E: Event> Emit for Option<E> {
    type Output = Option<E>;
    fn emit(self) -> Option<E> { self }
    fn empty() -> Option<E> { None }
}

impl<Iter> sealed::Sealed for Many<Iter>
where
    Iter: IntoIterator,
    Iter::Item: Event,
{}

impl<Iter> Emit for Many<Iter>
where
    Iter: IntoIterator,
    Iter::Item: Event,
{
    type Output = Option<Many<Iter>>;
    fn emit(self) -> Option<Many<Iter>> { Some(self) }
    fn empty() -> Option<Many<Iter>> { None }
}

impl sealed::Sealed for HNil {}
impl Emit for HNil {
    type Output = Option<HNil>;
    fn emit(self) -> Option<HNil> { Some(HNil) }
    fn empty() -> Option<HNil> { None }
}

impl<H: Emit, T: Emit> sealed::Sealed for HCons<H, T> {}
impl<H: Emit, T: Emit> Emit for HCons<H, T> {
    type Output = Option<HCons<H, T>>;
    fn emit(self) -> Option<HCons<H, T>> { Some(self) }
    fn empty() -> Option<HCons<H, T>> { None }
}
