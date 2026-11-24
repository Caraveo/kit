use crate::Entity;

/// Sparse-set storage: O(1) insert/remove/lookup, dense contiguous iteration.
pub struct SparseSet<T> {
    /// entity index -> position in `dense`
    sparse: Vec<Option<u32>>,
    dense: Vec<Entity>,
    data: Vec<T>,
}

impl<T> Default for SparseSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SparseSet<T> {
    pub fn new() -> Self {
        Self { sparse: Vec::new(), dense: Vec::new(), data: Vec::new() }
    }

    fn slot(&self, e: Entity) -> Option<usize> {
        let d = (*self.sparse.get(e.index() as usize)?)? as usize;
        (self.dense[d] == e).then_some(d)
    }

    pub fn contains(&self, e: Entity) -> bool {
        self.slot(e).is_some()
    }

    pub fn insert(&mut self, e: Entity, value: T) -> Option<T> {
        let i = e.index() as usize;
        if i >= self.sparse.len() {
            self.sparse.resize(i + 1, None);
        }
        if let Some(d) = self.sparse[i].map(|d| d as usize) {
            if self.dense[d] == e {
                return Some(std::mem::replace(&mut self.data[d], value));
            }
            // Stale slot from a previous generation: overwrite in place.
            self.dense[d] = e;
            self.data[d] = value;
            return None;
        }
        self.sparse[i] = Some(self.dense.len() as u32);
        self.dense.push(e);
        self.data.push(value);
        None
    }

    pub fn remove(&mut self, e: Entity) -> Option<T> {
        let d = self.slot(e)?;
        let last = self.dense.len() - 1;
        self.dense.swap(d, last);
        self.data.swap(d, last);
        let moved = self.dense[d];
        self.sparse[moved.index() as usize] = Some(d as u32);
        self.sparse[e.index() as usize] = None;
        self.dense.pop();
        self.data.pop()
    }

    pub fn get(&self, e: Entity) -> Option<&T> {
        self.slot(e).map(|d| &self.data[d])
    }

    pub fn get_mut(&mut self, e: Entity) -> Option<&mut T> {
        self.slot(e).map(|d| &mut self.data[d])
    }

    pub fn len(&self) -> usize {
        self.dense.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }

    pub fn entities(&self) -> &[Entity] {
        &self.dense
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.dense.iter().copied().zip(self.data.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.dense.iter().copied().zip(self.data.iter_mut())
    }
}
