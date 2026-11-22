//! Tao's macOS delegate omits applicationShouldTerminate. Intercept AppKit termination
//! before its non-cancellable applicationWillTerminate path, preserving the exit barrier.
use anyhow::{Context, Result};
use objc2::{
    class, msg_send,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use std::sync::OnceLock;
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
extern "C-unwind" fn should_terminate(
    _delegate: *mut AnyObject,
    _selector: Sel,
    _sender: *mut AnyObject,
) -> usize {
    // NSTerminateCancel returns control to the normal run loop. Do not nest an AppKit
    // termination modal loop while the WebViews save/confirm. Accepted quit uses app.exit.
    if let Some(app) = APP.get() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::request_exit(app, "quit");
        }));
    }
    0
}
unsafe fn add_hook(class: &AnyClass) -> Result<()> {
    let selector = sel!(applicationShouldTerminate:);
    anyhow::ensure!(
        class.instance_method(selector).is_none(),
        "The macOS delegate already handles termination; update the shutdown bridge before shipping"
    );
    // AppKit signature: NSUInteger method(id self, SEL selector, NSApplication *sender).
    // Adding a missing method preserves the existing delegate and all its ivars/methods.
    let implementation: extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize =
        should_terminate;
    let added = unsafe {
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            std::mem::transmute::<
                extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
                Imp,
            >(implementation),
            c"Q@:@".as_ptr(),
        )
    };
    anyhow::ensure!(added.as_bool(), "Could not install macOS termination hook");
    Ok(())
}
pub fn install(app: &tauri::AppHandle) -> Result<()> {
    anyhow::ensure!(
        objc2::MainThreadMarker::new().is_some(),
        "Termination hook must be installed on the main thread"
    );
    unsafe {
        let nsapp: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![nsapp, delegate];
        let delegate = delegate.as_ref().context("Missing macOS app delegate")?;
        add_hook(delegate.class())?;
    }
    APP.set(app.clone())
        .map_err(|_| anyhow::anyhow!("Termination hook already installed"))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_has_appkit_signature_and_refuses_to_replace_an_existing_handler() {
        let class = objc2::runtime::ClassBuilder::new(c"MdiffTerminationTest", class!(NSObject))
            .unwrap()
            .register();
        unsafe { add_hook(class).unwrap() };
        let method = class
            .instance_method(sel!(applicationShouldTerminate:))
            .unwrap();
        assert_eq!(method.return_type().to_str().unwrap(), "Q");
        assert_eq!(method.arguments_count(), 3);
        assert!(unsafe { add_hook(class) }.is_err());
        // A cancelled request must never claim NSTerminateNow without an app barrier.
        assert_eq!(
            should_terminate(
                std::ptr::null_mut(),
                sel!(applicationShouldTerminate:),
                std::ptr::null_mut()
            ),
            0
        );
    }
}
