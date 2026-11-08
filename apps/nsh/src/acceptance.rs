//! The Mac acceptance driver: with `BEX_ACCEPTANCE=<scenario>` and
//! `BEX_ACCEPTANCE_OUTPUT=<directory>`, the app takes the scenario's steps on its
//! own windows, writes the result there and quits, posting fixture steps to the
//! control port `BEX_ACCEPTANCE_FIXTURE` names. Without them nothing runs.
//! `cargo xtask nsh-acceptance` starts each scenario against a fixture Host.
mod script;

#[cfg(target_os = "macos")]
mod driver;

/// Engine-side availability, before the native mount/measurement handshake.
#[cfg(target_os = "macos")]
pub(crate) fn rows_ready(count: usize) {
    use std::io::Write;
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("BEX_ACCEPTANCE_OUTPUT").is_some()) {
        let fields = serde_json::json!({"at": objc2_foundation::NSDate::timeIntervalSinceReferenceDate_class(), "count":count});
        let _ = std::io::stderr()
            .lock()
            .write_all(format!("render-rows {fields}\n").as_bytes());
    }
}

/// Starts the scenario the environment names, if any. The flag says whether it
/// passed once the app's event loop returns.
#[cfg(target_os = "macos")]
pub(crate) fn from_environment(
    mtm: objc2::MainThreadMarker,
) -> Result<Option<std::rc::Rc<std::cell::Cell<bool>>>, String> {
    let Some(scenario) = std::env::var_os("BEX_ACCEPTANCE").map(std::path::PathBuf::from) else {
        return Ok(None);
    };
    let output = std::env::var_os("BEX_ACCEPTANCE_OUTPUT")
        .map(std::path::PathBuf::from)
        .ok_or("BEX_ACCEPTANCE_OUTPUT is required with BEX_ACCEPTANCE")?;
    let source = std::fs::read_to_string(&scenario)
        .map_err(|reason| format!("{}: {reason}", scenario.display()))?;
    let lines =
        script::parse(&source).map_err(|reason| format!("{}: {reason}", scenario.display()))?;
    std::fs::create_dir_all(&output).map_err(|reason| format!("{}: {reason}", output.display()))?;
    let fixture = std::env::var("BEX_ACCEPTANCE_FIXTURE")
        .ok()
        .map(|port| {
            port.parse()
                .map_err(|_| format!("BEX_ACCEPTANCE_FIXTURE {port:?} is not a port"))
        })
        .transpose()?;
    Ok(Some(driver::start(scenario, lines, output, fixture, mtm)))
}
