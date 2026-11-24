//! # kit_ecs
//!
//! The entity-component-system core of Kit.
//!
//! * [`Entity`] — a generational handle (index + generation), safe against reuse.
//! * Components — any `'static` type, stored per type in a [`SparseSet`].
//! * Resources — singleton values stored by type (time, input, renderer settings...).
//!
//! Storages and resources live behind `RefCell`s so systems can borrow several
//! different component types at once through a shared `&World`, while structural
//! changes (spawn, despawn, insert, remove) need `&mut World`.

mod sparse;

pub use sparse::SparseSet;

use std::any::{Any, TypeId, type_name};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::fmt;

/// A generational entity handle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    /// Slot index of this entity (stable while it is alive).
    pub fn index(self) -> u32 {
        self.index
    }
    /// Generation of the slot; bumped every time the slot is reused.
    pub fn generation(self) -> u32 {
        self.generation
    }
    /// Pack into a single `u64` (generation in the high bits).
    pub fn to_bits(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }
    /// Inverse of [`Entity::to_bits`].
    pub fn from_bits(bits: u64) -> Self {
        Self { index: bits as u32, generation: (bits >> 32) as u32 }
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}v{}", self.index, self.generation)
    }
}

/// Marker trait for component types. Blanket-implemented for every `'static` type.
pub trait Component: 'static {}
impl<T: 'static> Component for T {}

/// Type-erased access to a component storage.
trait AnyStorage: Any {
    fn remove_entity(&self, e: Entity);
    fn contains(&self, e: Entity) -> bool;
    fn len(&self) -> usize;
    fn entities(&self) -> Vec<Entity>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: Component> AnyStorage for RefCell<SparseSet<T>> {
    fn remove_entity(&self, e: Entity) {
        self.borrow_mut().remove(e);
    }
    fn contains(&self, e: Entity) -> bool {
        self.borrow().contains(e)
    }
    fn len(&self) -> usize {
        self.borrow().len()
    }
    fn entities(&self) -> Vec<Entity> {
        self.borrow().entities().to_vec()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The ECS world: entities, their components, and global resources.
#[derive(Default)]
pub struct World {
    generations: Vec<u32>,
    alive: Vec<bool>,
    free: Vec<u32>,
    live_count: usize,
    storages: HashMap<TypeId, Box<dyn AnyStorage>>,
    resources: HashMap<TypeId, Box<dyn Any>>,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    // ---------------------------------------------------------------- entities

    /// Create a new, empty entity.
    pub fn spawn(&mut self) -> Entity {
        self.live_count += 1;
        if let Some(index) = self.free.pop() {
            let i = index as usize;
            self.alive[i] = true;
            Entity { index, generation: self.generations[i] }
        } else {
            let index = self.generations.len() as u32;
            self.generations.push(0);
            self.alive.push(true);
            Entity { index, generation: 0 }
        }
    }

    /// Destroy an entity and all its components. Returns `false` if it was already dead.
    pub fn despawn(&mut self, e: Entity) -> bool {
        if !self.is_alive(e) {
            return false;
        }
        for storage in self.storages.values() {
            storage.remove_entity(e);
        }
        let i = e.index as usize;
        self.alive[i] = false;
        self.generations[i] = self.generations[i].wrapping_add(1);
        self.free.push(e.index);
        self.live_count -= 1;
        true
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        let i = e.index as usize;
        i < self.alive.len() && self.alive[i] && self.generations[i] == e.generation
    }

    /// Number of live entities.
    pub fn entity_count(&self) -> usize {
        self.live_count
    }

    /// All live entities, in slot order.
    pub fn entities(&self) -> Vec<Entity> {
        self.alive
            .iter()
            .enumerate()
            .filter(|(_, a)| **a)
            .map(|(i, _)| Entity { index: i as u32, generation: self.generations[i] })
            .collect()
    }

    /// Remove every entity (resources are kept).
    pub fn clear_entities(&mut self) {
        for e in self.entities() {
            self.despawn(e);
        }
    }

    // -------------------------------------------------------------- components

    fn storage_cell<T: Component>(&self) -> Option<&RefCell<SparseSet<T>>> {
        self.storages
            .get(&TypeId::of::<T>())
            .map(|s| s.as_any().downcast_ref::<RefCell<SparseSet<T>>>().expect("storage type"))
    }

    fn storage_cell_or_insert<T: Component>(&mut self) -> &mut RefCell<SparseSet<T>> {
        self.storages
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(RefCell::new(SparseSet::<T>::new())))
            .as_any_mut()
            .downcast_mut::<RefCell<SparseSet<T>>>()
            .expect("storage type")
    }

    /// Attach (or replace) a component. Panics if the entity is dead.
    pub fn insert<T: Component>(&mut self, e: Entity, component: T) -> Option<T> {
        assert!(self.is_alive(e), "insert {} on dead entity {e:?}", type_name::<T>());
        self.storage_cell_or_insert::<T>().get_mut().insert(e, component)
    }

    /// Detach a component, returning it.
    pub fn remove<T: Component>(&mut self, e: Entity) -> Option<T> {
        self.storages
            .get_mut(&TypeId::of::<T>())
            .and_then(|s| s.as_any_mut().downcast_mut::<RefCell<SparseSet<T>>>().unwrap().get_mut().remove(e))
    }

    pub fn has<T: Component>(&self, e: Entity) -> bool {
        self.storage_cell::<T>().is_some_and(|s| s.borrow().contains(e))
    }

    /// Borrow one component immutably.
    pub fn get<T: Component>(&self, e: Entity) -> Option<Ref<'_, T>> {
        let cell = self.storage_cell::<T>()?;
        Ref::filter_map(cell.borrow(), |s| s.get(e)).ok()
    }

    /// Borrow one component mutably.
    pub fn get_mut<T: Component>(&self, e: Entity) -> Option<RefMut<'_, T>> {
        let cell = self.storage_cell::<T>()?;
        RefMut::filter_map(cell.borrow_mut(), |s| s.get_mut(e)).ok()
    }

    /// Clone a component out (convenience for small `Clone` components).
    pub fn get_cloned<T: Component + Clone>(&self, e: Entity) -> Option<T> {
        self.get::<T>(e).map(|c| c.clone())
    }

    /// Borrow the whole storage of `T`, if any `T` was ever inserted.
    pub fn storage<T: Component>(&self) -> Option<Ref<'_, SparseSet<T>>> {
        self.storage_cell::<T>().map(|c| c.borrow())
    }

    /// Borrow the whole storage of `T` mutably. Creates it if needed.
    pub fn storage_mut<T: Component>(&mut self) -> RefMut<'_, SparseSet<T>> {
        self.storage_cell_or_insert::<T>().borrow_mut()
    }

    /// Borrow the storage of `T` mutably through a shared world reference.
    /// Returns `None` if no `T` was ever inserted.
    pub fn try_storage_mut<T: Component>(&self) -> Option<RefMut<'_, SparseSet<T>>> {
        self.storage_cell::<T>().map(|c| c.borrow_mut())
    }

    /// Entities that have every component type in `Q` (a tuple of up to 6 types).
    ///
    /// ```
    /// # use kit_ecs::World;
    /// let mut w = World::new();
    /// let e = w.spawn();
    /// w.insert(e, 1u32);
    /// w.insert(e, 2.0f32);
    /// assert_eq!(w.query::<(u32, f32)>(), vec![e]);
    /// ```
    pub fn query<Q: QueryTypes>(&self) -> Vec<Entity> {
        Q::matching(self)
    }

    // --------------------------------------------------------------- resources

    pub fn insert_resource<R: 'static>(&mut self, r: R) {
        self.resources.insert(TypeId::of::<R>(), Box::new(RefCell::new(r)));
    }

    pub fn remove_resource<R: 'static>(&mut self) -> Option<R> {
        self.resources
            .remove(&TypeId::of::<R>())
            .map(|b| b.downcast::<RefCell<R>>().expect("resource type").into_inner())
    }

    pub fn has_resource<R: 'static>(&self) -> bool {
        self.resources.contains_key(&TypeId::of::<R>())
    }

    fn resource_cell<R: 'static>(&self) -> Option<&RefCell<R>> {
        self.resources.get(&TypeId::of::<R>()).map(|b| b.downcast_ref::<RefCell<R>>().unwrap())
    }

    /// Borrow a resource. Panics with the type name if it is missing.
    pub fn resource<R: 'static>(&self) -> Ref<'_, R> {
        self.try_resource::<R>().unwrap_or_else(|| panic!("missing resource {}", type_name::<R>()))
    }

    pub fn resource_mut<R: 'static>(&self) -> RefMut<'_, R> {
        self.try_resource_mut::<R>().unwrap_or_else(|| panic!("missing resource {}", type_name::<R>()))
    }

    pub fn try_resource<R: 'static>(&self) -> Option<Ref<'_, R>> {
        self.resource_cell::<R>().map(|c| c.borrow())
    }

    pub fn try_resource_mut<R: 'static>(&self) -> Option<RefMut<'_, R>> {
        self.resource_cell::<R>().map(|c| c.borrow_mut())
    }

    /// Get a resource, inserting `R::default()` first if missing.
    pub fn resource_or_default<R: Default + 'static>(&mut self) -> RefMut<'_, R> {
        if !self.has_resource::<R>() {
            self.insert_resource(R::default());
        }
        self.resource_mut::<R>()
    }
}

/// Tuples of component types usable with [`World::query`].
pub trait QueryTypes {
    fn matching(world: &World) -> Vec<Entity>;
}

macro_rules! impl_query {
    ($first:ident $(, $rest:ident)*) => {
        impl<$first: Component $(, $rest: Component)*> QueryTypes for ($first, $($rest,)*) {
            fn matching(world: &World) -> Vec<Entity> {
                let Some(first) = world.storages.get(&TypeId::of::<$first>()) else {
                    return Vec::new();
                };
                // Iterate the smallest storage for speed.
                #[allow(unused_mut)]
                let mut smallest: &dyn AnyStorage = first.as_ref();
                $(
                    match world.storages.get(&TypeId::of::<$rest>()) {
                        Some(s) => if s.len() < smallest.len() { smallest = s.as_ref(); },
                        None => return Vec::new(),
                    }
                )*
                let mut out = smallest.entities();
                out.retain(|e| {
                    first.contains(*e) $(&& world.storages[&TypeId::of::<$rest>()].contains(*e))*
                });
                out.sort_unstable();
                out
            }
        }
    };
}

impl_query!(A);
impl_query!(A, B);
impl_query!(A, B, C);
impl_query!(A, B, C, D);
impl_query!(A, B, C, D, E);
impl_query!(A, B, C, D, E, F);

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Clone)]
    struct Pos(f32);
    #[derive(Debug, PartialEq)]
    struct Vel(f32);

    #[test]
    fn spawn_insert_get() {
        let mut w = World::new();
        let a = w.spawn();
        let b = w.spawn();
        w.insert(a, Pos(1.0));
        w.insert(b, Pos(2.0));
        w.insert(b, Vel(3.0));
        assert_eq!(*w.get::<Pos>(a).unwrap(), Pos(1.0));
        assert!(w.get::<Vel>(a).is_none());
        assert_eq!(w.query::<(Pos,)>(), vec![a, b]);
        assert_eq!(w.query::<(Pos, Vel)>(), vec![b]);
        assert_eq!(w.query::<(Vel, Pos)>(), vec![b]);
    }

    #[test]
    fn despawn_and_generations() {
        let mut w = World::new();
        let a = w.spawn();
        w.insert(a, Pos(1.0));
        assert!(w.despawn(a));
        assert!(!w.is_alive(a));
        assert!(!w.despawn(a));
        let b = w.spawn();
        assert_eq!(b.index(), a.index());
        assert_ne!(b.generation(), a.generation());
        assert!(w.get::<Pos>(b).is_none());
        assert!(w.get::<Pos>(a).is_none());
        assert_eq!(w.entity_count(), 1);
    }

    #[test]
    fn simultaneous_borrows_of_different_types() {
        let mut w = World::new();
        let e = w.spawn();
        w.insert(e, Pos(1.0));
        w.insert(e, Vel(2.0));
        {
            let v = w.get::<Vel>(e).unwrap();
            let mut p = w.get_mut::<Pos>(e).unwrap();
            p.0 += v.0;
        }
        assert_eq!(w.get_cloned::<Pos>(e), Some(Pos(3.0)));
    }

    #[test]
    fn resources() {
        let mut w = World::new();
        w.insert_resource(5u64);
        *w.resource_mut::<u64>() += 1;
        assert_eq!(*w.resource::<u64>(), 6);
        assert_eq!(w.remove_resource::<u64>(), Some(6));
        assert!(!w.has_resource::<u64>());
    }

    #[test]
    fn empty_storage_reads() {
        let w = World::new();
        assert!(w.storage::<Pos>().is_none());
        assert!(w.query::<(Pos,)>().is_empty());
    }

    #[test]
    fn entity_bits_roundtrip() {
        let mut w = World::new();
        let e = w.spawn();
        assert_eq!(Entity::from_bits(e.to_bits()), e);
    }
}
