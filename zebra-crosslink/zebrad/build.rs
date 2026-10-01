//! Build script for zebrad.

use vergen_git2::{CargoBuilder, Emitter, Git2Builder, RustcBuilder};

/// Process entry point for `zebrad`'s build script
#[allow(clippy::print_stderr)]
fn main() {
    #[cfg(feature = "ape")]
    apeify();

    // The host decides whether embed-resource is here at all; the target decides
    // whether its output belongs in the link. An APE's per-architecture builds
    // run on a Windows host too, and cosmo's ld cannot take a Windows resource.
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_icon_resource();
    }
    let mut emitter = Emitter::default();
    // Dependency instructions run nested `cargo metadata`, which cannot resolve
    // unpublished workspace versions during a multi-package publish.
    let cargo = CargoBuilder::default()
        .debug(true)
        .features(true)
        .opt_level(true)
        .target_triple(true)
        .build()
        .expect("requested cargo instructions should build successfully");

    // Configures an [`Emitter`] for everything except for `git` env vars.
    // This builder fails the build on error.
    emitter
        .fail_on_error()
        .add_instructions(&cargo)
        .expect("adding cargo instructions should succeed")
        .add_instructions(
            &RustcBuilder::all_rustc().expect("all_rustc() should build successfully"),
        )
        .expect("adding all_rustc() instructions should succeed");

    // Get git information. This is used by e.g. ZebradApp::register_components()
    // to log the commit hash
    let all_git = Git2Builder::default()
        .branch(true)
        .commit_author_email(true)
        .commit_author_name(true)
        .commit_count(true)
        .commit_date(true)
        .commit_message(true)
        .commit_timestamp(true)
        .describe(false, false, None)
        .sha(true)
        .dirty(false)
        .describe(true, true, Some("v*.*.*"))
        .build()
        .expect("all_git + describe + sha should build successfully");

    if let Err(e) = emitter.add_instructions(&all_git) {
        // The most common failure here is due to a missing `.git` directory,
        // e.g., when building from `cargo install zebrad`. We simply
        // proceed with the build.
        // Note that this won't be printed unless in cargo very verbose mode (-vv).
        // We could emit a build warning, but that might scare users.
        println!("git error in vergen build script: skipping git env vars: {e:?}",);
    }

    emitter.emit().expect("base emit should succeed");

    #[cfg(feature = "lightwalletd-grpc-tests")]
    tonic_prost_build::configure()
        .build_client(true)
        .build_server(false)
        .compile_protos(
            &["tests/common/lightwalletd/proto/service.proto"],
            &["tests/common/lightwalletd/proto"],
        )
        .expect("Failed to generate lightwalletd gRPC files");
}

// Explorer, pinned shortcuts and the taskbar read RT_GROUP_ICON out of the binary; the
// GUI's runtime icon covers only a window that is already open. Compiling the resource
// needs rc.exe from the Windows SDK, so a missing one warns instead of failing.
#[cfg(windows)]
fn embed_icon_resource() {
    use std::{env, fs, path::PathBuf};

    let repo_root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .and_then(|p| p.parent())
        .unwrap()
        .to_path_buf();
    let ico = repo_root.join("packaging/icons/zebra-crosslink.ico");
    println!("cargo:rerun-if-changed={}", ico.display());

    let rc = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app_icon.rc");
    let ico = ico.display().to_string().replace('\\', "/");
    fs::write(&rc, format!("1 ICON \"{}\"\n", ico)).unwrap();

    if let Err(err) = embed_resource::compile(&rc, embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=application icon not embedded: {err}");
    }
}

/// Build zebrad again, once per architecture, and fuse the two ELFs into one
/// Actually Portable Executable.
///
/// cosmo-build points cc-rs at cosmocc for the C and C++ in this tree (rocksdb,
/// zcash_script, secp256k1, ring, bzip2, lz4, zlib); what is set here is only
/// what this tree's C needs on top. Setting it before `apeify()` is what makes
/// it stick: cosmo-build scrubs the toolchain variables from the nested cargo's
/// environment but passes everything else through.
#[cfg(feature = "ape")]
fn apeify() {
    use std::{env, fs, path::PathBuf};

    append_env("COSMO_CFLAGS", ENDIAN_FIX);
    append_env("COSMO_CXXFLAGS", &format!("{ENDIAN_FIX} -include algorithm {ROCKSDB_PLATFORM}"));

    // librocksdb-sys names its C++ runtime by triple: `stdc++` for anything
    // containing "linux", otherwise `c++`, set with cpp_link_stdlib, which beats
    // CXXSTDLIB. `*-unknown-cosmo` lands on `c++`, and cosmocc has no libc++.a:
    // its driver links libcxx.a on its own, and its libstdc++.a is an empty
    // archive so that `-lstdc++` (libzcash_script's) resolves to nothing. An
    // empty libc++.a on the search path does the same for `-lc++`.
    //
    // mio picks epoll and eventfd for target_os = "linux", which the cosmo
    // targets claim, but cosmopolitan has neither (it has poll and pipes on
    // every host). mio's own escape hatches select its poll(2) selector and pipe
    // waker instead; cosmo-compat has no epoll shim to fall back on.
    let cxx_stub = PathBuf::from(env::var("OUT_DIR").unwrap()).join("cosmo-cxx-stub");
    fs::create_dir_all(&cxx_stub).unwrap();
    fs::write(cxx_stub.join("libc++.a"), b"!<arch>\n").unwrap();
    append_env(
        "COSMO_RUSTFLAGS",
        &format!(
            "-L native={} --cfg mio_unsupported_force_poll_poll \
             --cfg mio_unsupported_force_waker_pipe",
            cxx_stub.display()
        ),
    );

    // The nested build gets a fresh feature set, so anything the outer build
    // turned on that changes the binary would have to be named again. Nothing
    // does now: the GUI is in every build, and whether it shows is decided at
    // run time (`viz2::run_node`), so one APE serves desktops and servers alike.
    //
    // `panic=unwind` is not a preference: cosmo-build compiles std with
    // -Zbuild-std and only the panic_unwind runtime, so this workspace's
    // `panic = "abort"` leaves rustc looking for a `panic_abort` crate that was
    // never built. Cosmopolitan ships the whole `_Unwind_*` ABI, so unwinding is
    // the supported choice there rather than a fallback.
    let args: Vec<&str> = vec![
        "--bin",
        "zebrad",
        "--config",
        "profile.release.panic=\"unwind\"",
    ];
    cosmo_build::apeify_with(&args);
}

/// Makes C use cosmo's own `endian.h`.
///
/// Cosmopolitan compiles OS-agnostically: `normalize.inc` undefines `__linux__`
/// and friends on purpose, so one binary can run anywhere. C that sniffs the OS
/// to pick its byte-order helpers then finds no branch it recognises --
/// equihash's bundled `portable_endian.h` stops at `#error platform not
/// supported`. Defining that header's include guard makes it a no-op and
/// force-including cosmo's own `endian.h` supplies the same macros for real.
///
/// It goes in `COSMO_CFLAGS` rather than `CFLAGS_<triple>`, because cc-rs
/// passes those to `.S` files too, and the assembler does not preprocess C: it
/// reads the declarations in `endian.h` and reports every one as an unknown
/// instruction. ring ships a lot of pregenerated assembly.
///
/// C++ additionally gets `-include algorithm`. Cosmo's libcxx does not pull it
/// in behind other headers the way the toolchains this code was written against
/// do, so zcash's `sha256.cpp` fails on `std::copy` and `std::equal`.
#[cfg(feature = "ape")]
const ENDIAN_FIX: &str = "-DPORTABLE_ENDIAN_H__=1 -include endian.h";

/// librocksdb-sys picks rocksdb's port layer with `target.contains("linux")` and
/// friends, so `*-unknown-cosmo` matches nothing and rocksdb compiles with no
/// platform defined at all -- which surfaces far from the cause, as hundreds of
/// incomplete-type errors inside libcxx. Cosmopolitan is POSIX, so this is the
/// configuration it would have picked had it recognised the triple. The macro
/// names are rocksdb's own, so nothing else in the build reads them.
///
/// POSIX without `OS_LINUX`, which is what rocksdb's own macOS and BSD branches
/// select: `OS_LINUX` additionally turns on kernel-specific paths that want
/// `<linux/fs.h>`, and cosmopolitan ships no Linux UAPI headers -- it cannot,
/// since the same binary has to boot on Windows and macOS.
///
/// `CYGWIN` is a misnomer here and is set deliberately: in rocksdb it does not
/// mean Cygwin so much as "a POSIX host without the glibc extensions", which is
/// exactly cosmopolitan. Everything it changes in the library is right for us:
///
/// * `env/io_posix.h` stops defining placeholder `POSIX_FADV_*` / `POSIX_MADV_*`
///   literals. That matters for more than compiling -- cosmo declares those as
///   `extern const int` resolved at load time, because the numbers differ per
///   OS, so rocksdb's hardcoded 0..4 would hand `posix_fadvise` the wrong flag
///   anywhere but Linux, silently.
/// * `port/port_posix.h` maps `fread_unlocked` and friends onto the locked
///   calls, which cosmo is the kind of libc to lack.
/// * `port/stack_trace.cc` drops rocksdb's own backtrace printer.
/// * `util/string_util.cc` parses with `strtoul` rather than `std::stoull`.
#[cfg(feature = "ape")]
const ROCKSDB_PLATFORM: &str = "-DROCKSDB_PLATFORM_POSIX -DROCKSDB_LIB_IO_POSIX -DCYGWIN";

/// Append to a space-separated flags variable, keeping whatever the caller set.
///
/// The workspace denies `unsafe_code`; the allow is scoped to this one function
/// because `env::set_var` is unsafe only for its effect on other threads, and a
/// build script is single-threaded at this point with nothing else reading the
/// environment.
#[cfg(feature = "ape")]
#[allow(unsafe_code)]
fn append_env(key: &str, value: &str) {
    let old = std::env::var(key).unwrap_or_default();
    // Safety: build scripts are single-threaded here; nothing else in this
    // process reads the environment concurrently.
    unsafe { std::env::set_var(key, format!("{old} {value}").trim()) };
}
