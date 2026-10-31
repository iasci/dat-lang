use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::Rc;

/// The one reference-counted pointer dat-rt uses, so a later switch to a
/// thread-safe count stays inside this crate. Stored function values are
/// the exception (see `func.rs`).
///
/// Like `Rc`, its operations are associated functions (`Shared::make_mut(&mut p)`),
/// so they never hide a method of the value it points to.
pub struct Shared<T>(Rc<T>);

impl<T> Shared<T> {
    #[inline]
    pub fn new(value: T) -> Shared<T> {
        Shared(Rc::new(value))
    }

    /// The value, to change in place. It is copied first when something
    /// else shares it, which `--report-copies` counts.
    #[inline]
    pub fn make_mut(this: &mut Shared<T>) -> &mut T
    where
        T: Clone + crate::copies::Size,
    {
        if Rc::strong_count(&this.0) != 1 {
            crate::copies::copied(this.0.size(), T::BYTES);
        }
        Rc::make_mut(&mut this.0)
    }

    /// The value, when nothing else shares it.
    #[inline]
    pub fn get_mut(this: &mut Shared<T>) -> Option<&mut T> {
        Rc::get_mut(&mut this.0)
    }

    #[inline]
    pub fn is_unique(this: &Shared<T>) -> bool {
        Rc::strong_count(&this.0) == 1
    }

    #[inline]
    pub fn ptr_eq(a: &Shared<T>, b: &Shared<T>) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    /// Where the value is, to tell whether a change copied it.
    #[inline]
    pub fn as_ptr(this: &Shared<T>) -> *const T {
        Rc::as_ptr(&this.0)
    }

    /// The value itself, copied only when something else shares it.
    #[inline]
    pub fn unwrap_or_clone(this: Shared<T>) -> T
    where
        T: Clone,
    {
        Rc::unwrap_or_clone(this.0)
    }
}

impl<T> Clone for Shared<T> {
    #[inline]
    fn clone(&self) -> Shared<T> {
        Shared(Rc::clone(&self.0))
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Shared<T> {
        Shared::new(T::default())
    }
}

impl<T: PartialEq> PartialEq for Shared<T> {
    #[inline]
    fn eq(&self, other: &Shared<T>) -> bool {
        *self.0 == *other.0
    }
}

impl<T: Eq> Eq for Shared<T> {}

impl<T: PartialOrd> PartialOrd for Shared<T> {
    #[inline]
    fn partial_cmp(&self, other: &Shared<T>) -> Option<std::cmp::Ordering> {
        (*self.0).partial_cmp(&*other.0)
    }
}

impl<T: Ord> Ord for Shared<T> {
    #[inline]
    fn cmp(&self, other: &Shared<T>) -> std::cmp::Ordering {
        (*self.0).cmp(&*other.0)
    }
}

impl<T: Hash> Hash for Shared<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        (*self.0).hash(state)
    }
}

impl<T: fmt::Debug> fmt::Debug for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (*self.0).fmt(f)
    }
}

impl<T: fmt::Display> fmt::Display for Shared<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (*self.0).fmt(f)
    }
}
