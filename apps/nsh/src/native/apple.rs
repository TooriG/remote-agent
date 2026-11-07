//! The application's signed Swift framework stays loaded with its registered metadata.
use libloading::Library;
use objc2::rc::Retained;
use objc2_foundation::{NSBundle, NSString};
use std::{mem::ManuallyDrop, path::PathBuf, sync::OnceLock};

static LIBRARY: OnceLock<Result<ManuallyDrop<Library>, String>> = OnceLock::new();

#[cfg(target_os = "macos")]
use objc2_app_kit::NSView as PlatformView;
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIView as PlatformView;

/// The caller declares the exact C ABI exported by apple/Sources/BexNshNative.
pub(super) unsafe fn symbol<T: Copy>(name: &[u8]) -> Result<T, String> {
    let library = LIBRARY
        .get_or_init(|| {
            let directory = NSBundle::mainBundle()
                .privateFrameworksPath()
                .ok_or_else(|| "アプリ内のネイティブ部品が見つかりません。".to_owned())?;
            let path =
                PathBuf::from(directory.to_string()).join("BexNshNative.framework/BexNshNative");
            // Even a symbol lookup failure must not unload registered Swift/ObjC metadata.
            unsafe { Library::new(path) }
                .map(ManuallyDrop::new)
                .map_err(|reason| reason.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)?;
    unsafe { library.get(name) }
        .map(|symbol| *symbol)
        .map_err(|reason| reason.to_string())
}

pub(super) fn failure(reason: &str) -> Retained<PlatformView> {
    let message = NSString::from_str(&format!("画面を開けませんでした。\n{reason}"));
    #[cfg(target_os = "macos")]
    {
        let label = objc2_app_kit::NSTextField::labelWithString(
            &message,
            objc2::MainThreadMarker::new().unwrap(),
        );
        label.setSelectable(true);
        label.into_super().into_super()
    }
    #[cfg(target_os = "ios")]
    {
        let label = objc2_ui_kit::UILabel::new(objc2::MainThreadMarker::new().unwrap());
        label.setText(Some(&message));
        label.setNumberOfLines(0);
        label.into_super()
    }
}
