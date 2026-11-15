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
    }

    impl Window {
        pub fn open(title: &str, w: u32, h: u32) -> Result<Window> {
            let x = xlib::Xlib::open().map_err(|e| EngineError::Plugin(format!("libX11: {e}")))?;
            // SAFETY: plain Xlib calls on a connection we own.
            unsafe {
                let display = (x.XOpenDisplay)(std::ptr::null());
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
                Ok(Window { x, display, window, wm_delete, open: true })
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
                Ok(Window { hwnd })
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

    pub struct Window;

    impl Window {
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
