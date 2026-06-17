use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

pub struct CacheId<T> {
    pub raw: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> CacheId<T> {
    pub fn new(raw: u32) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }
}

impl<T> Clone for CacheId<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for CacheId<T> {}
impl<T> PartialEq for CacheId<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl<T> Eq for CacheId<T> {}
impl<T> PartialOrd for CacheId<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for CacheId<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}
impl<T> Hash for CacheId<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}
impl<T> fmt::Debug for CacheId<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.raw)
    }
}

/// A dense set of `CacheId<T>` keys.
#[derive(Debug, Clone)]
pub struct IdSet<T> {
    inner: BTreeSet<u32>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Default for IdSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> IdSet<T> {
    pub fn new() -> Self {
        Self {
            inner: BTreeSet::new(),
            _marker: PhantomData,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn insert(&mut self, id: CacheId<T>) -> bool {
        self.inner.insert(id.raw)
    }
    pub fn remove(&mut self, id: CacheId<T>) -> bool {
        self.inner.remove(&id.raw)
    }
    pub fn contains(&self, id: CacheId<T>) -> bool {
        self.inner.contains(&id.raw)
    }
    pub fn iter(&self) -> impl Iterator<Item = CacheId<T>> + '_ {
        self.inner.iter().copied().map(CacheId::new)
    }
    pub fn union_with(&mut self, other: &Self) {
        for r in &other.inner {
            self.inner.insert(*r);
        }
    }
}

/// A dense map keyed by `CacheId<T>`.
#[derive(Debug, Clone)]
pub struct IdMap<T, V> {
    inner: BTreeMap<u32, V>,
    _marker: PhantomData<fn() -> T>,
}

impl<T, V> Default for IdMap<T, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, V> IdMap<T, V> {
    pub fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
            _marker: PhantomData,
        }
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn get(&self, id: CacheId<T>) -> Option<&V> {
        self.inner.get(&id.raw)
    }
    pub fn get_mut(&mut self, id: CacheId<T>) -> Option<&mut V> {
        self.inner.get_mut(&id.raw)
    }
    pub fn insert(&mut self, id: CacheId<T>, v: V) -> Option<V> {
        self.inner.insert(id.raw, v)
    }
    pub fn remove(&mut self, id: CacheId<T>) -> Option<V> {
        self.inner.remove(&id.raw)
    }
    pub fn iter(&self) -> impl Iterator<Item = (CacheId<T>, &V)> + '_ {
        self.inner.iter().map(|(k, v)| (CacheId::new(*k), v))
    }
    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.inner.values()
    }
    pub fn next_id(&self) -> CacheId<T> {
        let next = self.inner.keys().next_back().copied().map_or(0, |k| k + 1);
        CacheId::new(next)
    }
}
