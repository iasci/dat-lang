use crate::Shared;
use std::fmt;

/// A dat `string`: immutable UTF-8 text that is cheap to copy.
///
/// Copies share one buffer. Appending to a string that nothing else shares
/// happens in place, so `s += t` in a loop stays linear.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Str(Shared<String>);

impl Str {
    pub fn new(s: &str) -> Str {
        Str(Shared::new(s.to_string()))
    }

    pub fn from_string(s: String) -> Str {
        Str(Shared::new(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn push_str(&mut self, s: &str) {
        Shared::make_mut(&mut self.0).push_str(s);
    }
}

impl crate::copies::Size for String {
    const BYTES: bool = true;

    fn size(&self) -> u64 {
        self.len() as u64
    }
}

impl fmt::Display for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&crate::debug_str(self.as_str()))
    }
}

/// Builds the string that `+` on strings and template strings produce,
/// writing each part into one buffer.
pub struct Join(String);

impl Join {
    pub fn with_capacity(bytes: usize) -> Join {
        Join(String::with_capacity(bytes))
    }

    pub fn push_str(&mut self, s: &str) {
        self.0.push_str(s);
    }

    /// `${n}`, the text `toString()` gives.
    pub fn push_int(&mut self, n: i64) {
        crate::fmt::write_int(&mut self.0, n);
    }

    /// `${x}`, the text `toString()` gives.
    pub fn push_float(&mut self, x: f64) {
        crate::fmt::write_float(&mut self.0, x);
    }

    /// `${b}`, the text `toString()` gives.
    pub fn push_bool(&mut self, b: bool) {
        self.0.push_str(crate::fmt::bool_text(b));
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn finish(self) -> Str {
        Str::from_string(self.0)
    }
}
