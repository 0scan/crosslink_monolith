//! A component owning the Tokio runtime.
//!
//! Async tasks run here, as do blocking network and file tasks. Shutdown keeps the runtime
//! alive while the root future stops its workers, then waits for remaining blocking work.

#![allow(non_local_definitions)]

use std::future::Future;

use abscissa_core::{Component, FrameworkError};
use color_eyre::Report;
use tokio::runtime::Runtime;

use crate::prelude::*;

/// An Abscissa component which owns a Tokio runtime.
///
/// Taking the runtime before `block_on` avoids holding an application lock for the node's life.
#[derive(Component, Debug)]
pub struct TokioComponent {
    pub rt: Option<Runtime>,
}

impl TokioComponent {
    #[allow(clippy::unwrap_in_result)]
    pub fn new() -> Result<Self, FrameworkError> {
        Ok(Self {
            rt: Some(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("runtime building should not fail"),
            ),
        })
    }
}

/// Waits for either an OS signal or a shutdown request from another component.
async fn shutdown() {
    tokio::select! {
        biased;
        _ = zebra_chain::shutdown::shutdown_requested() => {},
        _ = imp::shutdown() => {},
    }
    zebra_chain::shutdown::set_shutting_down();
}

async fn run_root_until_stopped(
    fut: impl Future<Output = Result<(), Report>>,
    stop: impl Future<Output = ()>,
) -> Result<(), Report> {
    tokio::pin!(fut);
    // Never drop the root future on a stop request: it owns cooperative worker cleanup.
    tokio::select! {
        biased;
        _ = stop => fut.await,
        result = &mut fut => result,
    }
}

/// Extension trait to centralize entry point for runnable subcommands that depend on Tokio.
pub(crate) trait RuntimeRun {
    fn run(self, fut: impl Future<Output = Result<(), Report>>);
}

impl RuntimeRun for Runtime {
    fn run(self, fut: impl Future<Output = Result<(), Report>>) {
        let result = self.block_on(async move {
            // The root owns its database workers and supervised child.
            // Request shutdown without dropping it, so its cleanup runs
            // while the runtime and its IO driver are still alive.
            let result = run_root_until_stopped(fut, shutdown()).await;
            zebra_chain::shutdown::set_shutting_down();
            result
        });

        // Runtime::drop cancels residual async tasks and waits for blocking tasks to finish.
        // A timeout here would detach a writer still using its database, not graceful shutdown.
        info!("waiting for remaining blocking Tokio tasks to finish");
        drop(self);

        // Normally the root future already stopped the supervisor. Keep the fallback for
        // an error during startup, before its cooperative cleanup was established.
        {
            let config = APPLICATION.config();
            if config.zcashd_compat.enabled && config.zcashd_compat.manage_zcashd {
                crate::components::zcashd_compat::terminate_abandoned_zcashd(
                    config.zcashd_compat.shutdown_grace_period,
                );
            }
        }

        match result {
            Ok(()) => info!("shutting down Zebra"),
            Err(error) => {
                warn!(?error, "shutting down Zebra due to an error");
                // Final exit belongs to main, after the node and GUI threads join.
                crate::application::COMMAND_FAILED.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
    }
}

#[cfg(all(unix, not(cosmo)))]
mod imp {
    use tokio::signal::unix::{signal, SignalKind};

    pub(super) async fn shutdown() {
        tokio::select! {
            _ = sig(SignalKind::interrupt(), "SIGINT") => {},
            _ = sig(SignalKind::terminate(), "SIGTERM") => {},
        }
    }

    #[instrument]
    async fn sig(kind: SignalKind, name: &'static str) {
        signal(kind)
            .expect("Failed to register signal handler")
            .recv()
            .await;
        zebra_chain::shutdown::set_shutting_down();
        #[cfg(feature = "progress-bar")]
        howudoin::disable();
        info!(target: "zebrad::signal", "received {}, starting shutdown", name);
    }
}

#[cfg(all(not(unix), not(cosmo)))]
mod imp {
    pub(super) async fn shutdown() {
        tokio::signal::ctrl_c()
            .await
            .expect("listening for ctrl-c signal should never fail");
        zebra_chain::shutdown::set_shutting_down();
        #[cfg(feature = "progress-bar")]
        howudoin::disable();
        info!(target: "zebrad::signal", "received Ctrl-C, starting shutdown");
    }
}

/// Installs Cosmopolitan's host-aware signal handlers before starting the GUI or any runtime.
///
/// Tokio's Unix signal transport requires AF_UNIX, unavailable on some Windows APE hosts.
#[cfg(cosmo)]
pub fn install_portable_signal_handlers() -> std::io::Result<()> {
    imp::install()
}

#[cfg(cosmo)]
use portable_signals as imp;

#[cfg(any(cosmo, test))]
#[allow(unsafe_code)]
mod portable_signals {
    use std::{
        ffi::c_int,
        sync::atomic::{AtomicI32, Ordering},
    };
    #[cfg(cosmo)]
    use std::time::Duration;

    static SIGNAL_RECEIVED: AtomicI32 = AtomicI32::new(0);

    #[cfg(cosmo)]
    unsafe extern "C" {
        // cosmo-compat's linker wrapper translates Linux's signal numbers to the host ABI.
        fn signal(signum: c_int, handler: usize) -> usize;
    }

    extern "C" fn receive_signal(signum: c_int) {
        // Do not lock, allocate, log, or wake an executor in an OS signal handler.
        SIGNAL_RECEIVED.store(signum, Ordering::SeqCst);
    }

    #[cfg(cosmo)]
    pub(super) fn install() -> std::io::Result<()> {
        for signum in [2, 15] {
            // SIGINT and SIGTERM, expressed in the Linux ABI used by the APE Rust target.
            let previous = unsafe { signal(signum, receive_signal as *const () as usize) };
            if previous == usize::MAX {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }

    #[cfg(cosmo)]
    pub(super) async fn shutdown() {
        loop {
            let signum = SIGNAL_RECEIVED.load(Ordering::SeqCst);
            if signum != 0 {
                zebra_chain::shutdown::set_shutting_down();
                #[cfg(feature = "progress-bar")]
                howudoin::disable();
                info!(target: "zebrad::signal", signum, "received signal, starting shutdown");
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn signal_callback_only_records_request() {
        SIGNAL_RECEIVED.store(0, Ordering::SeqCst);
        receive_signal(2);
        assert_eq!(SIGNAL_RECEIVED.load(Ordering::SeqCst), 2);
        SIGNAL_RECEIVED.store(0, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::run_root_until_stopped;
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    #[tokio::test]
    async fn stop_waits_for_root_to_finish_in_flight_write() {
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let root = async {
            finish_rx.await.unwrap();
            Ok(())
        };
        let handle = tokio::spawn(run_root_until_stopped(root, async {
            stop_rx.await.unwrap();
        }));
        stop_tx.send(()).unwrap();
        tokio::task::yield_now().await;
        assert!(!handle.is_finished(), "stop must not drop a root owning a write");
        finish_tx.send(()).unwrap();
        handle.await.unwrap().unwrap();
    }

    #[test]
    fn runtime_drop_waits_for_blocking_write_completion() {
        let finished = Arc::new(AtomicBool::new(false));
        let writer_finished = finished.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let rt = tokio::runtime::Builder::new_multi_thread().build().unwrap();
        rt.spawn_blocking(move || {
            started_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(30));
            writer_finished.store(true, Ordering::SeqCst);
        });
        started_rx.recv().unwrap();
        drop(rt);
        assert!(finished.load(Ordering::SeqCst));
    }
}
