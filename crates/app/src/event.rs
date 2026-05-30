use frunk::{HCons, HNil};

pub trait Event: 'static {}
impl Event for () {}

pub struct Many<Iter>(pub Iter);

mod sealed {
    pub trait Sealed {}
}

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
