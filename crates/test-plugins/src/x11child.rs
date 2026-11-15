//! A child window inside the host's editor window (stands in for a plugin's GUI in the host tests): an X11
//! window on Linux, a Win32 child window on Windows.

#[cfg(target_os = "linux")]
pub struct Child {
    x: x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
    window: x11_dl::xlib::Window,
}

#[cfg(target_os = "linux")]
impl Child {
    pub fn new(parent: u64, w: u32, h: u32) -> Option<Child> {
        let x = x11_dl::xlib::Xlib::open().ok()?;
        // SAFETY: plain Xlib calls on our own connection.
        unsafe {
            let display = (x.XOpenDisplay)(std::ptr::null());
            if display.is_null() {
                return None;
            }
            let screen = (x.XDefaultScreen)(display);
            let color = (x.XWhitePixel)(display, screen);
            let window = (x.XCreateSimpleWindow)(display, parent, 0, 0, w, h, 0, color, color);
            (x.XMapWindow)(display, window);
            (x.XSync)(display, 0);
            Some(Child { x, display, window })
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for Child {
    fn drop(&mut self) {
        // SAFETY: destroy what we created.
        unsafe {
            (self.x.XDestroyWindow)(self.display, self.window);
            (self.x.XSync)(self.display, 0);
            (self.x.XCloseDisplay)(self.display);
        }
    }
}

#[cfg(windows)]
pub struct Child {
    hwnd: windows_sys::Win32::Foundation::HWND,
}

#[cfg(windows)]
impl Child {
    pub fn new(parent: u64, w: u32, h: u32) -> Option<Child> {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let class: Vec<u16> = "STATIC".encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: a plain child window of the host's window.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                w as i32,
                h as i32,
                parent as usize as windows_sys::Win32::Foundation::HWND,
                std::ptr::null_mut(),
                windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        (!hwnd.is_null()).then_some(Child { hwnd })
    }
}

#[cfg(windows)]
impl Drop for Child {
    fn drop(&mut self) {
        // SAFETY: destroy what we created.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd) };
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub struct Child;

#[cfg(not(any(target_os = "linux", windows)))]
impl Child {
    pub fn new(_parent: u64, _w: u32, _h: u32) -> Option<Child> {
        None
    }
}
