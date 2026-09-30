//! Main entry point for Zebrad

use zebrad::application::{boot, APPLICATION};

// Memory profiling
// #[cfg(not(target_env = "msvc"))]
// #[global_allocator]
// static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
// 
// // #[global_allocator]
// // static ALLOC: std::alloc::System = std::alloc::System;


// Nothing calls into it: linking it is the point. cosmo-build's linker shim
// passes `--wrap` for the libc entry points std uses, and cosmo-compat is what
// defines the matching `__wrap_*`. Without this the rlib is never referenced and
// the link fails on undefined `__wrap_mmap` and friends.
#[cfg(cosmo)]
extern crate cosmo_compat as _;

/// libc symbols the cosmo link asks for and cosmopolitan does not provide.
#[cfg(cosmo)]
#[allow(unsafe_code)]
mod cosmo_missing {
    /// tokio's signal registry sizes its table with libc::SIGRTMAX(), which on
    /// target_os = "linux" is a call to glibc's `__libc_current_sigrtmax`.
    /// Linux's value; the table only has to cover the signals tokio is asked for.
    #[unsafe(no_mangle)]
    extern "C" fn __libc_current_sigrtmax() -> std::ffi::c_int {
        64
    }

    /// rocksdb's trace replayer calls `std::llround`; cosmopolitan's libm has
    /// `lround` and `round` but not the `long long` one. Both round half away
    /// from zero, as `f64::round` does; out of range is unspecified in C.
    #[unsafe(no_mangle)]
    extern "C" fn llround(x: f64) -> std::ffi::c_longlong {
        x.round() as std::ffi::c_longlong
    }
}

/// Process entry point for `zebrad`
fn main() {
    #[cfg(cosmo)]
    if let Err(error) = zebrad::components::tokio::install_portable_signal_handlers() {
        eprintln!("failed to install shutdown signal handlers: {error}");
        std::process::exit(1);
    }

    // Enable backtraces by default for zebrad, but allow users to override it.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
        // Disable library backtraces (i.e. eyre) to avoid performance hit for
        // non-panic errors, but allow users to override it.
        if std::env::var_os("RUST_LIB_BACKTRACE").is_none() {
            std::env::set_var("RUST_LIB_BACKTRACE", "0");
        }
    }
    boot(&APPLICATION);
}
