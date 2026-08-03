#![cfg(target_os = "macos")]

//! Focus watcher — polls `NSWorkspace` for the frontmost application's PID
//! and notifies the broker when it changes. This drives the focus-based
//! session lifecycle (Stage 2): when the session's app loses focus, the
//! broker revokes the session.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::broker::BrokerCommand;

type Id = *mut std::ffi::c_void;

unsafe extern "C" {
    fn objc_getClass(name: *const u8) -> Id;
    fn sel_registerName(name: *const u8) -> Id;
    fn objc_msgSend(obj: Id, sel: Id) -> Id;
    /// Separate declaration for calls returning an int (e.g. processIdentifier).
    /// Using objc_msgSend with a pointer return type for an int return is
    /// technically UB even though it works on current Apple hardware.
    #[link_name = "objc_msgSend"]
    #[allow(clashing_extern_declarations)]
    fn objc_msgSend_int(obj: Id, sel: Id) -> i32;
}

const NSWORKSPACE: &[u8] = b"NSWorkspace\0";
const SHARED_WORKSPACE: &[u8] = b"sharedWorkspace\0";
const FRONTMOST_APP: &[u8] = b"frontmostApplication\0";
const PROCESS_IDENTIFIER: &[u8] = b"processIdentifier\0";

fn frontmost_pid() -> Option<u32> {
    unsafe {
        let cls = objc_getClass(NSWORKSPACE.as_ptr());
        if cls.is_null() {
            return None;
        }
        let sel_shared = sel_registerName(SHARED_WORKSPACE.as_ptr());
        let ws = objc_msgSend(cls, sel_shared);
        if ws.is_null() {
            return None;
        }
        let sel_front = sel_registerName(FRONTMOST_APP.as_ptr());
        let app = objc_msgSend(ws, sel_front);
        if app.is_null() {
            return None;
        }
        let sel_pid = sel_registerName(PROCESS_IDENTIFIER.as_ptr());
        let pid = objc_msgSend_int(app, sel_pid);
        if pid > 0 { Some(pid as u32) } else { None }
    }
}

/// Spawn a polling thread that watches the frontmost application. When it
/// changes, sends `BrokerCommand::FocusChanged` to the broker.
pub fn spawn(broker: mpsc::Sender<BrokerCommand>) {
    std::thread::Builder::new()
        .name("usahp-focus-watcher".into())
        .spawn(move || {
            let mut last_pid: Option<u32> = frontmost_pid();
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let pid = frontmost_pid();
                if pid != last_pid {
                    last_pid = pid;
                    tracing::info!(?pid, "frontmost application changed");
                    let _ =
                        broker.blocking_send(BrokerCommand::FocusChanged { frontmost_pid: pid });
                }
            }
        })
        .ok();
}
