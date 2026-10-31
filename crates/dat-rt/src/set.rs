//! `set<T>` (`reference/11-set-methods.md`): a [`Map`] whose values are `()`,
//! so it keeps insertion order the same way. Results that are sets keep this
//! set's order first, then `other`'s.

use crate::{DatHash, Equal, List, Map, Placeholder, Show};
use std::fmt;

/// A dat `set<T>`, copy-on-write like [`List`].
pub struct Set<T>(Map<T, ()>);

impl<T> Set<T> {
    #[inline]
    pub const fn new() -> Set<T> {
        Set(Map::new())
    }

    #[inline]
    pub fn len(&self) -> i64 {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The elements in insertion order.
    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        Iter(self.0.iter())
    }

    pub fn clear(&mut self) {
        self.0.clear()
    }

    #[cfg(test)]
    pub(crate) fn map(&self) -> &Map<T, ()> {
        &self.0
    }
}

impl<T: DatHash> Set<T> {
    #[inline]
    pub fn has(&self, item: &T) -> bool {
        self.0.has(item)
    }

    pub fn is_subset_of(&self, other: &Set<T>) -> bool {
        self.len() <= other.len() && self.iter().all(|x| other.has(x))
    }

    pub fn is_superset_of(&self, other: &Set<T>) -> bool {
        other.is_subset_of(self)
    }

    pub fn is_disjoint_from(&self, other: &Set<T>) -> bool {
        let (small, large) = if self.len() <= other.len() { (self, other) } else { (other, self) };
        !small.iter().any(|x| large.has(x))
    }
}

impl<T: DatHash + Clone> Set<T> {
    /// A set of `items`' values, each where it first appears.
    pub fn from_list(items: &List<T>) -> Set<T> {
        items.iter().cloned().collect()
    }

    /// Adds `item` last, unless it's present.
    #[inline]
    pub fn add(&mut self, item: T) {
        self.0.add_key(item, ());
    }

    pub fn delete(&mut self, item: &T) -> bool {
        self.0.delete(item)
    }

    pub fn to_list(&self) -> List<T> {
        self.0.keys()
    }

    pub fn union(&self, other: &Set<T>) -> Set<T> {
        let mut out = self.clone();
        for x in other.iter() {
            out.add(x.clone());
        }
        out
    }

    pub fn intersection(&self, other: &Set<T>) -> Set<T> {
        self.iter().filter(|x| other.has(x)).cloned().collect()
    }

    pub fn difference(&self, other: &Set<T>) -> Set<T> {
        self.iter().filter(|x| !other.has(x)).cloned().collect()
    }

    pub fn symmetric_difference(&self, other: &Set<T>) -> Set<T> {
        let mine = self.iter().filter(|x| !other.has(x));
        let theirs = other.iter().filter(|x| !self.has(x));
        mine.chain(theirs).cloned().collect()
    }
}

/// The elements of a set, in insertion order.
pub struct Iter<'a, T>(crate::map::Iter<'a, T, ()>);

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<&'a T> {
        self.0.next().map(|(k, _)| k)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}

impl<'a, T> IntoIterator for &'a Set<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

impl<T> Clone for Set<T> {
    #[inline]
    fn clone(&self) -> Set<T> {
        Set(self.0.clone())
    }
}

impl<T> Default for Set<T> {
    fn default() -> Set<T> {
        Set::new()
    }
}

impl<T: DatHash + Clone> FromIterator<T> for Set<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Set<T> {
        let mut s = Set::new();
        for x in iter {
            s.add(x);
        }
        s
    }
}

impl<T: Show> Show for Set<T> {
    fn show(&self, out: &mut String) {
        out.push('{');
        for (i, x) in self.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            x.show(out);
        }
        out.push('}');
    }
}

impl<T: DatHash> Equal for Set<T> {
    /// The same elements, in any order.
    fn equal(&self, other: &Set<T>) -> bool {
        self.len() == other.len() && self.is_subset_of(other)
    }
}

impl<T> Placeholder for Set<T> {
    #[inline(always)]
    fn placeholder() -> Set<T> {
        Set::new()
    }
}

impl<T: DatHash> PartialEq for Set<T> {
    fn eq(&self, other: &Set<T>) -> bool {
        self.equal(other)
    }
}

impl<T: Show> fmt::Debug for Set<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::debug(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Shared, Str, debug};

    fn tags(xs: &[&str]) -> Set<Str> {
        xs.iter().map(|s| Str::new(s)).collect()
    }

    fn s(x: &str) -> Str {
        Str::new(x)
    }

    #[test]
    fn printing() {
        let ints: Set<i64> = [1, 2, 3].into_iter().collect();
        assert_eq!(debug(&ints), "{1, 2, 3}");
        assert_eq!(debug(&Set::<i64>::new()), "{}");
        assert_eq!(debug(&tags(&["go", "rust", "go"])), r#"{"go", "rust"}"#);
        let nested: List<Set<i64>> = [ints.clone(), Set::new()].into_iter().collect();
        assert_eq!(debug(&nested), "[{1, 2, 3}, {}]");
    }

    #[test]
    fn order_rules() {
        let mut t = tags(&["go", "rust", "python"]);
        t.add(s("zig"));
        t.add(s("go"));
        assert_eq!(t.len(), 4);
        assert_eq!(debug(&t), r#"{"go", "rust", "python", "zig"}"#);
        assert!(t.has(&s("rust")));
        assert!(t.delete(&s("python")));
        assert!(!t.delete(&s("python")));
        t.add(s("python"));
        assert_eq!(debug(&t.to_list()), r#"["go", "rust", "zig", "python"]"#);
        let from: Set<i64> = Set::from_list(&[3, 1, 3, 2, 1].into_iter().collect());
        assert_eq!(debug(&from), "{3, 1, 2}");
        t.clear();
        assert!(t.is_empty());
    }

    #[test]
    fn set_operations_keep_this_sets_order_first() {
        let mut t = tags(&["go", "rust", "python", "zig"]);
        t.delete(&s("python"));
        let systems = tags(&["rust", "zig", "c"]);
        assert_eq!(debug(&t.intersection(&systems)), r#"{"rust", "zig"}"#);
        assert_eq!(debug(&t.difference(&systems)), r#"{"go"}"#);
        assert_eq!(debug(&t.union(&systems)), r#"{"go", "rust", "zig", "c"}"#);
        assert_eq!(debug(&systems.union(&t)), r#"{"rust", "zig", "c", "go"}"#);
        assert_eq!(debug(&t.symmetric_difference(&systems)), r#"{"go", "c"}"#);
        assert!(tags(&["zig"]).is_subset_of(&t));
        assert!(!systems.is_subset_of(&t));
        assert!(t.is_superset_of(&tags(&["go", "zig"])));
        assert!(t.is_disjoint_from(&tags(&["c", "d"])));
        assert!(!t.is_disjoint_from(&systems));
        assert!(Set::<i64>::new().is_subset_of(&Set::new()));
    }

    #[test]
    fn equality_ignores_order() {
        assert!(tags(&["a", "b"]).equal(&tags(&["b", "a"])));
        assert!(!tags(&["a", "b"]).equal(&tags(&["a"])));
        assert!(!tags(&["a", "b"]).equal(&tags(&["a", "c"])));
    }

    #[test]
    fn copy_on_write() {
        let a = tags(&["x", "y"]);
        let mut b = a.clone();
        b.add(s("x"));
        assert!(Shared::ptr_eq(a.map().table().unwrap(), b.map().table().unwrap()), "adding a present element copies nothing");
        let u = a.union(&tags(&["y"]));
        assert!(Shared::ptr_eq(a.map().table().unwrap(), u.map().table().unwrap()));
        b.add(s("z"));
        assert_eq!(debug(&a), r#"{"x", "y"}"#);
        assert_eq!(debug(&b), r#"{"x", "y", "z"}"#);
    }
}
