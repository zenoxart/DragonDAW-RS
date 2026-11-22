//! Which side of a modifier is held, and the numeric-pad 0 – things the GUI toolkit does not report but
//! the shortcuts use (Right-Shift+click slices, Right-Alt+click mutes / quantizes, numpad 0 fast
//! forward). Asked from the OS keyboard state: X11 (`XQueryKeymap`) and Windows (`GetAsyncKeyState`);
//! elsewhere (Wayland, macOS) every query answers `None`.

/// Keys this module can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideKey {
    LeftShift,
    RightShift,
    LeftAlt,
    RightAlt,
    Numpad0,
}

/// Whether the key is held now; `None` when the platform cannot tell.
pub fn is_down(k: SideKey) -> Option<bool> {
    imp::is_down(k)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::SideKey;
    use std::sync::{Mutex, OnceLock};
    use x11_dl::xlib;

    struct Conn {
        x: xlib::Xlib,
        display: usize,
    }
    // The display is only used under the mutex.
    unsafe impl Send for Conn {}

    fn conn() -> &'static Mutex<Option<Conn>> {
        static C: OnceLock<Mutex<Option<Conn>>> = OnceLock::new();
        C.get_or_init(|| {
            let c = xlib::Xlib::open().ok().and_then(|x| {
                // SAFETY: plain Xlib call; a null display means no X server (Wayland, headless).
                let d = unsafe { (x.XOpenDisplay)(std::ptr::null()) };
                (!d.is_null()).then_some(Conn { x, display: d as usize })
            });
            Mutex::new(c)
        })
    }

    pub fn is_down(k: SideKey) -> Option<bool> {
        let guard = conn().lock().ok()?;
        let c = guard.as_ref()?;
        let d = c.display as *mut xlib::Display;
        let syms: &[u64] = match k {
            SideKey::LeftShift => &[0xffe1],
            SideKey::RightShift => &[0xffe2],
            SideKey::LeftAlt => &[0xffe9],
            // Right Alt is Alt_R, or AltGr (ISO_Level3_Shift) on many layouts.
            SideKey::RightAlt => &[0xffea, 0xfe03],
            // KP_0, or KP_Insert when Num Lock is off.
            SideKey::Numpad0 => &[0xffb0, 0xff9e],
        };
        let mut keys = [0 as std::os::raw::c_char; 32];
        // SAFETY: Xlib calls on our own connection, under the mutex.
        unsafe {
            (c.x.XQueryKeymap)(d, keys.as_mut_ptr());
            Some(syms.iter().any(|s| {
                let code = (c.x.XKeysymToKeycode)(d, *s) as usize;
                code != 0 && (keys[code / 8] as u8 >> (code % 8)) & 1 == 1
            }))
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::SideKey;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    pub fn is_down(k: SideKey) -> Option<bool> {
        let vk = match k {
            SideKey::LeftShift => 0xA0,
            SideKey::RightShift => 0xA1,
            SideKey::LeftAlt => 0xA4,
            SideKey::RightAlt => 0xA5,
            SideKey::Numpad0 => 0x60,
        };
        // SAFETY: reads the asynchronous key state.
        Some(unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    pub fn is_down(_k: super::SideKey) -> Option<bool> {
        None
    }
}
