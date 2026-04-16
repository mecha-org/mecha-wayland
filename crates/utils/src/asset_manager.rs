use anyhow::Result;
use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Opaque identifier for a loaded asset.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct AssetId(usize);

/// A typed, lightweight reference to a loaded asset.
#[derive(Debug)]
pub struct Handle<A: Asset> {
    asset_id: AssetId,
    _phantom: std::marker::PhantomData<A>,
}

impl<A: Asset> Clone for Handle<A> {
    fn clone(&self) -> Self { *self }
}
impl<A: Asset> Copy for Handle<A> {}

impl<A: Asset> PartialEq for Handle<A> {
    fn eq(&self, other: &Self) -> bool { self.asset_id == other.asset_id }
}
impl<A: Asset> Eq for Handle<A> {}

impl<A: Asset> std::hash::Hash for Handle<A> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.asset_id.hash(state); }
}

impl<A: Asset> Handle<A> {
    fn new(asset_id: AssetId) -> Self {
        Self { asset_id, _phantom: std::marker::PhantomData }
    }

    pub fn id(&self) -> AssetId { self.asset_id }
}

/// A self-loading asset.
pub trait Asset: Sized + 'static {
    type Params: 'static;
    fn load(params: Self::Params) -> Result<Self>;
}

// ── Internal: per-type storage ─────────────────────────────────────────────

struct AssetStore<A: Asset> {
    storage: HashMap<AssetId, A>,
}

// ── Asset Manager ──────────────────────────────────────────────────────────

pub struct AssetManager {
    stores:  HashMap<TypeId, Box<dyn Any>>,
    next_id: usize,
}

impl AssetManager {
    pub fn new() -> Self {
        Self { stores: HashMap::new(), next_id: 0 }
    }

    fn next_id(&mut self) -> AssetId {
        let id = AssetId(self.next_id);
        self.next_id += 1;
        id
    }

    fn store_mut<A: Asset>(&mut self) -> &mut AssetStore<A> {
        self.stores
            .entry(TypeId::of::<A>())
            .or_insert_with(|| Box::new(AssetStore::<A> { storage: HashMap::new() }) as Box<dyn Any>)
            .downcast_mut::<AssetStore<A>>()
            .expect("AssetStore type mismatch")
    }

    fn store<A: Asset>(&self) -> Option<&AssetStore<A>> {
        self.stores.get(&TypeId::of::<A>())?.downcast_ref::<AssetStore<A>>()
    }

    pub fn load<A, P>(&mut self, params: P) -> Result<Handle<A>>
    where
        A: Asset,
        P: Into<A::Params>,
    {
        let asset = A::load(params.into())?;
        let id    = self.next_id();
        self.store_mut::<A>().storage.insert(id, asset);
        Ok(Handle::new(id))
    }

    pub fn insert<A: Asset>(&mut self, asset: A) -> Handle<A> {
        let id = self.next_id();
        self.store_mut::<A>().storage.insert(id, asset);
        Handle::new(id)
    }

    pub fn get<A: Asset>(&self, handle: &Handle<A>) -> Option<&A> {
        self.store::<A>()?.storage.get(&handle.id())
    }

    pub fn get_mut<A: Asset>(&mut self, handle: &Handle<A>) -> Option<&mut A> {
        self.store_mut::<A>().storage.get_mut(&handle.id())
    }

    pub fn remove<A: Asset>(&mut self, handle: &Handle<A>) -> Option<A> {
        self.store_mut::<A>().storage.remove(&handle.id())
    }

    pub fn count<A: Asset>(&self) -> usize {
        self.store::<A>().map_or(0, |s| s.storage.len())
    }
}

impl Default for AssetManager {
    fn default() -> Self { Self::new() }
}
