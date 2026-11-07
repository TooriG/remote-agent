#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "macos", test))]
mod saved;
#[cfg(target_os = "macos")]
pub use macos::{Browser, Prepared, Startup};
