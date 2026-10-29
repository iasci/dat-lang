//! Running a program: a large-stack thread, the uncaught-error report and exit statuses.

use crate::{DatError, Entry, Frame, catch, flush_stdout};
use std::io::Write;

/// Runs `f` on a new thread with a `stack`-byte stack and the stack guard set
/// up for it. A Rust panic inside `f` (a bug in dat) is reported as an
/// internal error with exit status 2.
pub fn run_on_dat_thread<T: Send>(stack: usize, f: impl FnOnce() -> T + Send) -> T {
    install_panic_hook();
    let result = std::thread::scope(|s| {
        std::thread::Builder::new()
            .name("dat".into())
            .stack_size(stack)
            .spawn_scoped(s, move || {
                crate::stack::set_limit(stack);
                f()
            })
            .expect("failed to start the dat thread")
            .join()
    });
    match result {
        Ok(v) => v,
        Err(_) => exit_with(2),
    }
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        flush_stdout();
        eprintln!("internal error: {info}");
        eprintln!("This is a bug in dat, not in your program.");
    }));
}

/// Writes an error that escaped `main` to stderr, after flushing stdout.
pub fn report_uncaught(err: &DatError) {
    flush_stdout();
    let mut e = std::io::stderr().lock();
    let _ = e.write_all(err.render_uncaught().as_bytes());
    let _ = e.flush();
}

/// The entry point of a compiled program.
pub fn run_main(main: Entry) -> ! {
    let code = run_on_dat_thread(crate::STACK_SIZE, || {
        let root = Frame::ROOT;
        let code = match catch(|| main(&root, None)) {
            Ok(()) => 0,
            Err(err) => {
                report_uncaught(&err);
                1
            }
        };
        flush_stdout();
        code
    });
    exit_with(code)
}

pub fn exit_with(code: i32) -> ! {
    let _ = std::io::stdout().flush();
    std::process::exit(code)
}
