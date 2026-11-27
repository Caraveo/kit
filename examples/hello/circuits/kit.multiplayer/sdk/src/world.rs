//! Seed + diff worlds on the client: the scope's seed, the modifications this
//! client has for the chunks it watches, and the hashes that prove a cached
//! chunk is current (so rejoining costs 16 bytes per unchanged chunk).

use std::collections::HashMap;

use crate::protocol::{Value, WorldDescriptor, mod_hash, seed_hash};

#[derive(Clone, Debug, Default)]
pub struct WorldCache {
    pub seed: Option<WorldDescriptor>,
    /// The world's current hash as last reported by the scope.
    pub world_hash: u64,
    pub revision: u32,
    chunks: HashMap<(i16, i16), Chunk>,
}

#[derive(Clone, Debug, Default)]
pub struct Chunk {
    pub mods: HashMap<String, Value>,
    pub hash: u64,
}

impl WorldCache {
    pub fn set_seed(&mut self, seed: Option<WorldDescriptor>, world_hash: u64, revision: u32) {
        if self.seed != seed {
            // A new seed is a new world.
            self.chunks.clear();
        }
        self.seed = seed;
        self.world_hash = world_hash;
        self.revision = revision;
    }

    /// The hash this client holds for a chunk (0 if it has nothing cached).
    pub fn chunk_hash(&self, cx: i16, cz: i16) -> u64 {
        self.chunks.get(&(cx, cz)).map_or(0, |c| c.hash)
    }

    /// Apply one modification (idempotent: the same key and value twice is one change).
    pub fn apply(&mut self, cx: i16, cz: i16, key: &str, value: Value) {
        let chunk = self.chunks.entry((cx, cz)).or_default();
        if let Some(old) = chunk.mods.get(key) {
            chunk.hash = chunk.hash.wrapping_sub(mod_hash(cx, cz, key, old));
        }
        chunk.hash = chunk.hash.wrapping_add(mod_hash(cx, cz, key, &value));
        chunk.mods.insert(key.to_owned(), value);
    }

    /// The scope sent a chunk hash: forget our copy if it's stale (its mods follow).
    pub fn chunk_header(&mut self, cx: i16, cz: i16, hash: u64, count: u32) {
        if count > 0 || self.chunk_hash(cx, cz) != hash {
            self.chunks.remove(&(cx, cz));
        }
    }

    pub fn chunk(&self, cx: i16, cz: i16) -> Option<&Chunk> {
        self.chunks.get(&(cx, cz))
    }

    /// The world hash computed from what this client has. Equals the scope's
    /// `world_hash` once every modified chunk is cached.
    pub fn local_hash(&self) -> u64 {
        self.chunks.values().fold(seed_hash(self.seed.as_ref()), |h, c| h.wrapping_add(c.hash))
    }

    /// Chunk coordinates around a position within `radius` chunks.
    pub fn chunks_around(&self, x: f32, z: f32, radius: i32) -> Vec<(i16, i16)> {
        let size = self.seed.as_ref().map_or(32.0, |s| s.chunk_size as f32);
        let (cx, cz) = ((x / size).floor() as i32, (z / size).floor() as i32);
        let mut v = Vec::new();
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                v.push(((cx + dx).clamp(i16::MIN as i32, i16::MAX as i32) as i16, (cz + dz).clamp(i16::MIN as i32, i16::MAX as i32) as i16));
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_hash_matches_the_scope() {
        let seed = WorldDescriptor { id: "forest".into(), seed: 847291.0, generator: "kit.terrain".into(), generator_version: "1".into(), chunk_size: 32, compatibility_hash: String::new() };
        let mut w = WorldCache::default();
        w.set_seed(Some(seed), 0, 1);
        w.apply(10, 12, "tree:42", Value::Str("removed".into()));
        w.apply(10, 12, "tree:42", Value::Str("removed".into()));
        assert_eq!(w.local_hash(), 0xe928_26ef_6662_ffb2, "same number the Worker computes");
        assert_eq!(w.chunk_hash(10, 12), 0xa898_0eaa_b5bc_d69b);
        w.chunk_header(10, 12, 0xa898_0eaa_b5bc_d69b, 0);
        assert!(w.chunk(10, 12).is_some(), "an up-to-date chunk stays cached");
        w.chunk_header(10, 12, 1, 3);
        assert!(w.chunk(10, 12).is_none(), "a stale chunk is dropped before its replay");
        assert_eq!(w.chunks_around(40.0, -1.0, 1).len(), 9);
    }
}
