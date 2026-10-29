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

    /// Joins string parts, as `+` and template strings do.
    pub fn concat(parts: &[&str]) -> Str {
        let mut s = String::with_capacity(parts.iter().map(|p| p.len()).sum());
        for p in parts {
            s.push_str(p);
        }
        Str::from_string(s)
    }

    pub fn push_str(&mut self, s: &str) {
        Shared::make_mut(&mut self.0).push_str(s);
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
