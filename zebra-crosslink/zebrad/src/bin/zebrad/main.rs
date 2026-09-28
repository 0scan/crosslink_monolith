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

/// Process entry point for `zebrad`
fn main() {
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
