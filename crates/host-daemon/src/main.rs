mod command_line;
#[cfg(any(target_os = "macos", test))]
mod environment;
mod management;
mod runtime;

use std::env;

use clap::Parser;
use command_line::StartupConfig;

fn main() {
    #[cfg(target_os = "macos")]
    environment::initialize();
    host_runtime().block_on(run());
}

fn host_runtime() -> tokio::runtime::Runtime {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    #[cfg(target_os = "macos")]
    {
        // The updater can start us on a background-QoS thread. Host RPCs and
        // provider processes must retain normal priority under CPU load.
        set_default_thread_qos();
        builder.on_thread_start(set_default_thread_qos);
    }
    builder.build().expect("create Host runtime")
}

#[cfg(target_os = "macos")]
fn set_default_thread_qos() {
    // This affects only the calling thread; Tokio also calls it for each worker
    // and blocking-pool thread rather than inheriting the launcher's QoS.
    let error =
        unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_DEFAULT, 0) };
    if error != 0 {
        eprintln!(
            "Host thread QoS: {}",
            std::io::Error::from_raw_os_error(error)
        );
    }
}

async fn run() {
    let config = match StartupConfig::try_parse_from(env::args().skip(1)) {
        Ok(config) => config,
        Err(error) => error.exit(),
    };

    if let Some(command_line::Mode::BrowserMcp { socket, thread }) = &config.mode {
        if let Err(error) = host_daemon::browser::mcp::serve(socket, thread).await {
            eprintln!("BEX browser bridge: {error}");
            std::process::exit(1);
        }
        return;
    }

    let result = match config.mode {
        Some(mode) => management::run(mode, config.state_dir, config.isolated).await,
        None => runtime::run(config).await,
    };
    if let Err(error) = result {
        let exit_code = if error.is::<management::DifferentHostBinary>() {
            42
        } else {
            1
        };
        let error = format!("{error:#}");
        tracing::error!(target: "agent", operation = "host.runtime", message = %error);
        eprintln!(
            "host daemon failed: {}",
            agent_transport::diagnostics::sanitize(&error)
        );
        std::process::exit(exit_code);
    }
    tracing::info!(target: "agent", operation = "shutdown", "Bex shutting down");
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    fn thread_qos() -> u32 {
        let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
        assert_eq!(
            unsafe {
                libc::pthread_get_qos_class_np(
                    libc::pthread_self(),
                    &mut class,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        class as u32
    }

    #[test]
    fn runtime_restores_normal_qos_for_main_async_and_blocking_threads() {
        std::thread::spawn(|| {
            assert_eq!(
                unsafe {
                    libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0)
                },
                0
            );
            assert_eq!(thread_qos(), libc::qos_class_t::QOS_CLASS_BACKGROUND as u32);
            let runtime = host_runtime();
            let expected = libc::qos_class_t::QOS_CLASS_DEFAULT as u32;
            assert_eq!(thread_qos(), expected);
            runtime.block_on(async {
                assert_eq!(
                    tokio::spawn(async { thread_qos() }).await.unwrap(),
                    expected
                );
            });
            // Blocking threads can be created later by a background caller.
            assert_eq!(
                unsafe {
                    libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0)
                },
                0
            );
            assert_eq!(
                runtime
                    .block_on(runtime.spawn_blocking(thread_qos))
                    .unwrap(),
                expected
            );
        })
        .join()
        .unwrap();
    }
}
