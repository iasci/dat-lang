//! `list<T>` (`reference/10-list-methods.md`), `range` and the length check
//! of destructuring.
//!
//! `try_*` functions are for the interpreter. The others are for generated
//! code: the ones that can fail take the operation's location and throw.
//! Methods that take callbacks exist only in the `try_*` form. Generated code
//! passes callbacks that always return `Ok`, since they throw by unwinding,
//! and unwraps the result with [`crate::done`].
//!
//! Callbacks are called in element order, once per element. A callback that
//! fails leaves the list as it was.

use crate::{DatError, DatHash, Equal, ErrorKind, Loc, Map, Order, Placeholder, R, Shared, Show, Str, throw};
use std::any::Any;
use std::cmp::Ordering;
use std::fmt;
use std::panic::{self, AssertUnwindSafe};

#[inline(always)]
fn or_throw<T>(r: R<T>, loc: &'static Loc) -> T {
    match r {
        Ok(v) => v,
        Err(e) => throw(e, loc),
    }
}

#[cold]
#[inline(never)]
fn range_error(message: String) -> Box<DatError> {
    Box::new(DatError::new(ErrorKind::RangeError, message))
}

#[cold]
#[inline(never)]
fn index_error(i: i64, len: usize) -> Box<DatError> {
    range_error(format!("index {i} out of range for length {len}"))
}

#[cold]
#[inline(never)]
fn empty_error(method: &str) -> Box<DatError> {
    range_error(format!("{method}() on an empty list"))
}

/// A dat `list<T>`. Copies share one buffer until one of them changes: every
/// change goes through [`Shared::make_mut`], which copies only a shared
/// buffer. An empty list may have no buffer at all, so `[]` and the
/// placeholder a move leaves behind cost no allocation.
pub struct List<T>(Option<Shared<Buf<T>>>);

/// The elements are `vec[head..]`, always one slice. `shift` moves the first
/// element out, leaves a placeholder and advances `head`, so a queue stays
/// linear; the dead front is dropped once it outgrows the live part.
pub(crate) struct Buf<T> {
    vec: Vec<T>,
    head: usize,
}

impl<T> Buf<T> {
    #[inline(always)]
    fn live(&self) -> &[T] {
        &self.vec[self.head..]
    }

    #[inline(always)]
    fn live_mut(&mut self) -> &mut [T] {
        &mut self.vec[self.head..]
    }

    /// Drops the dead front once it's longer than the live part. The moves
    /// this costs are paid for by the shifts that made the dead front.
    #[inline]
    fn trim_front(&mut self) {
        if self.head > self.vec.len() - self.head {
            self.vec.drain(..self.head);
            self.head = 0;
        }
    }
}

impl<T> crate::copies::Size for Buf<T> {
    fn size(&self) -> u64 {
        self.live().len() as u64
    }
}

impl<T: Clone> Clone for Buf<T> {
    /// A copy holds only the live part.
    fn clone(&self) -> Buf<T> {
        Buf { vec: self.live().to_vec(), head: 0 }
    }
}

/// A `slice` position: negative counts from the end, and out-of-range
/// positions are clamped.
fn position(p: i64, len: usize) -> usize {
    if p < 0 { len.saturating_sub(p.unsigned_abs().min(len as u64) as usize) } else { (p as u64).min(len as u64) as usize }
}

impl<T> List<T> {
    #[inline]
    pub const fn new() -> List<T> {
        List(None)
    }

    pub fn with_capacity(n: usize) -> List<T> {
        if n == 0 { List(None) } else { List::from_vec(Vec::with_capacity(n)) }
    }

    #[inline]
    pub fn from_vec(vec: Vec<T>) -> List<T> {
        if vec.capacity() == 0 { List(None) } else { List(Some(Shared::new(Buf { vec, head: 0 }))) }
    }

    #[inline]
    pub fn len(&self) -> i64 {
        self.as_slice().len() as i64
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[T] {
        match &self.0 {
            Some(b) => b.live(),
            None => &[],
        }
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    /// The buffer, for tests that check when it's copied.
    #[cfg(test)]
    pub(crate) fn buffer(&self) -> Option<&Shared<Buf<T>>> {
        self.0.as_ref()
    }

    // ---- index ----

    /// `xs[i]`.
    #[inline]
    pub fn try_get(&self, i: i64) -> R<&T> {
        let s = self.as_slice();
        match usize::try_from(i).ok().and_then(|j| s.get(j)) {
            Some(x) => Ok(x),
            None => Err(index_error(i, s.len())),
        }
    }

    #[inline(always)]
    pub fn get(&self, i: i64, loc: &'static Loc) -> &T {
        or_throw(self.try_get(i), loc)
    }

    /// `xs.at(i)`: a negative `i` counts from the end.
    #[inline]
    pub fn try_at(&self, i: i64) -> R<&T> {
        let s = self.as_slice();
        let j = if i < 0 { i + s.len() as i64 } else { i };
        match usize::try_from(j).ok().and_then(|j| s.get(j)) {
            Some(x) => Ok(x),
            None => Err(index_error(i, s.len())),
        }
    }

    #[inline(always)]
    pub fn at(&self, i: i64, loc: &'static Loc) -> &T {
        or_throw(self.try_at(i), loc)
    }

    /// `xs.clear()`. A buffer nothing else shares is kept for reuse.
    pub fn clear(&mut self) {
        match self.0.as_mut().and_then(Shared::get_mut) {
            Some(b) => {
                b.vec.clear();
                b.head = 0;
            }
            None => self.0 = None,
        }
    }

    // ---- reading ----

    pub fn includes(&self, item: &T) -> bool
    where
        T: Equal,
    {
        self.iter().any(|x| x.equal(item))
    }

    pub fn index_of(&self, item: &T) -> i64
    where
        T: Equal,
    {
        self.iter().position(|x| x.equal(item)).map_or(-1, |i| i as i64)
    }

    pub fn last_index_of(&self, item: &T) -> i64
    where
        T: Equal,
    {
        self.iter().rposition(|x| x.equal(item)).map_or(-1, |i| i as i64)
    }

    /// The elements' `toString()` texts, so strings aren't quoted.
    pub fn join(&self, separator: &str) -> Str
    where
        T: Show,
    {
        let mut out = String::new();
        for (i, x) in self.iter().enumerate() {
            if i > 0 {
                out.push_str(separator);
            }
            x.text(&mut out);
        }
        Str::from_string(out)
    }

    // ---- callbacks that don't build a list ----

    pub fn try_reduce<U>(&self, initial: U, mut f: impl FnMut(U, &T) -> R<U>) -> R<U> {
        let mut acc = initial;
        for x in self.iter() {
            acc = f(acc, x)?;
        }
        Ok(acc)
    }

    pub fn try_for_each(&self, mut f: impl FnMut(&T, i64) -> R<()>) -> R<()> {
        for (i, x) in self.iter().enumerate() {
            f(x, i as i64)?;
        }
        Ok(())
    }

    /// `find(fn, fallback)`.
    pub fn try_find(&self, mut f: impl FnMut(&T, i64) -> R<bool>, fallback: T) -> R<T>
    where
        T: Clone,
    {
        for (i, x) in self.iter().enumerate() {
            if f(x, i as i64)? {
                return Ok(x.clone());
            }
        }
        Ok(fallback)
    }

    pub fn try_find_index(&self, mut f: impl FnMut(&T, i64) -> R<bool>) -> R<i64> {
        for (i, x) in self.iter().enumerate() {
            if f(x, i as i64)? {
                return Ok(i as i64);
            }
        }
        Ok(-1)
    }

    pub fn try_some(&self, mut f: impl FnMut(&T, i64) -> R<bool>) -> R<bool> {
        for (i, x) in self.iter().enumerate() {
            if f(x, i as i64)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn try_every(&self, mut f: impl FnMut(&T, i64) -> R<bool>) -> R<bool> {
        for (i, x) in self.iter().enumerate() {
            if !f(x, i as i64)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn try_count(&self, mut f: impl FnMut(&T, i64) -> R<bool>) -> R<i64> {
        let mut n = 0;
        for (i, x) in self.iter().enumerate() {
            if f(x, i as i64)? {
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn try_map<U>(&self, mut f: impl FnMut(&T, i64) -> R<U>) -> R<List<U>> {
        let mut out = Vec::with_capacity(self.as_slice().len());
        for (i, x) in self.iter().enumerate() {
            out.push(f(x, i as i64)?);
        }
        Ok(List::from_vec(out))
    }

    pub fn try_flat_map<U: Clone>(&self, mut f: impl FnMut(&T, i64) -> R<List<U>>) -> R<List<U>> {
        let mut out = Vec::new();
        for (i, x) in self.iter().enumerate() {
            out.extend_from_slice(f(x, i as i64)?.as_slice());
        }
        Ok(List::from_vec(out))
    }

    pub fn zip<U: Clone>(&self, other: &List<U>) -> List<(T, U)>
    where
        T: Clone,
    {
        self.iter().zip(other.iter()).map(|(a, b)| (a.clone(), b.clone())).collect()
    }
}

/// A list a loop changes, made its buffer's only owner once, before the
/// loop ([`List::edit`]). Its operations are `List`'s, without the check
/// for a shared buffer that each change of a `List` makes. Generated code
/// uses it only for a list the loop doesn't copy or pass on, so nothing
/// can share the buffer while it's being changed.
pub struct ListMut<'a, T>(&'a mut Buf<T>);

/// A list a loop only reads and writes the elements of, made the list's
/// own once, before the loop ([`List::edit_slice`]). Its length stays in a
/// register, where a [`ListMut`] reads it from the buffer at every access.
pub struct ListSlice<'a, T>(&'a mut [T]);

impl<T> ListSlice<'_, T> {
    #[inline(always)]
    pub fn len(&self) -> i64 {
        self.0.len() as i64
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `xs[i]`.
    #[inline(always)]
    pub fn get(&self, i: i64, loc: &'static Loc) -> &T {
        match usize::try_from(i).ok().and_then(|j| self.0.get(j)) {
            Some(x) => x,
            None => throw(index_error(i, self.0.len()), loc),
        }
    }

    /// `xs[i]`, to change in place.
    #[inline(always)]
    pub fn get_mut(&mut self, i: i64, loc: &'static Loc) -> &mut T {
        let len = self.0.len();
        match usize::try_from(i).ok().and_then(|j| self.0.get_mut(j)) {
            Some(x) => x,
            None => throw(index_error(i, len), loc),
        }
    }
}

impl<T> ListMut<'_, T> {
    #[inline(always)]
    pub fn len(&self) -> i64 {
        self.0.live().len() as i64
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.0.live().is_empty()
    }

    /// `xs[i]`.
    #[inline(always)]
    pub fn get(&self, i: i64, loc: &'static Loc) -> &T {
        let s = self.0.live();
        match usize::try_from(i).ok().and_then(|j| s.get(j)) {
            Some(x) => x,
            None => throw(index_error(i, s.len()), loc),
        }
    }

    /// `xs[i]`, to change in place.
    #[inline(always)]
    pub fn get_mut(&mut self, i: i64, loc: &'static Loc) -> &mut T {
        let s = self.0.live_mut();
        let len = s.len();
        match usize::try_from(i).ok().and_then(|j| s.get_mut(j)) {
            Some(x) => x,
            None => throw(index_error(i, len), loc),
        }
    }

    #[inline(always)]
    pub fn push(&mut self, v: T) {
        self.0.vec.push(v);
    }

    #[inline(always)]
    pub fn pop(&mut self, loc: &'static Loc) -> T {
        if self.is_empty() {
            throw(empty_error("pop"), loc);
        }
        let v = self.0.vec.pop();
        self.0.trim_front();
        v.unwrap_or_else(|| crate::unreachable())
    }

    #[inline(always)]
    pub fn shift(&mut self, loc: &'static Loc) -> T
    where
        T: Placeholder,
    {
        if self.is_empty() {
            throw(empty_error("shift"), loc);
        }
        let b = &mut *self.0;
        let v = std::mem::replace(&mut b.vec[b.head], T::placeholder());
        b.head += 1;
        b.trim_front();
        v
    }
}

impl List<i64> {
    /// `xs.sort((a, b) => a - b)`, or `b - a` when `descending`. If no two
    /// elements are far enough apart for their difference to overflow,
    /// which one pass over the list tells, the comparator can't throw and
    /// its sign is that of `a.cmp(b)`: the list is sorted in place and
    /// this returns `true`. Otherwise it returns `false` and changes
    /// nothing, and the comparator runs as written.
    pub fn sort_by_difference(&mut self, descending: bool) -> bool {
        let s = self.as_slice();
        let (min, max) = s.iter().fold((i64::MAX, i64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
        if s.len() >= 2 && max.checked_sub(min).is_none() {
            return false;
        }
        if s.len() >= 2 {
            // Equal integers can't be told apart, so stability doesn't matter.
            let s = self.slice_mut();
            if descending {
                s.sort_unstable_by(|a, b| b.cmp(a));
            } else {
                s.sort_unstable();
            }
        }
        true
    }
}

impl<T: Clone> List<T> {
    /// The buffer, to change. A shared one is copied first; a missing one is made.
    #[inline]
    fn buf_mut(&mut self) -> &mut Buf<T> {
        Shared::make_mut(self.0.get_or_insert_with(|| Shared::new(Buf { vec: Vec::new(), head: 0 })))
    }

    /// The list, to change in a loop: its buffer is copied here if it's
    /// shared, and not checked again.
    #[inline]
    pub fn edit(&mut self) -> ListMut<'_, T> {
        ListMut(self.buf_mut())
    }

    /// The list, to change in a loop that only reads and writes its
    /// elements: they're copied here if they're shared, and the loop works
    /// on the slice.
    #[inline]
    pub fn edit_slice(&mut self) -> ListSlice<'_, T> {
        ListSlice(self.slice_mut())
    }

    /// The elements, to change in place. Copies them first if they're shared.
    #[inline]
    pub fn slice_mut(&mut self) -> &mut [T] {
        match &mut self.0 {
            Some(b) => Shared::make_mut(b).live_mut(),
            None => &mut [],
        }
    }

    pub fn to_vec(&self) -> Vec<T> {
        self.as_slice().to_vec()
    }

    /// Puts `vec` in place of the elements, in the buffer itself when nothing
    /// else shares it.
    fn replace_with(&mut self, vec: Vec<T>) {
        match self.0.as_mut().and_then(Shared::get_mut) {
            Some(b) => {
                b.vec = vec;
                b.head = 0;
            }
            None => *self = List::from_vec(vec),
        }
    }

    // ---- index ----

    /// `xs[i]`, to change in place.
    #[inline]
    pub fn try_get_mut(&mut self, i: i64) -> R<&mut T> {
        let len = self.as_slice().len();
        match usize::try_from(i) {
            Ok(j) if j < len => Ok(&mut self.slice_mut()[j]),
            _ => Err(index_error(i, len)),
        }
    }

    #[inline(always)]
    pub fn get_mut(&mut self, i: i64, loc: &'static Loc) -> &mut T {
        or_throw(self.try_get_mut(i), loc)
    }

    /// `f(mut xs[0], mut xs[2])`: the elements at `idx`, all at once. The
    /// call reached each one first, so they're in range and all different.
    pub fn disjoint_mut<const N: usize>(&mut self, idx: [i64; N]) -> [&mut T; N] {
        match self.slice_mut().get_disjoint_mut(idx.map(|i| i as usize)) {
            Ok(elems) => elems,
            Err(_) => crate::unreachable(),
        }
    }

    /// `xs[i] = v`.
    #[inline]
    pub fn try_set(&mut self, i: i64, v: T) -> R<()> {
        *self.try_get_mut(i)? = v;
        Ok(())
    }

    #[inline(always)]
    pub fn set(&mut self, i: i64, v: T, loc: &'static Loc) {
        or_throw(self.try_set(i, v), loc)
    }

    // ---- changes ----

    #[inline]
    pub fn push(&mut self, v: T) {
        self.buf_mut().vec.push(v);
    }

    /// `xs.push(...others)`.
    pub fn push_all(&mut self, others: &List<T>) {
        if !others.is_empty() {
            self.buf_mut().vec.extend_from_slice(others.as_slice());
        }
    }

    pub fn try_pop(&mut self) -> R<T> {
        if self.is_empty() {
            return Err(empty_error("pop"));
        }
        let b = self.buf_mut();
        let v = b.vec.pop();
        b.trim_front();
        v.ok_or_else(|| empty_error("pop"))
    }

    #[inline(always)]
    pub fn pop(&mut self, loc: &'static Loc) -> T {
        or_throw(self.try_pop(), loc)
    }

    /// Adds `v` at the front, in the dead front when there is one.
    pub fn unshift(&mut self, v: T) {
        let b = self.buf_mut();
        if b.head > 0 {
            b.head -= 1;
            b.vec[b.head] = v;
        } else {
            b.vec.insert(0, v);
        }
    }

    /// `xs.unshift(...others)`: `others` keep their order at the front.
    pub fn unshift_all(&mut self, others: &List<T>) {
        let items = others.as_slice();
        if items.is_empty() {
            return;
        }
        let b = self.buf_mut();
        if b.head >= items.len() {
            b.head -= items.len();
            let head = b.head;
            b.vec[head..head + items.len()].clone_from_slice(items);
        } else {
            let head = b.head;
            b.vec.splice(head..head, items.iter().cloned());
        }
    }

    pub fn try_shift(&mut self) -> R<T>
    where
        T: Placeholder,
    {
        if self.is_empty() {
            return Err(empty_error("shift"));
        }
        let b = self.buf_mut();
        let v = std::mem::replace(&mut b.vec[b.head], T::placeholder());
        b.head += 1;
        b.trim_front();
        Ok(v)
    }

    #[inline(always)]
    pub fn shift(&mut self, loc: &'static Loc) -> T
    where
        T: Placeholder,
    {
        or_throw(self.try_shift(), loc)
    }

    /// `xs.insert(i, v)`: before index `i`; `i == length` appends.
    pub fn try_insert(&mut self, i: i64, v: T) -> R<()> {
        let len = self.as_slice().len();
        match usize::try_from(i) {
            Ok(0) => self.unshift(v),
            Ok(j) if j <= len => {
                let b = self.buf_mut();
                b.vec.insert(b.head + j, v);
            }
            _ => return Err(index_error(i, len)),
        }
        Ok(())
    }

    #[inline(always)]
    pub fn insert(&mut self, i: i64, v: T, loc: &'static Loc) {
        or_throw(self.try_insert(i, v), loc)
    }

    pub fn try_remove_at(&mut self, i: i64) -> R<T>
    where
        T: Placeholder,
    {
        let len = self.as_slice().len();
        match usize::try_from(i) {
            Ok(0) if len > 0 => self.try_shift(),
            Ok(j) if j < len => {
                let b = self.buf_mut();
                let v = b.vec.remove(b.head + j);
                b.trim_front();
                Ok(v)
            }
            _ => Err(index_error(i, len)),
        }
    }

    #[inline(always)]
    pub fn remove_at(&mut self, i: i64, loc: &'static Loc) -> T
    where
        T: Placeholder,
    {
        or_throw(self.try_remove_at(i), loc)
    }

    /// Removes the first element equal to `item`.
    pub fn remove(&mut self, item: &T) -> bool
    where
        T: Equal + Placeholder,
    {
        let i = self.index_of(item);
        i >= 0 && self.try_remove_at(i).is_ok()
    }

    pub fn reverse(&mut self) {
        if self.as_slice().len() > 1 {
            self.slice_mut().reverse();
        }
    }

    // ---- sorting ----

    /// `xs.sort()`: stable, in place, in the natural order. It can't throw,
    /// so it needs no copy.
    pub fn sort(&mut self)
    where
        T: Order,
    {
        if self.as_slice().len() > 1 {
            stable_sort(self.slice_mut(), |a, b| a.order(b));
        }
    }

    /// `xs.sort(compare)`. It sorts a copy and swaps it in only when the sort
    /// finishes, so a comparator that fails leaves the list as it was. In
    /// the interpreter the comparator returns the error: it isn't called
    /// again, and the error is returned at the end. In generated code it
    /// unwinds out of the sort, and the copy is dropped on the way.
    pub fn try_sort_by(&mut self, mut compare: impl FnMut(&T, &T) -> R<i64>) -> R<()> {
        if self.as_slice().len() < 2 {
            return Ok(());
        }
        let mut copy = self.to_vec();
        let mut failed = None;
        stable_sort(&mut copy, |a, b| call_compare(&mut compare, &mut failed, a, b));
        if let Some(e) = failed {
            return Err(e);
        }
        self.replace_with(copy);
        Ok(())
    }

    /// `xs.sortBy(key)`: calls `key` once per element, in order, then sorts
    /// stably by the keys. Nothing changes until every key is known.
    pub fn try_sort_by_key<K: Order>(&mut self, key: impl FnMut(&T) -> R<K>) -> R<()> {
        if let Some(sorted) = self.sorted_by_key(key)? {
            self.replace_with(sorted);
        }
        Ok(())
    }

    /// The elements in the order of their keys, or `None` when they're in
    /// that order already. Each element is sorted together with its key, so
    /// no pass afterwards has to move the elements to their places.
    fn sorted_by_key<K: Order>(&self, mut key: impl FnMut(&T) -> R<K>) -> R<Option<Vec<T>>> {
        let s = self.as_slice();
        let mut pairs = Vec::with_capacity(s.len());
        for x in s {
            pairs.push((key(x)?, x.clone()));
        }
        if pairs.is_sorted_by(|a, b| a.0.order(&b.0) != Ordering::Greater) {
            return Ok(None);
        }
        stable_sort(&mut pairs, |a, b| a.0.order(&b.0));
        Ok(Some(pairs.into_iter().map(|(_, x)| x).collect()))
    }

    pub fn to_sorted(&self) -> List<T>
    where
        T: Order,
    {
        let mut out = self.clone();
        out.sort();
        out
    }

    pub fn try_to_sorted_by(&self, compare: impl FnMut(&T, &T) -> R<i64>) -> R<List<T>> {
        let mut out = self.clone();
        out.try_sort_by(compare)?;
        Ok(out)
    }

    pub fn try_to_sorted_by_key<K: Order>(&self, key: impl FnMut(&T) -> R<K>) -> R<List<T>> {
        Ok(match self.sorted_by_key(key)? {
            Some(sorted) => List::from_vec(sorted),
            None => self.clone(),
        })
    }

    // ---- new lists ----

    /// `xs.slice(start, end)`. A call without `end` passes `int.MAX`.
    pub fn slice(&self, start: i64, end: i64) -> List<T> {
        let s = self.as_slice();
        let (a, b) = (position(start, s.len()), position(end, s.len()));
        if a == 0 && b == s.len() {
            self.clone()
        } else if a < b {
            List::from_vec(s[a..b].to_vec())
        } else {
            List::new()
        }
    }

    pub fn concat(&self, other: &List<T>) -> List<T> {
        if other.is_empty() {
            return self.clone();
        }
        if self.is_empty() {
            return other.clone();
        }
        let (a, b) = (self.as_slice(), other.as_slice());
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        List::from_vec(out)
    }

    /// `filter` on a list nothing else holds, such as the one `map` just
    /// made: the elements that stay are kept in its own buffer, in order.
    pub fn try_filter_owned(mut self, mut f: impl FnMut(&T, i64) -> R<bool>) -> R<List<T>> {
        let Some(b) = self.0.as_mut().and_then(Shared::get_mut) else { return self.try_filter(f) };
        b.vec.drain(..b.head);
        b.head = 0;
        let (mut i, mut failed) = (0, None);
        b.vec.retain(|x| {
            if failed.is_some() {
                return true;
            }
            i += 1;
            f(x, i - 1).unwrap_or_else(|e| {
                failed = Some(e);
                true
            })
        });
        match failed {
            Some(e) => Err(e),
            None => Ok(self),
        }
    }

    /// `xs.sort(compare)` with a comparator that can't throw or print, as
    /// generated code finds: the list is sorted in place, since there's no
    /// failure to leave it unchanged for.
    pub fn sort_by_total(&mut self, mut compare: impl FnMut(&T, &T) -> i64) {
        stable_sort(self.slice_mut(), |a, b| compare(a, b).cmp(&0));
    }

    /// `xs.sortBy(key)` with a key that can't throw or print and costs
    /// little, as generated code finds: the key is worked out in each
    /// comparison, as Rust's `sort_by_key` does, which nothing can tell
    /// from working it out once per element.
    pub fn sort_by_total_key<K: Order>(&mut self, mut key: impl FnMut(&T) -> K) {
        stable_sort(self.slice_mut(), |a, b| key(a).order(&key(b)));
    }

    pub fn try_filter(&self, mut f: impl FnMut(&T, i64) -> R<bool>) -> R<List<T>> {
        let mut out = Vec::new();
        for (i, x) in self.iter().enumerate() {
            if f(x, i as i64)? {
                out.push(x.clone());
            }
        }
        Ok(List::from_vec(out))
    }

    /// Each element paired with its index.
    pub fn entries(&self) -> List<(i64, T)> {
        self.iter().enumerate().map(|(i, x)| (i as i64, x.clone())).collect()
    }

    pub fn to_reversed(&self) -> List<T> {
        self.iter().rev().cloned().collect()
    }

    /// The first occurrence of each value, in order.
    pub fn unique(&self) -> List<T>
    where
        T: DatHash,
    {
        let s = self.as_slice();
        // Indexes into `s`, with their hashes, so nothing is copied twice.
        let mut seen = hashbrown::HashTable::<(u64, usize)>::new();
        let mut out = Vec::new();
        for (i, x) in s.iter().enumerate() {
            let hash = crate::map::hash_of(x);
            if let hashbrown::hash_table::Entry::Vacant(e) = seen.entry(hash, |&(h, j)| h == hash && s[j].equal(x), |&(h, _)| h) {
                e.insert((hash, i));
                out.push(x.clone());
            }
        }
        if out.len() == s.len() { self.clone() } else { List::from_vec(out) }
    }

    /// `groupBy(key)`: groups keep their elements' order, and keys come in
    /// the order they're first seen.
    pub fn try_group_by<K: DatHash + Clone>(&self, mut key: impl FnMut(&T) -> R<K>) -> R<Map<K, List<T>>> {
        let mut groups = Map::new();
        for x in self.iter() {
            groups.get_or_insert_with(key(x)?, List::new).push(x.clone());
        }
        Ok(groups)
    }
}

impl<U: Clone> List<List<U>> {
    /// Joins a list of lists into one list.
    pub fn flat(&self) -> List<U> {
        let mut out = Vec::with_capacity(self.iter().map(|l| l.as_slice().len()).sum());
        for l in self.iter() {
            out.extend_from_slice(l.as_slice());
        }
        List::from_vec(out)
    }
}

// ---- sorting with a comparator that may fail or contradict itself ----

/// The message of the panic Rust's stable sort raises when it notices that
/// a comparator isn't a total order (`core::slice::sort`, rustc 1.99.0; only
/// for 21 or more elements). In dat that's no error: the sort still keeps
/// every element (`reference/10-list-methods.md`). A unit test pins the text,
/// so a toolchain that changes it fails loudly.
pub(crate) const SORT_ORDER_PANIC: &str = "user-provided comparison function does not correctly implement a total order";

/// Whether a panic payload is [`SORT_ORDER_PANIC`].
pub(crate) fn is_sort_order_panic(payload: &(dyn Any + Send)) -> bool {
    let message = match payload.downcast_ref::<&'static str>() {
        Some(s) => Some(*s),
        None => payload.downcast_ref::<String>().map(String::as_str),
    };
    message == Some(SORT_ORDER_PANIC)
}

/// Rust's stable sort, except that a comparator that turns out not to be a
/// total order isn't an error: the sort stops, and `v` still holds every
/// element. Any other panic, such as a dat error unwinding out of the
/// comparator, keeps unwinding.
fn stable_sort<T>(v: &mut [T], compare: impl FnMut(&T, &T) -> Ordering) {
    if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| v.sort_by(compare)))
        && !is_sort_order_panic(&*payload)
    {
        panic::resume_unwind(payload);
    }
}

/// Calls a comparator that may fail. After the first failure it isn't called
/// again: every pair is `Equal`, so the sort finishes quickly, and the caller
/// returns the error.
#[inline(always)]
fn call_compare<T>(compare: &mut impl FnMut(&T, &T) -> R<i64>, failed: &mut Option<Box<DatError>>, a: &T, b: &T) -> Ordering {
    if failed.is_some() {
        return Ordering::Equal;
    }
    match compare(a, b) {
        Ok(n) => n.cmp(&0),
        Err(e) => {
            *failed = Some(e);
            Ordering::Equal
        }
    }
}

// ---- traits ----

impl<T> Clone for List<T> {
    #[inline]
    fn clone(&self) -> List<T> {
        List(self.0.clone())
    }
}

impl<T> Default for List<T> {
    fn default() -> List<T> {
        List::new()
    }
}

impl<T> FromIterator<T> for List<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> List<T> {
        List::from_vec(iter.into_iter().collect())
    }
}

impl<'a, T> IntoIterator for &'a List<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> std::slice::Iter<'a, T> {
        self.iter()
    }
}

impl<T: Show> Show for List<T> {
    fn show(&self, out: &mut String) {
        out.push('[');
        for (i, x) in self.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            x.show(out);
        }
        out.push(']');
    }
}

impl<T: Equal> Equal for List<T> {
    fn equal(&self, other: &List<T>) -> bool {
        let (a, b) = (self.as_slice(), other.as_slice());
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.equal(y))
    }
}

impl<T: DatHash> DatHash for List<T> {
    fn dat_hash<H: std::hash::Hasher>(&self, h: &mut H) {
        // The length first, so nested lists can't run into each other.
        h.write_usize(self.as_slice().len());
        for x in self.iter() {
            x.dat_hash(h);
        }
    }
}

impl<T> Placeholder for List<T> {
    #[inline(always)]
    fn placeholder() -> List<T> {
        List::new()
    }
}

impl<T: Equal> PartialEq for List<T> {
    fn eq(&self, other: &List<T>) -> bool {
        self.equal(other)
    }
}

impl<T: Show> fmt::Debug for List<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::debug(self))
    }
}

// ---- range ----

/// `range(start, end, step)` in a `for...of` loop: the numbers, without a list.
#[derive(Clone, Debug)]
pub struct Range {
    next: i64,
    step: i64,
    left: u64,
}

impl Range {
    /// How many numbers are left.
    pub fn count_left(&self) -> u64 {
        self.left
    }
}

impl Iterator for Range {
    type Item = i64;

    #[inline]
    fn next(&mut self) -> Option<i64> {
        if self.left == 0 {
            return None;
        }
        let v = self.next;
        self.left -= 1;
        // Wraps only past the last number, which is never read.
        self.next = v.wrapping_add(self.step);
        Some(v)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match usize::try_from(self.left) {
            Ok(n) => (n, Some(n)),
            Err(_) => (usize::MAX, None),
        }
    }
}

/// `range(start, end, step)` as an iterator. Callers pass `start = 0` and
/// `step = 1` when the call leaves them out. A negative step counts down.
pub fn try_range_iter(start: i64, end: i64, step: i64) -> R<Range> {
    if step == 0 {
        return Err(range_error("range step can't be 0".to_string()));
    }
    // In i128, so no step can overflow.
    let (s, e, d) = (start as i128, end as i128, step as i128);
    let n = if d > 0 { (e - s + d - 1) / d } else { (s - e - d - 1) / -d };
    Ok(Range { next: start, step, left: n.max(0) as u64 })
}

#[inline(always)]
pub fn range_iter(start: i64, end: i64, step: i64, loc: &'static Loc) -> Range {
    or_throw(try_range_iter(start, end, step), loc)
}

/// `range(start, end, step)` as a list.
pub fn try_range(start: i64, end: i64, step: i64) -> R<List<i64>> {
    let numbers = try_range_iter(start, end, step)?;
    let mut out = match usize::try_from(numbers.count_left()) {
        Ok(n) if n <= crate::MAX_LEN => Vec::with_capacity(n),
        _ => return Err(range_error(format!("list too long: range() would have {} elements", numbers.count_left()))),
    };
    out.extend(numbers);
    Ok(List::from_vec(out))
}

#[inline(always)]
pub fn range(start: i64, end: i64, step: i64, loc: &'static Loc) -> List<i64> {
    or_throw(try_range(start, end, step), loc)
}

// ---- destructuring ----

/// Destructuring `needed` elements, not counting `...rest`, from a list of
/// length `len`.
#[inline]
pub fn try_check_len(len: i64, needed: i64) -> R<()> {
    if len >= needed { Ok(()) } else { Err(too_short(len, needed)) }
}

#[inline(always)]
pub fn check_len(len: i64, needed: i64, loc: &'static Loc) {
    or_throw(try_check_len(len, needed), loc)
}

#[cold]
#[inline(never)]
fn too_short(len: i64, needed: i64) -> Box<DatError> {
    let s = if needed == 1 { "" } else { "s" };
    range_error(format!("can't take {needed} element{s} from a list of length {len}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ALL_ERRORS, Set, catch, debug, done};
    use std::cell::Cell;
    use std::time::{Duration, Instant};

    fn list<T>(v: Vec<T>) -> List<T> {
        List::from_vec(v)
    }

    fn ints(xs: &[i64]) -> List<i64> {
        List::from_vec(xs.to_vec())
    }

    fn strs(xs: &[&str]) -> List<Str> {
        xs.iter().map(|s| Str::new(s)).collect()
    }

    fn err<T: fmt::Debug>(r: R<T>) -> String {
        let e = r.unwrap_err();
        format!("{}: {}", e.kind.name(), e.message)
    }

    #[test]
    fn disjoint_elements_change_together() {
        let mut xs = ints(&[0, 1, 2, 3]);
        xs.try_shift().unwrap();
        let before = xs.clone();
        let [a, b] = xs.disjoint_mut([0, 2]);
        std::mem::swap(a, b);
        assert_eq!(debug(&xs), "[3, 2, 1]");
        assert_eq!(debug(&before), "[1, 2, 3]");
    }

    /// A comparator that contradicts itself, deterministically.
    fn inconsistent() -> impl FnMut(&i64, &i64) -> Ordering {
        let mut state: u64 = 12345;
        move |_, _| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            if state >> 63 == 0 { Ordering::Less } else { Ordering::Greater }
        }
    }

    fn as_ordering_int(o: Ordering) -> i64 {
        o as i64
    }

    #[test]
    fn printing() {
        assert_eq!(debug(&ints(&[1, 2, 3])), "[1, 2, 3]");
        assert_eq!(debug(&List::<i64>::new()), "[]");
        assert_eq!(debug(&strs(&["a", "b\"c"])), r#"["a", "b\"c"]"#);
        assert_eq!(debug(&list(vec![2.0, 0.5, f64::NAN])), "[2.0, 0.5, NaN]");
        assert_eq!(debug(&list(vec![ints(&[1, 2]), ints(&[])])), "[[1, 2], []]");
        assert_eq!(debug(&list(vec![(1i64, Str::new("a")), (2, Str::new("b"))])), r#"[[1, "a"], [2, "b"]]"#);
        assert_eq!(crate::text(&strs(&["a"])).as_str(), r#"["a"]"#);
        assert_eq!(strs(&["a", "b"]).join(", ").as_str(), "a, b");
        assert_eq!(list(vec![1.0, 2.5]).join("-").as_str(), "1.0-2.5");
        assert_eq!(list(vec![strs(&["x"])]).join(",").as_str(), r#"["x"]"#);
    }

    #[test]
    fn equality() {
        assert!(ints(&[1, 2]).equal(&ints(&[1, 2])));
        assert!(!ints(&[1, 2]).equal(&ints(&[2, 1])));
        assert!(!ints(&[1]).equal(&ints(&[1, 1])));
        assert!(!list(vec![f64::NAN]).equal(&list(vec![f64::NAN])));
        let nan = list(vec![f64::NAN]);
        assert!(!nan.equal(&nan.clone()), "a shared buffer still compares by IEEE 754");
        assert!(list(vec![ints(&[1]), ints(&[])]).equal(&list(vec![ints(&[1]), ints(&[])])));
        assert!(List::<i64>::new().equal(&List::with_capacity(4)));
    }

    #[test]
    fn index_errors() {
        let xs = strs(&["red", "green", "blue"]);
        assert_eq!(xs.try_get(0).unwrap().as_str(), "red");
        assert_eq!(err(xs.try_get(5)), "RangeError: index 5 out of range for length 3");
        assert_eq!(err(xs.try_get(-1)), "RangeError: index -1 out of range for length 3");
        assert_eq!(xs.try_at(-1).unwrap().as_str(), "blue");
        assert_eq!(xs.try_at(-3).unwrap().as_str(), "red");
        assert_eq!(err(xs.try_at(-4)), "RangeError: index -4 out of range for length 3");
        assert_eq!(err(xs.try_at(3)), "RangeError: index 3 out of range for length 3");
        assert_eq!(err(xs.try_at(i64::MIN)), format!("RangeError: index {} out of range for length 3", i64::MIN));
        let mut ys = xs.clone();
        assert_eq!(err(ys.try_set(3, Str::new("x"))), "RangeError: index 3 out of range for length 3");
        ys.try_set(1, Str::new("yellow")).unwrap();
        *ys.try_get_mut(2).unwrap() = Str::new("navy");
        assert_eq!(debug(&ys), r#"["red", "yellow", "navy"]"#);
        assert_eq!(debug(&xs), r#"["red", "green", "blue"]"#);
        let mut e = List::<i64>::new();
        assert_eq!(err(e.try_pop()), "RangeError: pop() on an empty list");
        assert_eq!(err(e.try_shift()), "RangeError: shift() on an empty list");
        assert_eq!(err(e.try_remove_at(0)), "RangeError: index 0 out of range for length 0");
        assert_eq!(err(e.try_insert(1, 5)), "RangeError: index 1 out of range for length 0");
        assert_eq!(err(try_check_len(2, 3)), "RangeError: can't take 3 elements from a list of length 2");
        assert_eq!(err(try_check_len(0, 1)), "RangeError: can't take 1 element from a list of length 0");
        assert!(try_check_len(3, 3).is_ok());
    }

    #[test]
    fn changes() {
        let mut xs = ints(&[1, 2, 3]);
        xs.try_insert(3, 4).unwrap();
        xs.try_insert(0, 0).unwrap();
        xs.try_insert(2, 9).unwrap();
        assert_eq!(err(xs.try_insert(7, 1)), "RangeError: index 7 out of range for length 6");
        assert_eq!(debug(&xs), "[0, 1, 9, 2, 3, 4]");
        assert_eq!(xs.try_remove_at(2).unwrap(), 9);
        assert_eq!(err(xs.try_remove_at(5)), "RangeError: index 5 out of range for length 5");
        assert_eq!(xs.try_shift().unwrap(), 0);
        assert_eq!(xs.try_pop().unwrap(), 4);
        xs.unshift(7);
        xs.unshift_all(&ints(&[5, 6]));
        xs.push_all(&ints(&[8, 9]));
        assert_eq!(debug(&xs), "[5, 6, 7, 1, 2, 3, 8, 9]");
        // The dead front takes elements added at the front.
        assert_eq!(xs.try_shift().unwrap(), 5);
        assert_eq!(xs.try_shift().unwrap(), 6);
        xs.unshift_all(&ints(&[-2, -1]));
        xs.unshift_all(&ints(&[-5, -4, -3]));
        assert_eq!(debug(&xs), "[-5, -4, -3, -2, -1, 7, 1, 2, 3, 8, 9]");
        assert!(xs.remove(&7));
        assert!(!xs.remove(&7));
        assert!(xs.remove(&-5));
        xs.reverse();
        assert_eq!(debug(&xs), "[9, 8, 3, 2, 1, -1, -2, -3, -4]");
        let mut self_push = ints(&[1, 2]);
        let copy = self_push.clone();
        self_push.push_all(&copy);
        assert_eq!(debug(&self_push), "[1, 2, 1, 2]");
        xs.clear();
        assert!(xs.is_empty());
        xs.push(1);
        assert_eq!(debug(&xs), "[1]");
    }

    #[test]
    fn reading() {
        let xs = ints(&[3, 1, 4, 1, 5]);
        assert!(xs.includes(&4));
        assert_eq!(xs.index_of(&1), 1);
        assert_eq!(xs.last_index_of(&1), 3);
        assert_eq!(xs.index_of(&7), -1);
        assert_eq!(debug(&xs.slice(1, 3)), "[1, 4]");
        assert_eq!(debug(&xs.slice(-2, i64::MAX)), "[1, 5]");
        assert_eq!(debug(&xs.slice(3, 1)), "[]");
        assert_eq!(debug(&xs.slice(-100, 100)), "[3, 1, 4, 1, 5]");
        assert_eq!(debug(&xs.concat(&ints(&[9]))), "[3, 1, 4, 1, 5, 9]");
        assert_eq!(debug(&xs.unique()), "[3, 1, 4, 5]");
        assert_eq!(debug(&xs.entries()), "[[0, 3], [1, 1], [2, 4], [3, 1], [4, 5]]");
        assert_eq!(debug(&xs.to_reversed()), "[5, 1, 4, 1, 3]");
        assert_eq!(debug(&xs.zip(&strs(&["a", "b"]))), r#"[[3, "a"], [1, "b"]]"#);
        assert_eq!(debug(&list(vec![ints(&[1]), ints(&[]), ints(&[2, 3])]).flat()), "[1, 2, 3]");
        assert_eq!(debug(&strs(&["b", "a", "b"]).unique()), r#"["b", "a"]"#);
        assert_eq!(debug(&list(vec![ints(&[1]), ints(&[1]), ints(&[1, 2])]).unique()), "[[1], [1, 2]]");
    }

    #[test]
    fn callbacks_in_order_with_index() {
        let xs = ints(&[3, 1, 4]);
        let seen = std::cell::RefCell::new(Vec::new());
        let doubled = xs
            .try_map(|x, i| {
                seen.borrow_mut().push((*x, i));
                Ok(x * 2)
            })
            .unwrap();
        assert_eq!(debug(&doubled), "[6, 2, 8]");
        assert_eq!(*seen.borrow(), vec![(3, 0), (1, 1), (4, 2)]);
        assert_eq!(debug(&xs.try_filter(|x, i| Ok(x % 2 == 1 || i == 2)).unwrap()), "[3, 1, 4]");
        assert_eq!(debug(&xs.try_filter(|x, _| Ok(*x > 2)).unwrap()), "[3, 4]");
        assert_eq!(debug(&xs.try_flat_map(|x, _| Ok(ints(&[*x, *x]))).unwrap()), "[3, 3, 1, 1, 4, 4]");
        assert_eq!(xs.try_reduce(0, |acc, x| Ok(acc + x)).unwrap(), 8);
        assert_eq!(xs.try_find(|x, _| Ok(*x > 3), 0).unwrap(), 4);
        assert_eq!(xs.try_find(|x, _| Ok(*x > 9), 0).unwrap(), 0);
        assert_eq!(xs.try_find_index(|x, _| Ok(*x == 1)).unwrap(), 1);
        assert_eq!(xs.try_find_index(|x, _| Ok(*x == 9)).unwrap(), -1);
        assert!(xs.try_some(|x, _| Ok(*x == 4)).unwrap());
        assert!(!xs.try_every(|x, _| Ok(*x > 1)).unwrap());
        assert_eq!(xs.try_count(|x, _| Ok(*x > 1)).unwrap(), 2);
        let mut sum = 0;
        xs.try_for_each(|x, i| {
            sum += x * i;
            Ok(())
        })
        .unwrap();
        assert_eq!(sum, 1 + 8);
        // A callback that fails stops the method there.
        let calls = Cell::new(0);
        let r = xs.try_map(|x, _| {
            calls.set(calls.get() + 1);
            if *x == 1 { Err(Box::new(DatError::new(ErrorKind::Error, "bad"))) } else { Ok(*x) }
        });
        assert_eq!(err(r), "Error: bad");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn group_by_keeps_element_and_key_order() {
        let words = strs(&["bob", "al", "cy", "dan", "ed", "x"]);
        let groups = words.try_group_by(|w| Ok(w.as_str().len() as i64)).unwrap();
        assert_eq!(debug(&groups), r#"{3: ["bob", "dan"], 2: ["al", "cy", "ed"], 1: ["x"]}"#);
    }

    #[test]
    fn copy_on_write() {
        let mut a = ints(&[1, 2, 3]);
        let mut b = a.clone();
        assert!(Shared::ptr_eq(a.buffer().unwrap(), b.buffer().unwrap()));
        b.push(4);
        assert!(!Shared::ptr_eq(a.buffer().unwrap(), b.buffer().unwrap()));
        assert_eq!(debug(&a), "[1, 2, 3]");
        assert_eq!(debug(&b), "[1, 2, 3, 4]");
        // Changing a list nothing else shares doesn't copy it.
        let before = Shared::as_ptr(a.buffer().unwrap());
        a.push(5);
        *a.try_get_mut(0).unwrap() = 9;
        a.sort();
        a.try_sort_by_key(|x| Ok(-x)).unwrap();
        a.reverse();
        a.try_shift().unwrap();
        assert_eq!(Shared::as_ptr(a.buffer().unwrap()), before);
        assert_eq!(debug(&a), "[3, 5, 9]");
        // A copy holds only the live part.
        let mut q = ints(&[1, 2, 3, 4]);
        q.try_shift().unwrap();
        let mut r = q.clone();
        r.push(5);
        assert_eq!(r.buffer().unwrap().head, 0);
        assert_eq!(r.buffer().unwrap().vec, vec![2, 3, 4, 5]);
        assert_eq!(debug(&q), "[2, 3, 4]");
        // A loop goes through the list as it was when it started.
        let mut xs = ints(&[1, 2, 3]);
        let snapshot = xs.clone();
        for x in snapshot.as_slice() {
            xs.push(*x * 10);
        }
        assert_eq!(debug(&xs), "[1, 2, 3, 10, 20, 30]");
        // Empty lists and placeholders own no buffer.
        assert!(List::<i64>::new().buffer().is_none());
        assert!(List::<Str>::placeholder().buffer().is_none());
    }

    #[test]
    fn shift_in_a_loop_stays_linear() {
        // A breadth-first walk: a queue of a million, each shift pushing up
        // to two more. With `remove(0)` this would take minutes.
        let n = 1_000_000;
        let start = Instant::now();
        let mut queue = ints(&[0]);
        let mut visited = 0i64;
        while let Ok(node) = queue.try_shift() {
            visited += 1;
            for child in [2 * node + 1, 2 * node + 2] {
                if child < n {
                    queue.push(child);
                }
            }
        }
        assert_eq!(visited, n);
        // And a long queue of lists, drained from the front.
        let mut lists: List<List<i64>> = (0..n).map(|i| ints(&[i])).collect();
        let mut sum = 0;
        while let Ok(l) = lists.try_shift() {
            sum += l.try_get(0).unwrap();
        }
        assert_eq!(sum, n * (n - 1) / 2);
        assert!(start.elapsed() < Duration::from_secs(5), "took {:?}", start.elapsed());
    }

    #[test]
    fn sort_is_stable_and_natural() {
        let mut xs = list(vec![3.0, f64::NAN, -0.0, 1.0, 0.0, f64::NEG_INFINITY]);
        xs.sort();
        assert_eq!(debug(&xs), "[-Infinity, -0.0, 0.0, 1.0, 3.0, NaN]");
        let mut ys = list(vec![0.0, -0.0]);
        ys.sort();
        assert_eq!(debug(&ys), "[0.0, -0.0]");
        let mut words = strs(&["pear", "fig", "apple", "kiwi", "date"]);
        words.sort();
        assert_eq!(debug(&words), r#"["apple", "date", "fig", "kiwi", "pear"]"#);
        // By length: ties keep their order.
        let by_len = strs(&["pear", "fig", "apple", "kiwi", "date", "yew"]);
        let sorted = by_len.try_to_sorted_by(|a, b| Ok(a.as_str().len() as i64 - b.as_str().len() as i64)).unwrap();
        assert_eq!(debug(&sorted), r#"["fig", "yew", "pear", "kiwi", "date", "apple"]"#);
        let sorted = by_len.try_to_sorted_by_key(|w| Ok(w.as_str().len() as i64)).unwrap();
        assert_eq!(debug(&sorted), r#"["fig", "yew", "pear", "kiwi", "date", "apple"]"#);
        assert_eq!(debug(&by_len), r#"["pear", "fig", "apple", "kiwi", "date", "yew"]"#);
        // Stable over many equal keys.
        let pairs: List<(i64, i64)> = (0..1000).map(|i| (i % 7, i)).collect();
        let sorted = pairs.try_to_sorted_by_key(|p| Ok(p.0)).unwrap();
        assert!(sorted.as_slice().windows(2).all(|w| w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1)));
        let sorted = pairs.try_to_sorted_by(|a, b| Ok(a.0 - b.0)).unwrap();
        assert!(sorted.as_slice().windows(2).all(|w| w[0].0 < w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1)));
        assert_eq!(debug(&ints(&[3, 1, 2]).to_sorted()), "[1, 2, 3]");
    }

    #[test]
    fn sort_by_key_calls_the_key_once_per_element_in_order() {
        let mut xs = ints(&[5, 3, 9, 1, 7]);
        let mut seen = Vec::new();
        xs.try_sort_by_key(|x| {
            seen.push(*x);
            Ok(*x)
        })
        .unwrap();
        assert_eq!(seen, vec![5, 3, 9, 1, 7]);
        assert_eq!(debug(&xs), "[1, 3, 5, 7, 9]");
        let mut one = ints(&[4]);
        let mut calls = 0;
        one.try_sort_by_key(|x| {
            calls += 1;
            Ok(*x)
        })
        .unwrap();
        assert_eq!(calls, 1);
        // A key that fails leaves the list as it was.
        let r = xs.try_sort_by_key(|x| if *x == 7 { Err(Box::new(DatError::new(ErrorKind::ValueError, "no"))) } else { Ok(-x) });
        assert_eq!(err(r), "ValueError: no");
        assert_eq!(debug(&xs), "[1, 3, 5, 7, 9]");
    }

    #[test]
    fn rust_sort_panics_with_the_message_dat_matches() {
        // If a toolchain changes this, `stable_sort` and the panic hook stop
        // recognizing the panic, and this test says so.
        let mut v: Vec<i64> = (0..100).collect();
        let mut compare = inconsistent();
        let payload = panic::catch_unwind(AssertUnwindSafe(|| v.sort_by(&mut compare))).unwrap_err();
        assert!(is_sort_order_panic(&*payload), "{:?}", payload.downcast_ref::<&str>());
        assert!(!is_sort_order_panic(&"something else"));
        assert!(is_sort_order_panic(&SORT_ORDER_PANIC.to_string()));
    }

    #[test]
    fn a_failing_comparator_leaves_the_list_unchanged() {
        let original: Vec<i64> = (0..200).rev().collect();
        let mut xs = list(original.clone());
        let calls_after = Cell::new(0);
        let failed = Cell::new(false);
        let r = xs.try_sort_by(|a, b| {
            if failed.get() {
                calls_after.set(calls_after.get() + 1);
            }
            if *a == 50 || *b == 50 {
                failed.set(true);
                return Err(Box::new(DatError::new(ErrorKind::ValueError, "can't compare 50")));
            }
            Ok(a - b)
        });
        assert_eq!(err(r), "ValueError: can't compare 50");
        assert_eq!(calls_after.get(), 0, "the comparator isn't called after it fails");
        assert_eq!(xs.to_vec(), original);
        let r = xs.try_to_sorted_by(|_, _| Err(Box::new(DatError::new(ErrorKind::Error, "x"))));
        assert!(r.is_err());
        assert_eq!(xs.to_vec(), original);
    }

    #[test]
    fn an_inconsistent_comparator_is_no_error_and_keeps_every_element() {
        for n in [100, 1000] {
            let mut xs: List<i64> = (0..n).map(|i| (i * 37) % n).collect();
            let mut compare = inconsistent();
            xs.try_sort_by(|a, b| Ok(as_ordering_int(compare(a, b)))).unwrap();
            let mut kept = xs.to_vec();
            kept.sort();
            assert_eq!(kept, (0..n).collect::<Vec<_>>());
        }
        let mut xs: List<i64> = (0..100).collect();
        let mut compare = inconsistent();
        let sorted = xs.try_to_sorted_by(|a, b| Ok(as_ordering_int(compare(a, b)))).unwrap();
        assert_eq!(sorted.len(), 100);
        xs.sort();
        assert_eq!(xs.to_vec(), (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn unwinding_comparators_as_generated_code_runs_them() {
        // Generated code's callbacks return `Ok` and throw by unwinding with a
        // `DatError`; `done` unwraps the method's own result.
        let original: Vec<i64> = (0..150).map(|i| (i * 7) % 150).collect();
        let mut xs = list(original.clone());
        let r = catch(ALL_ERRORS, || {
            done(xs.try_sort_by(|a, b| {
                if *a == 3 || *b == 3 {
                    crate::throw_at(Box::new(DatError::new(ErrorKind::RangeError, "thrown in compare")), None);
                }
                Ok(a - b)
            }))
        });
        assert_eq!(err(r), "RangeError: thrown in compare");
        assert_eq!(xs.to_vec(), original, "the list is unchanged");
        let r = catch(ALL_ERRORS, || {
            done(xs.try_sort_by_key(|x| {
                if *x == 3 {
                    crate::throw_at(Box::new(DatError::new(ErrorKind::RangeError, "thrown in key")), None);
                }
                Ok(*x)
            }))
        });
        assert_eq!(err(r), "RangeError: thrown in key");
        assert_eq!(xs.to_vec(), original);
        // An inconsistent comparator still finishes without an error.
        let mut compare = inconsistent();
        catch(ALL_ERRORS, || done(xs.try_sort_by(|a, b| Ok(as_ordering_int(compare(a, b)))))).unwrap();
        let mut kept = xs.to_vec();
        kept.sort();
        assert_eq!(kept, (0..150).collect::<Vec<_>>());
        // A method's own error goes through `done` too.
        let mut empty = List::<i64>::new();
        assert_eq!(err(catch(ALL_ERRORS, || done(empty.try_pop()))), "RangeError: pop() on an empty list");
    }

    #[test]
    fn ranges() {
        let r = |a, b, c| debug(&try_range(a, b, c).unwrap());
        assert_eq!(r(0, 5, 1), "[0, 1, 2, 3, 4]");
        assert_eq!(r(2, 5, 1), "[2, 3, 4]");
        assert_eq!(r(10, 0, -3), "[10, 7, 4, 1]");
        assert_eq!(r(0, 0, 1), "[]");
        assert_eq!(r(5, 0, 1), "[]");
        assert_eq!(r(0, 5, -1), "[]");
        assert_eq!(r(0, 10, 4), "[0, 4, 8]");
        assert_eq!(r(i64::MAX - 2, i64::MAX, 1), format!("[{}, {}]", i64::MAX - 2, i64::MAX - 1));
        assert_eq!(r(i64::MAX - 5, i64::MAX, 4), format!("[{}, {}]", i64::MAX - 5, i64::MAX - 1));
        assert_eq!(r(i64::MIN + 2, i64::MIN, -1), format!("[{}, {}]", i64::MIN + 2, i64::MIN + 1));
        assert_eq!(r(i64::MIN, i64::MAX, i64::MAX), format!("[{}, -1, {}]", i64::MIN, i64::MAX - 1));
        assert_eq!(r(i64::MAX, i64::MIN, i64::MIN), format!("[{}, -1]", i64::MAX));
        assert_eq!(err(try_range(0, 5, 0)), "RangeError: range step can't be 0");
        assert_eq!(err(try_range_iter(0, 5, 0)), "RangeError: range step can't be 0");
        assert_eq!(err(try_range(i64::MIN, i64::MAX, 1)), "RangeError: list too long: range() would have 18446744073709551615 elements");
        // A loop over a range builds no list.
        let mut it = try_range_iter(i64::MIN, i64::MAX, 1).unwrap();
        assert_eq!(it.count_left(), u64::MAX);
        assert_eq!(it.next(), Some(i64::MIN));
        assert_eq!(try_range_iter(0, 1_000_000, 3).unwrap().map(|i| i % 2).sum::<i64>(), 166_667);
        let near_max: Vec<i64> = try_range_iter(i64::MAX - 3, i64::MAX, 2).unwrap().collect();
        assert_eq!(near_max, vec![i64::MAX - 3, i64::MAX - 1]);
    }

    #[test]
    fn unique_uses_value_equality() {
        let xs: List<Str> = ["a", "b", "a", "c", "b"].iter().map(|s| Str::new(s)).collect();
        assert_eq!(debug(&xs.unique()), r#"["a", "b", "c"]"#);
        let set: Set<Str> = Set::from_list(&xs);
        assert_eq!(debug(&set), r#"{"a", "b", "c"}"#);
        let distinct = ints(&[1, 2, 3]);
        assert!(Shared::ptr_eq(distinct.unique().buffer().unwrap(), distinct.buffer().unwrap()));
    }
}
