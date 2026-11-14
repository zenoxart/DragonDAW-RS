//! A child X11 window inside the host's editor window (stands in for a plugin's GUI in the host tests).

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

#[cfg(not(target_os = "linux"))]
pub struct Child;

#[cfg(not(target_os = "linux"))]
impl Child {
    pub fn new(_parent: u64, _w: u32, _h: u32) -> Option<Child> {
        None
    }
}
