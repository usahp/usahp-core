use super::Driver;
use anyhow::{Result, bail};
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::GetAsyncKeyState, WindowsAndMessaging::*},
};
thread_local! {static DRIVER:RefCell<Option<Driver>>=const{RefCell::new(None)};}
pub fn code_for_name(name: &str) -> Option<u32> {
    Some(match name {
        "Space" => 0x20,
        "Enter" => 0x0d,
        "Backspace" => 8,
        "Tab" => 9,
        "Escape" => 27,
        "ArrowUp" => 38,
        "ArrowDown" => 40,
        "ArrowLeft" => 37,
        "ArrowRight" => 39,
        "Home" => 36,
        "End" => 35,
        "PageUp" => 33,
        "PageDown" => 34,
        "Insert" => 45,
        "Delete" => 46,
        n if n.len() == 1
            && n.bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) =>
        {
            n.as_bytes()[0] as u32
        }
        n => {
            let f = n.strip_prefix('F')?.parse::<u32>().ok()?;
            if !(1..=24).contains(&f) {
                return None;
            }
            0x70 + f - 1
        }
    })
}
fn name_for_code(code: u32) -> Option<String> {
    for name in [
        "Space",
        "Enter",
        "Backspace",
        "Tab",
        "Escape",
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Insert",
        "Delete",
    ] {
        if code_for_name(name) == Some(code) {
            return Some(name.into());
        }
    }
    if (0x30..=0x39).contains(&code) || (0x41..=0x5a).contains(&code) {
        return char::from_u32(code).map(|c| c.to_string());
    }
    (0x70..=0x87)
        .contains(&code)
        .then(|| format!("F{}", code - 0x70 + 1))
}
unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && [WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP].contains(&(wparam as u32)) {
        let key = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if let Some(name) = name_for_code(key.vkCode) {
            let consumed = DRIVER.with(|driver| {
                driver.borrow().as_ref().is_some_and(|d| {
                    d.key(
                        &name,
                        [WM_KEYDOWN, WM_SYSKEYDOWN].contains(&(wparam as u32)),
                    )
                })
            });
            if consumed {
                return 1;
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}
pub struct Capture {
    stop: Arc<AtomicBool>,
    thread_id: u32,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Capture {
    pub fn start(driver: Driver) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("usahp-embedded-hook".into())
            .spawn(move || unsafe {
                let thread_id = GetCurrentThreadId();
                DRIVER.with(|slot| *slot.borrow_mut() = Some(driver.clone()));
                let hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(hook),
                    GetModuleHandleW(std::ptr::null()),
                    0,
                );
                if hook.is_null() {
                    let _ = tx.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                let timer = SetTimer(std::ptr::null_mut(), 0, 20, None);
                if timer == 0 {
                    UnhookWindowsHookEx(hook);
                    let _ = tx.send(Err(std::io::Error::last_os_error().to_string()));
                    return;
                }
                for code in 0..=255 {
                    if GetAsyncKeyState(code) < 0 {
                        if let Some(name) = name_for_code(code as u32) {
                            driver.key(&name, true);
                        }
                    }
                }
                driver.ready();
                if tx.send(Ok(thread_id)).is_ok() {
                    let mut msg = std::mem::zeroed();
                    while !stopping.load(Ordering::Acquire) {
                        let result = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                        if result <= 0 {
                            if !stopping.load(Ordering::Acquire) {
                                driver.lost();
                            }
                            break;
                        }
                        if msg.message == WM_TIMER {
                            driver.tick();
                        } else {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }
                }
                KillTimer(std::ptr::null_mut(), timer);
                UnhookWindowsHookEx(hook);
                DRIVER.with(|slot| *slot.borrow_mut() = None);
            })?;
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(thread_id)) => Ok(Self {
                stop,
                thread_id,
                thread: Some(thread),
            }),
            Ok(Err(message)) => {
                let _ = thread.join();
                bail!("Windows switch capture could not start: {message}");
            }
            Err(_) => {
                stop.store(true, Ordering::Release);
                bail!("Windows switch capture did not acknowledge startup.");
            }
        }
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
