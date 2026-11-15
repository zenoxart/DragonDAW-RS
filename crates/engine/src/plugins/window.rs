//! Native top-level windows that plugin editors embed into (X11 on Linux, Win32 on Windows).
//!
//! The app's own UI is drawn by egui on a GPU surface, which plugins cannot draw into. Instead each open
//! plugin editor gets its own small native window: the host creates it, hands its handle to the plugin
//! (`IPlugView::attached` / `clap_plugin_gui::set_parent`) and pumps it from the UI thread.

use crate::{EngineError, Result};
use std::ffi::c_void;

pub struct EditorWindow {
    inner: imp::Window,
}

impl EditorWindow {
    /// Opens a window of `width` × `height` pixels.
    pub fn open(title: &str, width: u32, height: u32) -> Result<EditorWindow> {
        Ok(EditorWindow { inner: imp::Window::open(title, width.max(32), height.max(32))? })
    }

    /// The native handle to embed into: an X11 window id, or an `HWND`.
    pub fn handle(&self) -> *mut c_void {
        self.inner.handle()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.inner.resize(width.max(32), height.max(32));
    }

    /// Current inner size in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    /// Lets the user resize the window (only for editors that can follow a new size).
    pub fn set_resizable(&mut self, on: bool, size: (u32, u32)) {
        self.inner.set_resizable(on, size);
    }

    /// Full screen on (covers the monitor) or back to the previous size.
    pub fn set_fullscreen(&mut self, on: bool) {
        self.inner.set_fullscreen(on);
    }

    pub fn is_fullscreen(&self) -> bool {
        self.inner.fullscreen
    }

    /// Processes pending window events; returns `false` once the user closed the window.
    pub fn pump(&mut self) -> bool {
        self.inner.pump()
    }

    /// Number of direct child windows (the plugin's GUI), so tests can check that the editor embedded.
    pub fn child_count(&self) -> usize {
        self.inner.children()
    }

    /// Display scale of the window (1.0 = 96 dpi on Windows); plugins are told to draw at this scale.
    pub fn scale(&self) -> f32 {
        self.inner.scale()
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use x11_dl::xlib;

    pub struct Window {
        x: xlib::Xlib,
        display: *mut xlib::Display,
        window: xlib::Window,
        wm_delete: xlib::Atom,
        open: bool,
        pub fullscreen: bool,
    }

    impl Window {
        pub fn open(title: &str, w: u32, h: u32) -> Result<Window> {
            let x = xlib::Xlib::open().map_err(|e| EngineError::Plugin(format!("libX11: {e}")))?;
            // SAFETY: plain Xlib calls on a connection we own.
            unsafe {
                // A busy X server (a CI Xvfb, a reconnecting session) can refuse a connection for a moment:
                // try a few times before giving up.
                let mut display = (x.XOpenDisplay)(std::ptr::null());
                for _ in 0..20 {
                    if !display.is_null() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    display = (x.XOpenDisplay)(std::ptr::null());
                }
                if display.is_null() {
                    return Err(EngineError::Plugin("cannot open the X display".into()));
                }
                let screen = (x.XDefaultScreen)(display);
                let root = (x.XRootWindow)(display, screen);
                let black = (x.XBlackPixel)(display, screen);
                let window = (x.XCreateSimpleWindow)(display, root, 0, 0, w, h, 0, black, black);
                let name = std::ffi::CString::new(title).unwrap_or_default();
                (x.XStoreName)(display, window, name.as_ptr() as *mut _);
                let atom = std::ffi::CString::new("WM_DELETE_WINDOW").unwrap();
                let mut wm_delete = (x.XInternAtom)(display, atom.as_ptr(), 0);
                (x.XSetWMProtocols)(display, window, &mut wm_delete, 1);
                (x.XSelectInput)(display, window, xlib::StructureNotifyMask);
                (x.XMapWindow)(display, window);
                (x.XSync)(display, 0);
                let mut win = Window { x, display, window, wm_delete, open: true, fullscreen: false };
                // Fixed size until the editor says it can follow a new one.
                win.set_resizable(false, (w, h));
                Ok(win)
            }
        }

        pub fn handle(&self) -> *mut c_void {
            self.window as *mut c_void
        }

        pub fn resize(&mut self, w: u32, h: u32) {
            // SAFETY: our window on our connection.
            unsafe {
                (self.x.XResizeWindow)(self.display, self.window, w, h);
                (self.x.XFlush)(self.display);
            }
        }

        pub fn size(&self) -> (u32, u32) {
            // SAFETY: geometry query on our window.
            unsafe {
                let mut attrs: xlib::XWindowAttributes = std::mem::zeroed();
                if (self.x.XGetWindowAttributes)(self.display, self.window, &mut attrs) == 0 {
                    return (0, 0);
                }
                (attrs.width.max(0) as u32, attrs.height.max(0) as u32)
            }
        }

        pub fn set_resizable(&mut self, on: bool, size: (u32, u32)) {
            // SAFETY: WM_NORMAL_HINTS on our window (min = max fixes the size).
            unsafe {
                let hints = (self.x.XAllocSizeHints)();
                if hints.is_null() {
                    return;
                }
                if on {
                    (*hints).flags = xlib::PMinSize;
                    (*hints).min_width = 64;
                    (*hints).min_height = 48;
                } else {
                    (*hints).flags = xlib::PMinSize | xlib::PMaxSize;
                    (*hints).min_width = size.0 as i32;
                    (*hints).max_width = size.0 as i32;
                    (*hints).min_height = size.1 as i32;
                    (*hints).max_height = size.1 as i32;
                }
                (self.x.XSetWMNormalHints)(self.display, self.window, hints);
                (self.x.XFree)(hints as *mut c_void);
                (self.x.XFlush)(self.display);
            }
        }

        pub fn set_fullscreen(&mut self, on: bool) {
            // SAFETY: EWMH _NET_WM_STATE request to the root window for our window.
            unsafe {
                let state = std::ffi::CString::new("_NET_WM_STATE").unwrap();
                let fs = std::ffi::CString::new("_NET_WM_STATE_FULLSCREEN").unwrap();
                let a_state = (self.x.XInternAtom)(self.display, state.as_ptr(), 0);
                let a_fs = (self.x.XInternAtom)(self.display, fs.as_ptr(), 0);
                let mut ev: xlib::XEvent = std::mem::zeroed();
                ev.client_message.type_ = xlib::ClientMessage;
                ev.client_message.window = self.window;
                ev.client_message.message_type = a_state;
                ev.client_message.format = 32;
                ev.client_message.data.set_long(0, if on { 1 } else { 0 });
                ev.client_message.data.set_long(1, a_fs as i64);
                let root = (self.x.XDefaultRootWindow)(self.display);
                (self.x.XSendEvent)(self.display, root, 0, xlib::SubstructureRedirectMask | xlib::SubstructureNotifyMask, &mut ev);
                (self.x.XFlush)(self.display);
            }
            self.fullscreen = on;
        }

        pub fn pump(&mut self) -> bool {
            // SAFETY: event loop on our own connection.
            unsafe {
                while self.open && (self.x.XPending)(self.display) > 0 {
                    let mut ev: xlib::XEvent = std::mem::zeroed();
                    (self.x.XNextEvent)(self.display, &mut ev);
                    if ev.get_type() == xlib::ClientMessage && ev.client_message.data.get_long(0) as xlib::Atom == self.wm_delete {
                        self.open = false;
                    }
                }
            }
            self.open
        }

        pub fn scale(&self) -> f32 {
            1.0
        }

        pub fn children(&self) -> usize {
            // SAFETY: XQueryTree with out-pointers we free.
            unsafe {
                (self.x.XSync)(self.display, 0);
                let (mut root, mut parent) = (0, 0);
                let mut kids: *mut xlib::Window = std::ptr::null_mut();
                let mut n = 0u32;
                if (self.x.XQueryTree)(self.display, self.window, &mut root, &mut parent, &mut kids, &mut n) == 0 {
                    return 0;
                }
                if !kids.is_null() {
                    (self.x.XFree)(kids as *mut c_void);
                }
                n as usize
            }
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: tear down what we created.
            unsafe {
                (self.x.XDestroyWindow)(self.display, self.window);
                (self.x.XCloseDisplay)(self.display);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    pub struct Window {
        hwnd: HWND,
        pub fullscreen: bool,
        saved: Option<(u32, windows_sys::Win32::Foundation::RECT)>,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_CLOSE {
            ShowWindow(hwnd, SW_HIDE);
            return 0;
        }
        DefWindowProcW(hwnd, msg, w, l)
    }

    /// Plugin GUIs (VSTGUI, JUCE, iPlug …) use OLE for drag and drop and expect it on the UI thread.
    /// The app's window toolkit normally initialises it already; then this is a no-op (S_FALSE).
    fn ensure_ole() {
        thread_local!(static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
        DONE.with(|d| {
            if !d.get() {
                // SAFETY: per-thread OLE initialisation; S_FALSE / RPC_E_CHANGED_MODE are fine for us.
                unsafe { windows_sys::Win32::System::Ole::OleInitialize(std::ptr::null()) };
                d.set(true);
            }
        });
    }

    impl Window {
        pub fn open(title: &str, w: u32, h: u32) -> Result<Window> {
            ensure_ole();
            // SAFETY: standard Win32 window creation on the UI thread.
            unsafe {
                let class = wide("DragonPluginEditor");
                let inst = GetModuleHandleW(std::ptr::null());
                let wc = WNDCLASSW {
                    style: CS_DBLCLKS,
                    lpfnWndProc: Some(wndproc),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: inst,
                    hIcon: std::ptr::null_mut(),
                    hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                    hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class.as_ptr(),
                };
                // Fails harmlessly when the class is already registered.
                RegisterClassW(&wc);
                // Owned by the DAW's main window: stays above it, minimises with it, no taskbar entry.
                let owner = GetActiveWindow();
                let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
                let mut rect = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
                AdjustWindowRect(&mut rect, style, 0);
                let t = wide(title);
                let hwnd = CreateWindowExW(
                    WS_EX_DLGMODALFRAME,
                    class.as_ptr(),
                    t.as_ptr(),
                    style,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    owner,
                    std::ptr::null_mut(),
                    inst,
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    return Err(EngineError::Plugin("CreateWindowExW failed".into()));
                }
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
                Ok(Window { hwnd, fullscreen: false, saved: None })
            }
        }

        pub fn handle(&self) -> *mut c_void {
            self.hwnd as *mut c_void
        }

        pub fn resize(&mut self, w: u32, h: u32) {
            // SAFETY: our window.
            unsafe {
                let style = GetWindowLongW(self.hwnd, GWL_STYLE) as u32;
                let mut rect = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
                AdjustWindowRect(&mut rect, style, 0);
                SetWindowPos(self.hwnd, std::ptr::null_mut(), 0, 0, rect.right - rect.left, rect.bottom - rect.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }

        pub fn size(&self) -> (u32, u32) {
            // SAFETY: client rect of our window.
            unsafe {
                let mut r = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetClientRect(self.hwnd, &mut r);
                ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32)
            }
        }

        pub fn set_resizable(&mut self, on: bool, _size: (u32, u32)) {
            // SAFETY: style change on our window.
            unsafe {
                let mut style = GetWindowLongW(self.hwnd, GWL_STYLE) as u32;
                if on {
                    style |= WS_THICKFRAME | WS_MAXIMIZEBOX;
                } else {
                    style &= !(WS_THICKFRAME | WS_MAXIMIZEBOX);
                }
                SetWindowLongW(self.hwnd, GWL_STYLE, style as i32);
                SetWindowPos(self.hwnd, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE);
            }
        }

        pub fn set_fullscreen(&mut self, on: bool) {
            use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
            // SAFETY: borderless window covering the monitor, and back.
            unsafe {
                if on && self.saved.is_none() {
                    let style = GetWindowLongW(self.hwnd, GWL_STYLE) as u32;
                    let mut rect = windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 };
                    GetWindowRect(self.hwnd, &mut rect);
                    self.saved = Some((style, rect));
                    let mon = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
                    let mut mi: MONITORINFO = std::mem::zeroed();
                    mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
                    GetMonitorInfoW(mon, &mut mi);
                    SetWindowLongW(self.hwnd, GWL_STYLE, (WS_POPUP | WS_VISIBLE | WS_CLIPCHILDREN) as i32);
                    let r = mi.rcMonitor;
                    SetWindowPos(self.hwnd, HWND_TOP, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED);
                } else if !on {
                    if let Some((style, r)) = self.saved.take() {
                        SetWindowLongW(self.hwnd, GWL_STYLE, style as i32);
                        SetWindowPos(self.hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_FRAMECHANGED | SWP_NOZORDER);
                    }
                }
            }
            self.fullscreen = on;
        }

        pub fn pump(&mut self) -> bool {
            // Messages are dispatched by the app's own message loop (winit); just report visibility.
            // SAFETY: query on our window.
            unsafe { IsWindowVisible(self.hwnd) != 0 }
        }

        pub fn scale(&self) -> f32 {
            // SAFETY: query on our window (0 when the API is unavailable → treat as 96 dpi).
            let dpi = unsafe { GetDpiForWindow(self.hwnd) };
            if dpi == 0 {
                1.0
            } else {
                dpi as f32 / 96.0
            }
        }

        pub fn children(&self) -> usize {
            // SAFETY: walking the direct children of our window.
            unsafe {
                let mut n = 0;
                let mut c = GetWindow(self.hwnd, GW_CHILD);
                while !c.is_null() {
                    n += 1;
                    c = GetWindow(c, GW_HWNDNEXT);
                }
                n
            }
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: destroy what we created.
            unsafe {
                DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use super::*;

    pub struct Window {
        pub fullscreen: bool,
    }

    impl Window {
        pub fn size(&self) -> (u32, u32) {
            (0, 0)
        }
        pub fn set_resizable(&mut self, _on: bool, _size: (u32, u32)) {}
        pub fn set_fullscreen(&mut self, on: bool) {
            self.fullscreen = on;
        }
        pub fn open(_title: &str, _w: u32, _h: u32) -> Result<Window> {
            Err(EngineError::Plugin("embedded plugin editors are not supported on this platform yet".into()))
        }
        pub fn handle(&self) -> *mut c_void {
            std::ptr::null_mut()
        }
        pub fn resize(&mut self, _w: u32, _h: u32) {}
        pub fn pump(&mut self) -> bool {
            false
        }
        pub fn scale(&self) -> f32 {
            1.0
        }
        pub fn children(&self) -> usize {
            0
        }
    }
}
