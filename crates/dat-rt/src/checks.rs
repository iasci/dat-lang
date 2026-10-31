//! `DAT_DEBUG_CHECKS=1`, which the conformance runs set: every value a
//! program makes is dropped exactly once by the time it ends. A value that
//! is never dropped would show as a block the allocator still holds; one
//! dropped twice can't happen in Rust. The other half of the check, no use
//! after a move, is static: `dat` checks the liveness pass's marks before
//! it runs anything (`dat_own::verify`).
//!
//! Only a run that ends normally is checked: printing an uncaught error or
//! a failed test reads debug info into caches that live until the process
//! ends.

use std::sync::OnceLock;

/// Whether `DAT_DEBUG_CHECKS=1` is set.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("DAT_DEBUG_CHECKS").is_some_and(|v| v == "1"))
}

/// The blocks allocated before the program started.
pub struct Window(isize);

/// Starts counting, if the checks are on. What the runtime keeps for the
/// whole run is made first, so it isn't counted.
pub fn begin() -> Option<Window> {
    if !enabled() {
        return None;
    }
    crate::output::reserve();
    drop(std::io::stdout().lock());
    drop(<crate::Str as crate::Placeholder>::placeholder());
    crate::copies::reserve();
    crate::alloc::count_blocks();
    Some(Window(crate::alloc::live_blocks()))
}

/// The status a run that started at `window` ends with: `status`, or 2
/// when it ended normally and left blocks behind.
pub fn end(window: Option<Window>, status: i32) -> i32 {
    let Some(Window(before)) = window else { return status };
    let left = crate::alloc::live_blocks() - before;
    if status != 0 || left == 0 {
        return status;
    }
    crate::flush_stdout();
    if left > 0 {
        eprintln!("internal error: the program ended with {left} allocated blocks never freed, so a value wasn't dropped");
    } else {
        eprintln!("internal error: the program freed {} blocks it didn't allocate", -left);
    }
    eprintln!("This is a bug in dat, not in your program.");
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_never_freed_fails_a_run_that_ended_normally() {
        crate::alloc::count_blocks();
        let window = Some(Window(crate::alloc::live_blocks()));
        drop(vec![1, 2, 3]);
        assert_eq!(end(window, 0), 0);
        let window = Some(Window(crate::alloc::live_blocks()));
        let kept: &'static mut Vec<i32> = Box::leak(Box::new(vec![1]));
        assert_eq!(end(window, 0), 2);
        let window = Some(Window(crate::alloc::live_blocks() - 1));
        assert_eq!(end(window, 1), 1, "a run that failed isn't checked");
        kept.push(2);
    }
}
