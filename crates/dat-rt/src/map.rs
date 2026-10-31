//! `map<K, V>` (`reference/16-map-methods.md`) and the ordered hash table
//! under it and [`crate::Set`].
//!
//! Entries live in a `Vec` of slots in insertion order. The index, a
//! hashbrown `HashTable`, holds each entry's slot number and hash. `delete`
//! leaves a hole, and a cursor marks the first live slot, so a map used as a
//! queue (delete the oldest, insert new ones) stays linear. Setting an
//! existing key keeps its place; a key added again after `delete` goes last.
//!
//! Hashing uses foldhash with one random seed per process. Iteration follows
//! insertion order, so the seed never shows in a program's output.

use crate::{DatError, DatHash, Equal, ErrorKind, List, Loc, Placeholder, R, Shared, Show, throw};
use foldhash::SharedSeed;
use foldhash::fast::FoldHasher;
use hashbrown::HashTable;
use hashbrown::hash_table::Entry;
use std::cell::Cell;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::sync::OnceLock;

/// Holes are compacted away once they outnumber this fraction of the live
/// entries, as (numerator, denominator). Holes slow iteration down, and a
/// compaction costs about two loops over the map; a dat benchmark will tune
/// the ratio.
const MAX_HOLES_PER_LIVE: (usize, usize) = (1, 4);

fn seeds() -> &'static (u64, SharedSeed) {
    static SEEDS: OnceLock<(u64, SharedSeed)> = OnceLock::new();
    SEEDS.get_or_init(|| {
        let random = std::collections::hash_map::RandomState::new();
        (random.hash_one(0u8), SharedSeed::from_u64(random.hash_one(1u8)))
    })
}

/// The hash a set or map files `v` under.
#[inline]
pub(crate) fn hash_of<T: DatHash + ?Sized>(v: &T) -> u64 {
    let (per_hasher, shared) = seeds();
    let mut h = FoldHasher::with_seed(*per_hasher, shared);
    v.dat_hash(&mut h);
    h.finish()
}

#[inline(always)]
fn or_throw<T>(r: R<T>, loc: &'static Loc) -> T {
    match r {
        Ok(v) => v,
        Err(e) => throw(e, loc),
    }
}

#[cold]
#[inline(never)]
fn key_error<K: Show>(key: &K) -> Box<DatError> {
    Box::new(DatError::new(ErrorKind::KeyError, format!("key {} not found", crate::debug(key))))
}

/// An entry's place in the index.
#[derive(Clone, Copy)]
struct Ix {
    slot: usize,
    hash: u64,
}

#[derive(Clone)]
pub(crate) struct Table<K, V> {
    index: HashTable<Ix>,
    /// Entries in insertion order; `None` is a hole left by `delete`.
    slots: Vec<Option<(K, V)>>,
    /// Every slot before this one is a hole.
    first: usize,
    /// The number of entries.
    live: usize,
    /// The slot the last lookup found, or [`Map::take`] took a value from,
    /// so the `insert` that follows it finds the entry without hashing:
    /// `m[k] = m.get(k, 0) + 1`, or the put-back of a take-and-restore.
    /// Only a hint: `insert` checks that the slot holds its key.
    last: Cell<Option<usize>>,
}

impl<K, V> crate::copies::Size for Table<K, V> {
    fn size(&self) -> u64 {
        self.live as u64
    }
}

impl<K, V> Table<K, V> {
    fn new() -> Table<K, V> {
        Table { index: HashTable::new(), slots: Vec::new(), first: 0, live: 0, last: Cell::new(None) }
    }

    fn iter(&self) -> Iter<'_, K, V> {
        Iter { slots: self.slots[self.first..].iter(), left: self.live }
    }

    /// The slot of `key`.
    #[inline]
    fn find(&self, hash: u64, key: &K) -> Option<usize>
    where
        K: Equal,
    {
        let slots = &self.slots;
        let found =
            self.index.find(hash, |ix| ix.hash == hash && matches!(&slots[ix.slot], Some((k, _)) if k.equal(key))).map(|ix| ix.slot);
        self.last.set(found);
        found
    }

    #[inline]
    fn value_mut(&mut self, slot: usize) -> &mut V {
        match &mut self.slots[slot] {
            Some((_, v)) => v,
            None => crate::unreachable(),
        }
    }

    /// Sets the value of `key`, which keeps its place if it's present.
    /// Returns whether it was new.
    #[inline]
    fn insert(&mut self, hash: u64, key: K, value: V) -> bool
    where
        K: Equal,
    {
        let slots = &self.slots;
        match self.index.entry(hash, |ix| ix.hash == hash && matches!(&slots[ix.slot], Some((k, _)) if k.equal(&key)), |ix| ix.hash) {
            Entry::Occupied(e) => {
                let slot = e.get().slot;
                *self.value_mut(slot) = value;
                false
            }
            Entry::Vacant(e) => {
                let slot = self.slots.len();
                self.slots.push(Some((key, value)));
                e.insert(Ix { slot, hash });
                self.live += 1;
                true
            }
        }
    }

    /// The value of `key`, inserting `make()` last when it's missing.
    #[inline]
    fn get_or_insert_with(&mut self, hash: u64, key: K, make: impl FnOnce() -> V) -> &mut V
    where
        K: Equal,
    {
        let slots = &self.slots;
        let slot =
            match self.index.entry(hash, |ix| ix.hash == hash && matches!(&slots[ix.slot], Some((k, _)) if k.equal(&key)), |ix| ix.hash) {
                Entry::Occupied(e) => e.get().slot,
                Entry::Vacant(e) => {
                    // Made before anything changes, in case it throws.
                    let value = make();
                    let slot = self.slots.len();
                    self.slots.push(Some((key, value)));
                    e.insert(Ix { slot, hash });
                    self.live += 1;
                    slot
                }
            };
        self.value_mut(slot)
    }

    fn remove(&mut self, hash: u64, key: &K) -> Option<(K, V)>
    where
        K: Equal,
    {
        let slots = &self.slots;
        let found = self.index.find_entry(hash, |ix| ix.hash == hash && matches!(&slots[ix.slot], Some((k, _)) if k.equal(key))).ok()?;
        let (ix, _) = found.remove();
        let entry = self.slots[ix.slot].take();
        self.live -= 1;
        self.tidy_after_hole(ix.slot);
        entry
    }

    /// Keeps the slots tidy after `slot` became a hole: holes at the end are
    /// dropped, the cursor moves past holes at the front, and the rest are
    /// compacted away once there are too many.
    fn tidy_after_hole(&mut self, slot: usize) {
        if slot + 1 == self.slots.len() {
            while let Some(None) = self.slots.last() {
                self.slots.pop();
            }
        }
        if slot == self.first {
            while let Some(None) = self.slots.get(self.first) {
                self.first += 1;
            }
        }
        self.first = self.first.min(self.slots.len());
        let holes = self.slots.len() - self.live;
        if holes * MAX_HOLES_PER_LIVE.1 > self.live * MAX_HOLES_PER_LIVE.0 {
            self.compact();
        }
    }

    /// Removes every hole, keeping the order of the entries.
    fn compact(&mut self) {
        let mut moved_to = Vec::with_capacity(self.slots.len());
        let mut next = 0;
        for s in &self.slots {
            moved_to.push(next);
            next += usize::from(s.is_some());
        }
        for ix in self.index.iter_mut() {
            ix.slot = moved_to[ix.slot];
        }
        self.slots.retain(Option::is_some);
        self.first = 0;
    }
}

/// A dat `map<K, V>`, copy-on-write like [`List`]. An empty map may have no
/// table at all, so `{}` and the placeholder a move leaves cost no allocation.
pub struct Map<K, V>(Option<Shared<Table<K, V>>>);

impl<K, V> Map<K, V> {
    #[inline]
    pub const fn new() -> Map<K, V> {
        Map(None)
    }

    #[inline]
    pub fn len(&self) -> i64 {
        self.0.as_ref().map_or(0, |t| t.live as i64)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entries in insertion order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, K, V> {
        match &self.0 {
            Some(t) => t.iter(),
            None => Iter { slots: [].iter(), left: 0 },
        }
    }

    /// A table nothing else shares is kept for reuse.
    pub fn clear(&mut self) {
        match self.0.as_mut().and_then(Shared::get_mut) {
            Some(t) => {
                t.index.clear();
                t.slots.clear();
                t.first = 0;
                t.live = 0;
            }
            None => self.0 = None,
        }
    }

    /// The table, for tests that check when it's copied.
    #[cfg(test)]
    pub(crate) fn table(&self) -> Option<&Shared<Table<K, V>>> {
        self.0.as_ref()
    }
}

impl<K: DatHash, V> Map<K, V> {
    /// The slot of `key` and its hash.
    #[inline]
    fn slot_of(&self, key: &K) -> Option<usize> {
        self.0.as_ref().and_then(|t| t.find(hash_of(key), key))
    }

    /// The value of `key`, if present.
    #[inline]
    pub fn find(&self, key: &K) -> Option<&V> {
        let t = self.0.as_ref()?;
        let slot = t.find(hash_of(key), key)?;
        t.slots[slot].as_ref().map(|(_, v)| v)
    }

    /// `key in m` and `m.has(key)`.
    #[inline]
    pub fn has(&self, key: &K) -> bool {
        self.slot_of(key).is_some()
    }

    /// `m[key]`.
    #[inline]
    pub fn try_get(&self, key: &K) -> R<&V>
    where
        K: Show,
    {
        match self.find(key) {
            Some(v) => Ok(v),
            None => Err(key_error(key)),
        }
    }

    #[inline(always)]
    pub fn get(&self, key: &K, loc: &'static Loc) -> &V
    where
        K: Show,
    {
        or_throw(self.try_get(key), loc)
    }

    /// `m.get(key, fallback)`.
    #[inline]
    pub fn get_or(&self, key: &K, fallback: V) -> V
    where
        V: Clone,
    {
        match self.find(key) {
            Some(v) => v.clone(),
            None => fallback,
        }
    }
}

impl<K: DatHash + Clone, V: Clone> Map<K, V> {
    /// The table, to change. A shared one is copied first; a missing one is made.
    #[inline]
    fn table_mut(&mut self) -> &mut Table<K, V> {
        Shared::make_mut(self.0.get_or_insert_with(|| Shared::new(Table::new())))
    }

    /// The value of `key`, to change in place. The table is copied only if
    /// it's shared and `key` is present. A copy keeps every slot number.
    #[inline]
    pub fn find_mut(&mut self, key: &K) -> Option<&mut V> {
        let slot = self.slot_of(key)?;
        Some(self.table_mut().value_mut(slot))
    }

    /// `m[key]`, to change in place: `m[key] += 1`, `m[key].push(x)`.
    #[inline]
    pub fn try_get_mut(&mut self, key: &K) -> R<&mut V>
    where
        K: Show,
    {
        match self.slot_of(key) {
            Some(slot) => Ok(self.table_mut().value_mut(slot)),
            None => Err(key_error(key)),
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self, key: &K, loc: &'static Loc) -> &mut V
    where
        K: Show,
    {
        or_throw(self.try_get_mut(key), loc)
    }

    /// `f(mut m["a"], mut m["b"])`: the values of `keys`, all at once. The
    /// call reached each one first, so they're present and all different.
    pub fn disjoint_mut<const N: usize>(&mut self, keys: [&K; N]) -> [&mut V; N] {
        let slots = keys.map(|k| self.slot_of(k).unwrap_or_else(|| crate::unreachable()));
        match self.table_mut().slots.get_disjoint_mut(slots) {
            Ok(entries) => entries.map(|e| match e {
                Some((_, v)) => v,
                None => crate::unreachable(),
            }),
            Err(_) => crate::unreachable(),
        }
    }

    /// `m[key] = value` and `m.set(key, value)`.
    #[inline]
    pub fn insert(&mut self, key: K, value: V) {
        let t = self.table_mut();
        // The entry the last lookup found, without hashing the key again.
        if let Some(slot) = t.last.take()
            && let Some(Some((k, v))) = t.slots.get_mut(slot)
            && k.equal(&key)
        {
            *v = value;
            return;
        }
        t.insert(hash_of(&key), key, value);
    }

    /// Adds `key` unless it's present, and says whether it was added. A
    /// shared table isn't copied when nothing changes.
    pub(crate) fn add_key(&mut self, key: K, value: V) -> bool {
        let hash = hash_of(&key);
        if let Some(t) = &self.0
            && !Shared::is_unique(t)
            && t.find(hash, &key).is_some()
        {
            return false;
        }
        self.table_mut().insert(hash, key, value)
    }

    /// The value of `key`, inserting `make()` last when it's missing.
    pub fn get_or_insert_with(&mut self, key: K, make: impl FnOnce() -> V) -> &mut V {
        let hash = hash_of(&key);
        self.table_mut().get_or_insert_with(hash, key, make)
    }

    /// Removes `key`; the others keep their order.
    pub fn delete(&mut self, key: &K) -> bool {
        let Some(t) = &mut self.0 else { return false };
        let hash = hash_of(key);
        if !Shared::is_unique(t) && t.find(hash, key).is_none() {
            return false;
        }
        Shared::make_mut(t).remove(hash, key).is_some()
    }

    pub fn keys(&self) -> List<K> {
        self.iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn values(&self) -> List<V> {
        self.iter().map(|(_, v)| v.clone()).collect()
    }

    pub fn entries(&self) -> List<(K, V)> {
        self.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// `{ ...self, ...other }`: `other`'s values win, and keys already here
    /// keep their place.
    pub fn extend_from(&mut self, other: &Map<K, V>) {
        if self.is_empty() {
            *self = other.clone();
            return;
        }
        for (k, v) in other.iter() {
            self.insert(k.clone(), v.clone());
        }
    }

    /// Moves the value of `key` out, leaving a placeholder in its place, for
    /// take-and-restore. The entry keeps its place.
    pub fn take(&mut self, key: &K) -> Option<V>
    where
        V: Placeholder,
    {
        let slot = self.slot_of(key)?;
        let t = self.table_mut();
        t.last.set(Some(slot));
        Some(std::mem::replace(t.value_mut(slot), V::placeholder()))
    }

    /// Puts back a value [`Map::take`] moved out.
    pub fn restore(&mut self, key: &K, value: V) {
        match self.find_mut(key) {
            Some(v) => *v = value,
            None => self.insert(key.clone(), value),
        }
    }
}

/// The entries of a map, in insertion order.
pub struct Iter<'a, K, V> {
    slots: std::slice::Iter<'a, Option<(K, V)>>,
    left: usize,
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    #[inline]
    fn next(&mut self) -> Option<(&'a K, &'a V)> {
        // Stops at the last entry, without walking any slots after it.
        if self.left == 0 {
            return None;
        }
        let (k, v) = self.slots.by_ref().flatten().next()?;
        self.left -= 1;
        Some((k, v))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.left, Some(self.left))
    }
}

impl<K, V> ExactSizeIterator for Iter<'_, K, V> {}

impl<'a, K, V> IntoIterator for &'a Map<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = Iter<'a, K, V>;

    fn into_iter(self) -> Iter<'a, K, V> {
        self.iter()
    }
}

impl<K, V> Clone for Map<K, V> {
    #[inline]
    fn clone(&self) -> Map<K, V> {
        Map(self.0.clone())
    }
}

impl<K, V> Default for Map<K, V> {
    fn default() -> Map<K, V> {
        Map::new()
    }
}

impl<K: DatHash + Clone, V: Clone> FromIterator<(K, V)> for Map<K, V> {
    /// Later entries win; a key keeps the place where it first appeared.
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Map<K, V> {
        let mut m = Map::new();
        for (k, v) in iter {
            m.insert(k, v);
        }
        m
    }
}

impl<K: Show, V: Show> Show for Map<K, V> {
    fn show(&self, out: &mut String) {
        out.push('{');
        for (i, (k, v)) in self.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            k.show(out);
            out.push_str(": ");
            v.show(out);
        }
        out.push('}');
    }
}

impl<K: DatHash, V: Equal> Equal for Map<K, V> {
    /// The same keys with equal values, in any order.
    fn equal(&self, other: &Map<K, V>) -> bool {
        self.len() == other.len() && self.iter().all(|(k, v)| other.find(k).is_some_and(|w| v.equal(w)))
    }
}

impl<K, V> Placeholder for Map<K, V> {
    #[inline(always)]
    fn placeholder() -> Map<K, V> {
        Map::new()
    }
}

impl<K: DatHash, V: Equal> PartialEq for Map<K, V> {
    fn eq(&self, other: &Map<K, V>) -> bool {
        self.equal(other)
    }
}

impl<K: Show, V: Show> fmt::Debug for Map<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::debug(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Str, debug};
    use std::time::{Duration, Instant};

    fn s(x: &str) -> Str {
        Str::new(x)
    }

    fn scores() -> Map<Str, i64> {
        [(s("alice"), 10), (s("bob"), 20), (s("charlie"), 30)].into_iter().collect()
    }

    fn keys(m: &Map<i64, i64>) -> Vec<i64> {
        m.iter().map(|(k, _)| *k).collect()
    }

    /// The table's slots and holes, to check the bookkeeping.
    fn shape<K, V>(m: &Map<K, V>) -> (usize, usize, usize) {
        m.table().map_or((0, 0, 0), |t| (t.slots.len(), t.first, t.live))
    }

    #[test]
    fn disjoint_values_change_together() {
        let mut m = scores();
        m.delete(&s("alice"));
        let before = m.clone();
        let [b, c] = m.disjoint_mut([&s("charlie"), &s("bob")]);
        std::mem::swap(b, c);
        assert_eq!(debug(&m), r#"{"bob": 30, "charlie": 20}"#);
        assert_eq!(debug(&before), r#"{"bob": 20, "charlie": 30}"#);
    }

    #[test]
    fn printing() {
        assert_eq!(debug(&scores()), r#"{"alice": 10, "bob": 20, "charlie": 30}"#);
        assert_eq!(debug(&Map::<i64, i64>::new()), "{}");
        let mut nested: Map<i64, List<Str>> = Map::new();
        nested.insert(1, [s("a"), s("b")].into_iter().collect());
        nested.insert(2, List::new());
        assert_eq!(debug(&nested), r#"{1: ["a", "b"], 2: []}"#);
        let floats: Map<bool, f64> = [(true, 2.0), (false, 0.5)].into_iter().collect();
        assert_eq!(debug(&floats), "{true: 2.0, false: 0.5}");
        assert_eq!(crate::text(&scores()).as_str(), r#"{"alice": 10, "bob": 20, "charlie": 30}"#);
    }

    #[test]
    fn access_and_key_errors() {
        let m = scores();
        assert_eq!(*m.try_get(&s("bob")).unwrap(), 20);
        let e = m.try_get(&s("nobody")).unwrap_err();
        assert_eq!(e.kind, ErrorKind::KeyError);
        assert_eq!(e.message, r#"key "nobody" not found"#);
        let tuples: Map<(i64, Str), i64> = Map::new();
        assert_eq!(tuples.try_get(&(1, s("x"))).unwrap_err().message, r#"key [1, "x"] not found"#);
        assert_eq!(m.get_or(&s("nobody"), 0), 0);
        assert_eq!(m.get_or(&s("alice"), 0), 10);
        assert!(m.has(&s("charlie")));
        assert!(!m.has(&s("dave")));
        let mut m = m;
        *m.try_get_mut(&s("alice")).unwrap() += 5;
        assert_eq!(m.try_get_mut(&s("zed")).unwrap_err().message, r#"key "zed" not found"#);
        assert_eq!(*m.find(&s("alice")).unwrap(), 15);
        assert!(m.find_mut(&s("zed")).is_none());
    }

    #[test]
    fn order_rules() {
        let mut m = scores();
        m.insert(s("alice"), 11);
        assert_eq!(debug(&m), r#"{"alice": 11, "bob": 20, "charlie": 30}"#, "setting a key keeps its place");
        assert!(m.delete(&s("bob")));
        assert!(!m.delete(&s("bob")));
        assert_eq!(debug(&m.keys()), r#"["alice", "charlie"]"#);
        assert_eq!(debug(&m.values()), "[11, 30]");
        m.insert(s("bob"), 21);
        assert_eq!(debug(&m), r#"{"alice": 11, "charlie": 30, "bob": 21}"#, "a key added again goes last");
        assert!(m.delete(&s("alice")));
        m.insert(s("alice"), 1);
        assert_eq!(debug(&m.entries()), r#"[["charlie", 30], ["bob", 21], ["alice", 1]]"#);
        assert_eq!(m.len(), 3);
        // Later entries win; a key keeps the place where it first appeared.
        let built: Map<i64, i64> = [(1, 1), (2, 2), (1, 3)].into_iter().collect();
        assert_eq!(debug(&built), "{1: 3, 2: 2}");
        let mut merged = built.clone();
        merged.extend_from(&[(3, 30), (2, 20)].into_iter().collect());
        assert_eq!(debug(&merged), "{1: 3, 2: 20, 3: 30}");
        let mut from_empty = Map::new();
        from_empty.extend_from(&built);
        from_empty.extend_from(&[(0, 0)].into_iter().collect());
        assert_eq!(debug(&from_empty), "{1: 3, 2: 2, 0: 0}");
        assert_eq!(debug(&built), "{1: 3, 2: 2}");
        m.clear();
        assert!(m.is_empty());
        assert_eq!(debug(&m), "{}");
        m.insert(s("z"), 0);
        assert_eq!(debug(&m), r#"{"z": 0}"#);
    }

    #[test]
    fn equality_ignores_order() {
        let a: Map<i64, f64> = [(1, 1.0), (2, 2.0)].into_iter().collect();
        let b: Map<i64, f64> = [(2, 2.0), (1, 1.0)].into_iter().collect();
        assert!(a.equal(&b));
        let c: Map<i64, f64> = [(1, 1.0), (2, 2.5)].into_iter().collect();
        assert!(!a.equal(&c));
        let d: Map<i64, f64> = [(1, 1.0)].into_iter().collect();
        assert!(!a.equal(&d) && !d.equal(&a));
        let nan: Map<i64, f64> = [(1, f64::NAN)].into_iter().collect();
        assert!(!nan.equal(&nan.clone()));
        assert!(Map::<i64, i64>::new().equal(&Map::new()));
    }

    #[test]
    fn compaction_keeps_order() {
        let mut m: Map<i64, i64> = (0..1000).map(|i| (i, i * 2)).collect();
        for i in (0..1000).filter(|i| i % 3 != 1) {
            assert!(m.delete(&i));
        }
        let expected: Vec<i64> = (0..1000).filter(|i| i % 3 == 1).collect();
        assert_eq!(keys(&m), expected);
        let (slots, _, live) = shape(&m);
        assert_eq!(live, expected.len());
        assert!((slots - live) * 4 <= live, "{} holes for {live} entries", slots - live);
        for k in &expected {
            assert_eq!(*m.try_get(k).unwrap(), k * 2);
        }
        m.insert(0, 0);
        m.insert(1, -1);
        assert_eq!(keys(&m).last(), Some(&0));
        assert_eq!(*m.try_get(&1).unwrap(), -1);
        assert_eq!(keys(&m)[0], 1);
        // Holes at the end are dropped, and the cursor skips holes at the front.
        let mut m: Map<i64, i64> = (0..10).map(|i| (i, i)).collect();
        m.delete(&9);
        m.delete(&8);
        assert_eq!(shape(&m), (8, 0, 8));
        m.delete(&0);
        assert_eq!(shape(&m), (8, 1, 7));
        // A second hole among 6 entries is too many: compacted.
        m.delete(&2);
        assert_eq!(shape(&m), (6, 0, 6));
        m.delete(&1);
        assert_eq!(shape(&m), (6, 1, 5));
        assert_eq!(keys(&m), vec![3, 4, 5, 6, 7]);
        for k in 3..8 {
            m.delete(&k);
        }
        assert_eq!(shape(&m), (0, 0, 0));
        m.insert(42, 1);
        assert_eq!(keys(&m), vec![42]);
    }

    /// A map used as a queue of 1,000 entries for `n` steps, then a map of
    /// `n` entries drained from the front: the time it takes.
    fn queue_and_drain(n: i64) -> Duration {
        let start = Instant::now();
        let mut queue: Map<i64, i64> = (0..1000).map(|i| (i, i)).collect();
        let mut sum = 0;
        for next in 1000..n + 1000 {
            let (&oldest, &v) = queue.iter().next().unwrap();
            sum += v;
            queue.delete(&oldest);
            queue.insert(next, next);
        }
        assert_eq!(queue.len(), 1000);
        assert_eq!(sum, (0..n).sum::<i64>());
        assert_eq!(keys(&queue), (n..n + 1000).collect::<Vec<_>>());
        let mut big: Map<i64, i64> = (0..n).map(|i| (i, i)).collect();
        while let Some((&k, _)) = big.iter().next() {
            big.delete(&k);
        }
        assert!(big.is_empty());
        start.elapsed()
    }

    #[test]
    fn a_map_used_as_a_queue_stays_linear() {
        // Linear is about 4x at four times the size, and quadratic 16x. The
        // faster of two runs of each, so a busy machine doesn't decide it.
        let n = 25_000;
        let best = |n| (0..2).map(|_| queue_and_drain(n)).min().unwrap();
        let (small, large) = (best(n), best(4 * n));
        let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-9);
        assert!(ratio < 8.0, "{small:?} at {n}, {large:?} at {}: {ratio:.1}x", 4 * n);
    }

    #[test]
    fn copy_on_write() {
        let a = scores();
        let mut b = a.clone();
        assert!(Shared::ptr_eq(a.table().unwrap(), b.table().unwrap()));
        // Nothing to change: no copy.
        assert!(!b.delete(&s("nobody")));
        assert!(b.find_mut(&s("nobody")).is_none());
        assert!(Shared::ptr_eq(a.table().unwrap(), b.table().unwrap()));
        b.insert(s("dave"), 40);
        *b.find_mut(&s("alice")).unwrap() = 0;
        assert!(!Shared::ptr_eq(a.table().unwrap(), b.table().unwrap()));
        assert_eq!(debug(&a), r#"{"alice": 10, "bob": 20, "charlie": 30}"#);
        assert_eq!(debug(&b), r#"{"alice": 0, "bob": 20, "charlie": 30, "dave": 40}"#);
        // A copy of a map with holes keeps its slots, and finds every key.
        let mut c = a.clone();
        c.delete(&s("alice"));
        let mut d = c.clone();
        *d.try_get_mut(&s("charlie")).unwrap() = 3;
        assert_eq!(debug(&d), r#"{"bob": 20, "charlie": 3}"#);
        assert_eq!(debug(&c), r#"{"bob": 20, "charlie": 30}"#);
        // Changing a map nothing else shares doesn't copy it.
        let before = Shared::as_ptr(b.table().unwrap());
        b.insert(s("eve"), 50);
        b.delete(&s("bob"));
        *b.try_get_mut(&s("eve")).unwrap() += 1;
        assert_eq!(Shared::as_ptr(b.table().unwrap()), before);
        assert!(Map::<i64, i64>::placeholder().table().is_none());
    }

    #[test]
    fn take_and_restore_keep_the_order() {
        let mut teams: Map<Str, List<Str>> = Map::new();
        teams.insert(s("red"), [s("Ada")].into_iter().collect());
        teams.insert(s("blue"), List::new());
        teams.insert(s("green"), List::new());
        let mut members = teams.take(&s("red")).unwrap();
        assert_eq!(debug(&teams), r#"{"red": [], "blue": [], "green": []}"#);
        let before = Shared::as_ptr(members.buffer().unwrap());
        members.push(s("Lin"));
        assert_eq!(Shared::as_ptr(members.buffer().unwrap()), before, "the taken list isn't shared, so it isn't copied");
        teams.restore(&s("red"), members);
        assert_eq!(debug(&teams), r#"{"red": ["Ada", "Lin"], "blue": [], "green": []}"#);
        assert!(teams.take(&s("nobody")).is_none());
        teams.restore(&s("violet"), List::new());
        assert_eq!(debug(&teams.keys()), r#"["red", "blue", "green", "violet"]"#);
        teams.get_or_insert_with(s("blue"), List::new).push(s("Kai"));
        teams.get_or_insert_with(s("black"), List::new).push(s("Max"));
        assert_eq!(debug(&teams), r#"{"red": ["Ada", "Lin"], "blue": ["Kai"], "green": [], "violet": [], "black": ["Max"]}"#);
    }

    #[test]
    fn keys_of_every_hashable_kind() {
        let mut m: Map<List<i64>, i64> = Map::new();
        m.insert([1, 2].into_iter().collect(), 1);
        m.insert([1].into_iter().collect(), 2);
        m.insert([1, 2].into_iter().collect(), 3);
        assert_eq!(debug(&m), "{[1, 2]: 3, [1]: 2}");
        let mut t: Map<(Str, Str), bool> = Map::new();
        t.insert((s("ab"), s("c")), true);
        t.insert((s("a"), s("bc")), false);
        assert_eq!(t.len(), 2);
        assert!(*t.try_get(&(s("ab"), s("c"))).unwrap());
        // Many keys, so the table grows and rehashes from its stored hashes.
        let big: Map<Str, i64> = (0..10_000).map(|i| (Str::from_string(i.to_string()), i)).collect();
        assert!((0..10_000).all(|i| *big.try_get(&Str::from_string(i.to_string())).unwrap() == i));
        assert_eq!(hash_of(&s("x")), hash_of(&s("x")));
        assert_ne!(hash_of(&(s("ab"), s("c"))), hash_of(&(s("a"), s("bc"))));
    }
}
