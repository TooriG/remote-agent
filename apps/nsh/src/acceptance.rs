//! The Mac acceptance driver: with `BEX_ACCEPTANCE=<scenario>` and
//! `BEX_ACCEPTANCE_OUTPUT=<directory>`, the app takes the scenario's steps on its
//! own windows, writes the result there and quits. Without them nothing runs.
//! `cargo xtask nsh-acceptance` starts each scenario against a fixture Host.
mod script;

#[cfg(target_os = "macos")]
mod driver;

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
    Ok(Some(driver::start(scenario, lines, output, mtm)))
}
