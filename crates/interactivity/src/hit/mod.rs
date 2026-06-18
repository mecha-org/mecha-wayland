use utils::Rect;

pub struct HitArea {
    pub id: u64,
    pub rect: Rect,
}

#[derive(Default)]
pub struct HitAreaRegistry {
    areas: Vec<HitArea>,
}

impl HitAreaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove all registered areas.
    pub fn clear(&mut self) {
        self.areas.clear();
    }

    /// Add a new area to the registry.
    pub fn push(&mut self, area: HitArea) {
        self.areas.push(area);
    }

    /// Return the `id` of the first area whose rect contains `(x, y)`, or
    /// `None` if the point is outside every registered area.
    pub fn hit_test(&self, x: f64, y: f64) -> Option<u64> {
        self.areas
            .iter()
            .find(|a| a.rect.contains(x, y))
            .map(|a| a.id)
    }

    /// Return an iterator over all areas whose rect contains `(x, y)`.
    ///
    /// Useful when areas overlap and you need to handle all matches (e.g. for
    /// a layered UI where multiple elements share the same screen region).
    pub fn hit_test_all<'a>(&'a self, x: f64, y: f64) -> impl Iterator<Item = u64> + 'a {
        self.areas
            .iter()
            .filter(move |a| a.rect.contains(x, y))
            .map(|a| a.id)
    }
}
