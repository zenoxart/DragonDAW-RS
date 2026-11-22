//! CLAP host (via `clap-sys`).

use super::{library, HostEvent, PluginInfo, Scratch};
use crate::effects::{Effect, PluginParam};
use crate::instruments::{InstrumentVoice, NoteOn};
use crate::model::{PluginFormat, PluginRef};
use crate::samples::SampleCache;
use crate::{EngineError, Result};
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::{clap_audio_port_info, clap_plugin_audio_ports, CLAP_EXT_AUDIO_PORTS};
use clap_sys::ext::gui::{clap_plugin_gui, clap_window, clap_window_handle, CLAP_EXT_GUI, CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32, CLAP_WINDOW_API_X11};
use clap_sys::ext::latency::{clap_plugin_latency, CLAP_EXT_LATENCY};
use clap_sys::ext::params::{clap_param_info, clap_plugin_params, CLAP_EXT_PARAMS, CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_STEPPED};
use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::clap_process;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;
use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: CLAP strings are NUL-terminated C strings owned by the plugin.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string()
    }
}

/// The shared library of a `.clap` plugin: the file itself, or inside a `.clap` folder – the macOS bundle
/// (`Contents/MacOS/<name>`) or a folder holding the library next to its resources (`<name>.clap/<name>.clap`,
/// used on Linux and Windows by plugins that ship data files).
pub(crate) fn binary(path: &Path) -> PathBuf {
    if path.is_dir() {
        let name = path.file_stem().map(|s| s.to_os_string()).unwrap_or_default();
        let mac = path.join("Contents").join("MacOS").join(&name);
        if mac.is_file() {
            return mac;
        }
        let inner = path.join(path.file_name().unwrap_or_default());
        if inner.is_file() {
            return inner;
        }
        // Any single `.clap` file inside.
        if let Ok(rd) = std::fs::read_dir(path) {
            let files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("clap"))).collect();
            if let [only] = files.as_slice() {
                return only.clone();
            }
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod bundle_tests {
    #[test]
    fn clap_folders_resolve_to_their_library() {
        let dir = tempfile::tempdir().unwrap();
        // Linux / Windows style: Foo.clap/Foo.clap (+ resources).
        let a = dir.path().join("Foo.clap");
        std::fs::create_dir_all(a.join("presets")).unwrap();
        std::fs::write(a.join("Foo.clap"), b"x").unwrap();
        assert_eq!(super::binary(&a), a.join("Foo.clap"));
        // Inner file with another name.
        let b = dir.path().join("Bar.clap");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("bar-x64.clap"), b"x").unwrap();
        assert_eq!(super::binary(&b), b.join("bar-x64.clap"));
        // macOS bundle.
        let c = dir.path().join("Baz.clap");
        std::fs::create_dir_all(c.join("Contents/MacOS")).unwrap();
        std::fs::write(c.join("Contents/MacOS/Baz"), b"x").unwrap();
        assert_eq!(super::binary(&c), c.join("Contents/MacOS/Baz"));
        // A plain file stays as it is.
        let f = dir.path().join("Plain.clap");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(super::binary(&f), f);
    }
}

/// Returns the entry, calling `init` once per library.
fn entry(path: &Path) -> Result<*const clap_plugin_entry> {
    static INIT: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let bin = binary(path);
    let lib = library(&bin)?;
    // SAFETY: `clap_entry` is an exported static of type `clap_plugin_entry` (CLAP ABI).
    let entry: *const clap_plugin_entry = unsafe { *lib.get::<*const clap_plugin_entry>(b"clap_entry\0").map_err(|e| EngineError::Plugin(format!("not a CLAP plugin: {e}")))? };
    if entry.is_null() {
        return Err(EngineError::Plugin("null clap_entry".into()));
    }
    let mut inited = INIT.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !inited.contains(&bin) {
        let cpath = CString::new(path.to_string_lossy().as_bytes()).unwrap_or_default();
        // SAFETY: calling the plugin's entry per the CLAP spec.
        let ok = unsafe { (*entry).init.map(|f| f(cpath.as_ptr())).unwrap_or(false) };
        if !ok {
            return Err(EngineError::Plugin("clap_entry.init failed".into()));
        }
        inited.insert(bin);
    }
    Ok(entry)
}

fn factory(path: &Path) -> Result<*const clap_plugin_factory> {
    let e = entry(path)?;
    // SAFETY: CLAP entry function.
    let f = unsafe { (*e).get_factory.map(|g| g(CLAP_PLUGIN_FACTORY_ID.as_ptr())).unwrap_or(std::ptr::null()) } as *const clap_plugin_factory;
    if f.is_null() {
        return Err(EngineError::Plugin("no plugin factory".into()));
    }
    Ok(f)
}

fn descriptor_info(path: &Path, d: &clap_plugin_descriptor) -> PluginInfo {
    let mut features = Vec::new();
    if !d.features.is_null() {
        let mut i = 0;
        // SAFETY: NULL-terminated array of C strings.
        unsafe {
            while !(*d.features.add(i)).is_null() {
                features.push(cstr(*d.features.add(i)));
                i += 1;
            }
        }
    }
    let instrument = features.iter().any(|f| f == "instrument" || f == "synthesizer" || f == "sampler" || f == "drum-machine");
    PluginInfo { format: PluginFormat::Clap, path: path.to_path_buf(), id: cstr(d.id), name: cstr(d.name), vendor: cstr(d.vendor), version: cstr(d.version), instrument, category: features.join(", ") }
}

pub fn scan(path: &Path) -> Result<Vec<PluginInfo>> {
    let f = factory(path)?;
    let mut out = Vec::new();
    // SAFETY: factory functions per CLAP spec.
    unsafe {
        let count = (*f).get_plugin_count.map(|g| g(f)).unwrap_or(0);
        for i in 0..count {
            let d = (*f).get_plugin_descriptor.map(|g| g(f, i)).unwrap_or(std::ptr::null());
            if !d.is_null() {
                out.push(descriptor_info(path, &*d));
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ host

/// Per-instance host state reached through `clap_host::host_data`: GUI resize requests, timers and file
/// descriptors registered by the plugin (serviced from the UI thread).
#[derive(Default)]
struct HostState {
    resize: std::cell::Cell<Option<(u32, u32)>>,
    closed: std::cell::Cell<bool>,
    timers: std::cell::RefCell<Vec<(clap_sys::id::clap_id, std::time::Duration, std::time::Instant)>>,
    next_timer: std::cell::Cell<u32>,
    fds: std::cell::RefCell<Vec<(i32, u32)>>,
}

unsafe fn host_state<'a>(h: *const clap_host) -> Option<&'a HostState> {
    if h.is_null() || (*h).host_data.is_null() {
        None
    } else {
        Some(&*((*h).host_data as *const HostState))
    }
}

unsafe extern "C" fn gui_resize_hints(_h: *const clap_host) {}
unsafe extern "C" fn gui_request_resize(h: *const clap_host, w: u32, hh: u32) -> bool {
    host_state(h).map(|s| s.resize.set(Some((w, hh)))).is_some()
}
unsafe extern "C" fn gui_request_show(_h: *const clap_host) -> bool {
    true
}
unsafe extern "C" fn gui_request_hide(_h: *const clap_host) -> bool {
    false
}
unsafe extern "C" fn gui_closed(h: *const clap_host, _destroyed: bool) {
    if let Some(s) = host_state(h) {
        s.closed.set(true);
    }
}
unsafe extern "C" fn timer_register(h: *const clap_host, period_ms: u32, id: *mut clap_sys::id::clap_id) -> bool {
    let Some(s) = host_state(h) else { return false };
    let n = s.next_timer.get() + 1;
    s.next_timer.set(n);
    s.timers.borrow_mut().push((n, std::time::Duration::from_millis(period_ms.max(1) as u64), std::time::Instant::now()));
    if !id.is_null() {
        *id = n;
    }
    true
}
unsafe extern "C" fn timer_unregister(h: *const clap_host, id: clap_sys::id::clap_id) -> bool {
    host_state(h).map(|s| s.timers.borrow_mut().retain(|t| t.0 != id)).is_some()
}
unsafe extern "C" fn fd_register(h: *const clap_host, fd: i32, flags: u32) -> bool {
    host_state(h).map(|s| s.fds.borrow_mut().push((fd, flags))).is_some()
}
unsafe extern "C" fn fd_modify(h: *const clap_host, fd: i32, flags: u32) -> bool {
    host_state(h).map(|s| s.fds.borrow_mut().iter_mut().filter(|f| f.0 == fd).for_each(|f| f.1 = flags)).is_some()
}
unsafe extern "C" fn fd_unregister(h: *const clap_host, fd: i32) -> bool {
    host_state(h).map(|s| s.fds.borrow_mut().retain(|f| f.0 != fd)).is_some()
}

struct HostExt<T>(T);
// SAFETY: tables of function pointers.
unsafe impl<T> Sync for HostExt<T> {}
static HOST_GUI: HostExt<clap_sys::ext::gui::clap_host_gui> = HostExt(clap_sys::ext::gui::clap_host_gui {
    resize_hints_changed: Some(gui_resize_hints),
    request_resize: Some(gui_request_resize),
    request_show: Some(gui_request_show),
    request_hide: Some(gui_request_hide),
    closed: Some(gui_closed),
});
static HOST_TIMER: HostExt<clap_sys::ext::timer_support::clap_host_timer_support> =
    HostExt(clap_sys::ext::timer_support::clap_host_timer_support { register_timer: Some(timer_register), unregister_timer: Some(timer_unregister) });
static HOST_FD: HostExt<clap_sys::ext::posix_fd_support::clap_host_posix_fd_support> =
    HostExt(clap_sys::ext::posix_fd_support::clap_host_posix_fd_support { register_fd: Some(fd_register), modify_fd: Some(fd_modify), unregister_fd: Some(fd_unregister) });

unsafe extern "C" fn host_get_extension(_h: *const clap_host, id: *const c_char) -> *const c_void {
    if id.is_null() {
        return std::ptr::null();
    }
    let id = CStr::from_ptr(id);
    if id == CLAP_EXT_GUI {
        &HOST_GUI.0 as *const _ as *const c_void
    } else if id == clap_sys::ext::timer_support::CLAP_EXT_TIMER_SUPPORT {
        &HOST_TIMER.0 as *const _ as *const c_void
    } else if id == clap_sys::ext::posix_fd_support::CLAP_EXT_POSIX_FD_SUPPORT {
        &HOST_FD.0 as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}
unsafe extern "C" fn host_request(_h: *const clap_host) {}

#[repr(C)]
#[derive(Clone, Copy)]
union EvSlot {
    header: clap_event_header,
    note: clap_event_note,
    param: clap_event_param_value,
}

unsafe extern "C" fn in_size(list: *const clap_input_events) -> u32 {
    let v = &*((*list).ctx as *const Vec<EvSlot>);
    v.len() as u32
}
unsafe extern "C" fn in_get(list: *const clap_input_events, i: u32) -> *const clap_event_header {
    let v = &*((*list).ctx as *const Vec<EvSlot>);
    v.get(i as usize).map(|e| e as *const EvSlot as *const clap_event_header).unwrap_or(std::ptr::null())
}
/// Output events: parameter values the plugin reports (its own GUI moved a control) are collected.
unsafe extern "C" fn out_push(list: *const clap_output_events, e: *const clap_event_header) -> bool {
    if list.is_null() || e.is_null() || (*list).ctx.is_null() {
        return true;
    }
    if (*e).space_id == CLAP_CORE_EVENT_SPACE_ID && (*e).type_ == CLAP_EVENT_PARAM_VALUE {
        let pv = &*(e as *const clap_event_param_value);
        let out = &mut *((*list).ctx as *mut Vec<(u32, f64)>);
        out.push((pv.param_id, pv.value));
    }
    true
}

unsafe extern "C" fn ostream_write(s: *const clap_ostream, buf: *const c_void, size: u64) -> i64 {
    let v = &mut *((*s).ctx as *mut Vec<u8>);
    v.extend_from_slice(std::slice::from_raw_parts(buf as *const u8, size as usize));
    size as i64
}
unsafe extern "C" fn istream_read(s: *const clap_istream, buf: *mut c_void, size: u64) -> i64 {
    let (data, pos) = &mut *((*s).ctx as *mut (Vec<u8>, usize));
    let n = (data.len() - *pos).min(size as usize);
    std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buf as *mut u8, n);
    *pos += n;
    n as i64
}

#[derive(Clone)]
struct ParamMeta {
    id: u32,
    name: String,
    min: f64,
    max: f64,
    default: f64,
    stepped: bool,
    automatable: bool,
}

/// A loaded CLAP plugin instance.
pub struct ClapPlugin {
    _host: Box<clap_host>,
    _strings: Vec<CString>,
    plugin: *const clap_plugin,
    params_ext: *const clap_plugin_params,
    gui: *const clap_plugin_gui,
    params: Vec<ParamMeta>,
    events: Vec<EvSlot>,
    pending: Vec<HostEvent>,
    scratch: Scratch,
    inputs: u32,
    outputs: u32,
    latency: u32,
    processing: bool,
    steady: i64,
    voices: usize,
    gui_open: bool,
    gui_resizable: bool,
    last_size: (u32, u32),
    state: Box<HostState>,
    window: Option<super::window::EditorWindow>,
    /// Parameter values reported by the plugin (GUI edits), plain values, filled during `process`.
    reported: Vec<(u32, f64)>,
    /// GUI edits for the app (normalised).
    gui_edits: Vec<(u32, f64)>,
    /// Last normalised value the host sent per parameter (unchanged project values are not re-sent).
    applied: std::collections::HashMap<u32, f64>,
    editor_error: Option<String>,
    pub info: PluginRef,
}

// SAFETY: the instance is used from one thread at a time (behind the renderer's mutex); CLAP plugins are
// designed to be driven from the audio thread for processing and from the main thread otherwise.
unsafe impl Send for ClapPlugin {}

impl ClapPlugin {
    pub fn load(r: &PluginRef, rate: f32, max_block: usize, _instrument: bool) -> Result<ClapPlugin> {
        let f = factory(&r.path)?;
        let strings = vec![CString::new("Dragon DAW").unwrap(), CString::new("ZenoxArt").unwrap(), CString::new("https://github.com/zenoxart/RedHiveFramework").unwrap(), CString::new(env!("CARGO_PKG_VERSION")).unwrap()];
        let state = Box::<HostState>::default();
        let host = Box::new(clap_host {
            clap_version: CLAP_VERSION,
            host_data: &*state as *const HostState as *mut c_void,
            name: strings[0].as_ptr(),
            vendor: strings[1].as_ptr(),
            url: strings[2].as_ptr(),
            version: strings[3].as_ptr(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_request),
            request_process: Some(host_request),
            request_callback: Some(host_request),
        });
        let id = CString::new(r.id.as_bytes()).map_err(|_| EngineError::Plugin("bad id".into()))?;
        // SAFETY: CLAP lifecycle per spec (create → init → activate), on the calling (main) thread.
        unsafe {
            let plugin = (*f).create_plugin.map(|c| c(f, &*host, id.as_ptr())).unwrap_or(std::ptr::null());
            if plugin.is_null() {
                return Err(EngineError::Plugin(format!("cannot create {}", r.id)));
            }
            if !(*plugin).init.map(|i| i(plugin)).unwrap_or(false) {
                return Err(EngineError::Plugin("init failed".into()));
            }
            let ext = |name: &CStr| (*plugin).get_extension.map(|g| g(plugin, name.as_ptr())).unwrap_or(std::ptr::null());
            let params_ext = ext(CLAP_EXT_PARAMS) as *const clap_plugin_params;
            let ports = ext(CLAP_EXT_AUDIO_PORTS) as *const clap_plugin_audio_ports;
            let latency_ext = ext(CLAP_EXT_LATENCY) as *const clap_plugin_latency;
            let state_ext = ext(CLAP_EXT_STATE) as *const clap_plugin_state;
            let gui = ext(CLAP_EXT_GUI) as *const clap_plugin_gui;
            let (mut inputs, mut outputs) = (1, 1);
            if !ports.is_null() {
                // Main input plus an optional second (sidechain) input port.
                inputs = (*ports).count.map(|c| c(plugin, true)).unwrap_or(0).min(2);
                outputs = (*ports).count.map(|c| c(plugin, false)).unwrap_or(1).min(1);
                let mut info: clap_audio_port_info = std::mem::zeroed();
                if outputs == 1 && (*ports).get.map(|g| g(plugin, 0, false, &mut info)).unwrap_or(false) && info.channel_count == 0 {
                    outputs = 0;
                }
            }
            let mut params = Vec::new();
            if !params_ext.is_null() {
                let n = (*params_ext).count.map(|c| c(plugin)).unwrap_or(0);
                for i in 0..n {
                    let mut pi: clap_param_info = std::mem::zeroed();
                    if (*params_ext).get_info.map(|g| g(plugin, i, &mut pi)).unwrap_or(false) {
                        params.push(ParamMeta {
                            id: pi.id,
                            name: cstr(pi.name.as_ptr()),
                            min: pi.min_value,
                            max: pi.max_value,
                            default: pi.default_value,
                            stepped: pi.flags & CLAP_PARAM_IS_STEPPED != 0,
                            automatable: pi.flags & CLAP_PARAM_IS_AUTOMATABLE != 0,
                        });
                    }
                }
            }
            if !r.state.is_empty() && !state_ext.is_null() {
                let mut ctx: (Vec<u8>, usize) = (r.state.clone(), 0);
                let stream = clap_istream { ctx: &mut ctx as *mut _ as *mut c_void, read: Some(istream_read) };
                if let Some(l) = (*state_ext).load {
                    l(plugin, &stream);
                }
            }
            if !(*plugin).activate.map(|a| a(plugin, rate as f64, 1, max_block as u32)).unwrap_or(false) {
                return Err(EngineError::Plugin("activate failed".into()));
            }
            let latency = if latency_ext.is_null() { 0 } else { (*latency_ext).get.map(|g| g(plugin)).unwrap_or(0) };
            let mut me = ClapPlugin {
                _host: host,
                _strings: strings,
                plugin,
                params_ext,
                gui,
                params,
                events: Vec::with_capacity(256),
                pending: Vec::with_capacity(256),
                scratch: Scratch::new(max_block),
                inputs,
                outputs,
                latency,
                processing: false,
                steady: 0,
                voices: 0,
                gui_open: false,
                gui_resizable: false,
                last_size: (0, 0),
                state,
                window: None,
                reported: Vec::with_capacity(64),
                gui_edits: Vec::new(),
                applied: std::collections::HashMap::new(),
                editor_error: None,
                info: r.clone(),
            };
            for (id, v) in &r.params {
                me.pending.push(HostEvent::Param { id: *id, value: me.plain(*id, *v) });
            }
            Ok(me)
        }
    }

    fn plain(&self, id: u32, normalized: f64) -> f64 {
        match self.params.iter().find(|p| p.id == id) {
            Some(p) => {
                let v = p.min + normalized.clamp(0.0, 1.0) * (p.max - p.min);
                if p.stepped {
                    v.round()
                } else {
                    v
                }
            }
            None => normalized,
        }
    }

    fn run(&mut self, n: usize) {
        self.scratch.ensure(n);
        self.events.clear();
        for e in self.pending.drain(..) {
            // SAFETY: plain-old-data event structs, fully initialised below.
            let mut slot: EvSlot = unsafe { std::mem::zeroed() };
            match e {
                HostEvent::NoteOn { key, velocity } => {
                    slot.note = note_event(CLAP_EVENT_NOTE_ON, key, velocity as f64);
                }
                HostEvent::NoteOff { key } => {
                    slot.note = note_event(CLAP_EVENT_NOTE_OFF, key, 0.0);
                }
                HostEvent::Param { id, value } => {
                    slot.param = clap_event_param_value {
                        header: clap_event_header { size: std::mem::size_of::<clap_event_param_value>() as u32, time: 0, space_id: CLAP_CORE_EVENT_SPACE_ID, type_: CLAP_EVENT_PARAM_VALUE, flags: 0 },
                        param_id: id,
                        cookie: std::ptr::null_mut(),
                        note_id: -1,
                        port_index: -1,
                        channel: -1,
                        key: -1,
                        value,
                    };
                }
            }
            self.events.push(slot);
        }
        let s = &mut self.scratch;
        let mut in_ptrs = [s.in_l.as_mut_ptr(), s.in_r.as_mut_ptr()];
        let mut sc_ptrs = [s.sc_l.as_mut_ptr(), s.sc_r.as_mut_ptr()];
        let mut out_ptrs = [s.out_l.as_mut_ptr(), s.out_r.as_mut_ptr()];
        let inb = [
            clap_audio_buffer { data32: in_ptrs.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 },
            clap_audio_buffer { data32: sc_ptrs.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 },
        ];
        let mut outb = clap_audio_buffer { data32: out_ptrs.as_mut_ptr(), data64: std::ptr::null_mut(), channel_count: 2, latency: 0, constant_mask: 0 };
        let in_events = clap_input_events { ctx: &self.events as *const Vec<EvSlot> as *mut c_void, size: Some(in_size), get: Some(in_get) };
        self.reported.clear();
        let out_events = clap_output_events { ctx: &mut self.reported as *mut Vec<(u32, f64)> as *mut c_void, try_push: Some(out_push) };
        let process = clap_process {
            steady_time: self.steady,
            frames_count: n as u32,
            transport: std::ptr::null(),
            audio_inputs: inb.as_ptr(),
            audio_outputs: &mut outb,
            audio_inputs_count: self.inputs,
            audio_outputs_count: self.outputs,
            in_events: &in_events,
            out_events: &out_events,
        };
        // SAFETY: CLAP processing per spec; buffers live for the call.
        unsafe {
            if !self.processing {
                self.processing = (*self.plugin).start_processing.map(|f| f(self.plugin)).unwrap_or(true);
            }
            if let Some(f) = (*self.plugin).process {
                f(self.plugin, &process);
            }
        }
        self.steady += n as i64;
        if !self.reported.is_empty() {
            let reported = std::mem::take(&mut self.reported);
            for (id, plain) in &reported {
                if let Some(p) = self.params.iter().find(|p| p.id == *id) {
                    let norm = if p.max > p.min { ((plain - p.min) / (p.max - p.min)).clamp(0.0, 1.0) } else { 0.0 };
                    if self.gui_edits.len() < 4096 {
                        self.gui_edits.push((*id, norm));
                    }
                }
            }
            self.reported = reported;
        }
    }

    fn params_list(&self) -> Vec<PluginParam> {
        self.params
            .iter()
            .map(|p| PluginParam {
                id: p.id,
                name: p.name.clone(),
                default: if p.max > p.min { (p.default - p.min) / (p.max - p.min) } else { 0.0 },
                steps: if p.stepped { (p.max - p.min).round() as u32 } else { 0 },
                automatable: p.automatable,
            })
            .collect()
    }

    fn value(&self, id: u32) -> Option<f64> {
        if self.params_ext.is_null() {
            return None;
        }
        let p = self.params.iter().find(|p| p.id == id)?;
        let mut v = 0.0;
        // SAFETY: params extension call.
        let ok = unsafe { (*self.params_ext).get_value.map(|g| g(self.plugin, id, &mut v)).unwrap_or(false) };
        ok.then(|| if p.max > p.min { (v - p.min) / (p.max - p.min) } else { 0.0 })
    }

    fn state(&mut self) -> Option<Vec<u8>> {
        // SAFETY: state extension call.
        unsafe {
            let ext = (*self.plugin).get_extension.map(|g| g(self.plugin, CLAP_EXT_STATE.as_ptr())).unwrap_or(std::ptr::null()) as *const clap_plugin_state;
            if ext.is_null() {
                return None;
            }
            let mut buf: Vec<u8> = Vec::new();
            let stream = clap_ostream { ctx: &mut buf as *mut Vec<u8> as *mut c_void, write: Some(ostream_write) };
            (*ext).save.map(|s| s(self.plugin, &stream)).unwrap_or(false).then_some(buf)
        }
    }

    fn set_state(&mut self, data: &[u8]) -> bool {
        // SAFETY: state extension call, on the main thread.
        unsafe {
            let ext = (*self.plugin).get_extension.map(|g| g(self.plugin, CLAP_EXT_STATE.as_ptr())).unwrap_or(std::ptr::null()) as *const clap_plugin_state;
            if ext.is_null() {
                return false;
            }
            let mut ctx: (Vec<u8>, usize) = (data.to_vec(), 0);
            let stream = clap_istream { ctx: &mut ctx as *mut _ as *mut c_void, read: Some(istream_read) };
            (*ext).load.map(|l| l(self.plugin, &stream)).unwrap_or(false)
        }
    }

    fn set_editor(&mut self, open: bool) -> bool {
        if self.gui.is_null() {
            if open {
                self.editor_error = Some("the plugin has no GUI (no gui extension)".into());
            }
            return false;
        }
        let api: &CStr = if cfg!(target_os = "windows") {
            CLAP_WINDOW_API_WIN32
        } else if cfg!(target_os = "macos") {
            CLAP_WINDOW_API_COCOA
        } else {
            CLAP_WINDOW_API_X11
        };
        // SAFETY: GUI extension lifecycle per the CLAP spec, on the UI thread.
        unsafe {
            let g = &*self.gui;
            if open && !self.gui_open {
                self.state.closed.set(false);
                let mut why: String;
                // Preferred: embed into a native window of ours.
                let embeddable = g.is_api_supported.map(|f| f(self.plugin, api.as_ptr(), false)).unwrap_or(false);
                if !embeddable {
                    why = format!("the GUI cannot be embedded ({} not supported)", api.to_string_lossy());
                } else if !g.create.map(|f| f(self.plugin, api.as_ptr(), false)).unwrap_or(false) {
                    why = "the plugin could not create its GUI".into();
                } else {
                    let (mut w, mut h) = (400u32, 300u32);
                    if let Some(f) = g.get_size {
                        f(self.plugin, &mut w, &mut h);
                    }
                    match super::window::EditorWindow::open(&self.info.name, w, h) {
                        Ok(mut win) => {
                            // HiDPI: tell the GUI the window's scale, then take its size.
                            let scale = win.scale() as f64;
                            if (scale - 1.0).abs() > 0.01 && g.set_scale.map(|f| f(self.plugin, scale)).unwrap_or(false) {
                                let (mut w2, mut h2) = (w, h);
                                if g.get_size.map(|f| f(self.plugin, &mut w2, &mut h2)).unwrap_or(false) && (w2, h2) != (w, h) {
                                    win.resize(w2, h2);
                                }
                            }
                            let handle = win.handle();
                            let specific = if cfg!(target_os = "linux") { clap_window_handle { x11: handle as std::ffi::c_ulong } } else { clap_window_handle { ptr: handle } };
                            let cw = clap_window { api: api.as_ptr(), specific };
                            if g.set_parent.map(|f| f(self.plugin, &cw)).unwrap_or(false) {
                                if let Some(f) = g.show {
                                    f(self.plugin);
                                }
                                // Editors that can follow a new size get a resizable window.
                                let resizable = g.can_resize.map(|f| f(self.plugin)).unwrap_or(false);
                                win.set_resizable(resizable, win.size());
                                self.gui_resizable = resizable;
                                self.last_size = win.size();
                                self.window = Some(win);
                                self.gui_open = true;
                                self.editor_error = None;
                                return true;
                            }
                            why = "the GUI refused the host window (set_parent failed)".into();
                        }
                        Err(e) => {
                            tracing::warn!("plugin editor window: {e}");
                            why = format!("cannot open the editor window: {e}");
                        }
                    }
                    if let Some(f) = g.destroy {
                        f(self.plugin);
                    }
                }
                // Fallback: the plugin's own floating window.
                if g.is_api_supported.map(|f| f(self.plugin, api.as_ptr(), true)).unwrap_or(false) && g.create.map(|f| f(self.plugin, api.as_ptr(), true)).unwrap_or(false) {
                    self.gui_open = g.show.map(|f| f(self.plugin)).unwrap_or(false);
                    if !self.gui_open {
                        if let Some(f) = g.destroy {
                            f(self.plugin);
                        }
                        why = format!("{why}; its floating window did not open either");
                    }
                }
                self.editor_error = (!self.gui_open).then(|| {
                    tracing::warn!("{}: {why}", self.info.name);
                    why
                });
            } else if !open && self.gui_open {
                if let Some(f) = g.hide {
                    f(self.plugin);
                }
                if let Some(f) = g.destroy {
                    f(self.plugin);
                }
                self.window = None;
                self.gui_open = false;
            }
        }
        self.gui_open
    }

    /// Parameter edits made in the plugin's own GUI since the last call (normalised). The project will store
    /// them; they are already applied, so they are not sent back.
    pub fn gui_edits(&mut self) -> Vec<(u32, f64)> {
        let edits = std::mem::take(&mut self.gui_edits);
        for (id, v) in &edits {
            self.applied.insert(*id, *v);
        }
        edits
    }

    /// Pumps the editor window and services the plugin's timers and file descriptors.
    fn idle_editor(&mut self) -> bool {
        if !self.gui_open {
            return false;
        }
        let closed_by_user = self.window.as_mut().map(|w| !w.pump()).unwrap_or(false);
        if closed_by_user || self.state.closed.get() {
            self.set_editor(false);
            return false;
        }
        // SAFETY: plugin extension calls on the UI (main) thread.
        unsafe {
            if let Some((w, h)) = self.state.resize.take() {
                if let Some(win) = &mut self.window {
                    win.resize(w, h);
                }
                if let Some(f) = (*self.gui).set_size {
                    f(self.plugin, w, h);
                }
                self.last_size = (w, h);
            }
            // The user (or full screen) resized the window: let the editor follow when it can.
            if let Some(win) = &mut self.window {
                let now = win.size();
                if now != self.last_size && now.0 > 0 && now.1 > 0 {
                    self.last_size = now;
                    if self.gui_resizable {
                        let (mut w, mut h) = now;
                        if let Some(f) = (*self.gui).adjust_size {
                            f(self.plugin, &mut w, &mut h);
                        }
                        if let Some(f) = (*self.gui).set_size {
                            f(self.plugin, w, h);
                        }
                    }
                }
            }
            let ext = |name: &CStr| (*self.plugin).get_extension.map(|g| g(self.plugin, name.as_ptr())).unwrap_or(std::ptr::null());
            let timer = ext(clap_sys::ext::timer_support::CLAP_EXT_TIMER_SUPPORT) as *const clap_sys::ext::timer_support::clap_plugin_timer_support;
            if !timer.is_null() {
                let now = std::time::Instant::now();
                let due: Vec<u32> = self
                    .state
                    .timers
                    .borrow_mut()
                    .iter_mut()
                    .filter(|t| now.duration_since(t.2) >= t.1)
                    .map(|t| {
                        t.2 = now;
                        t.0
                    })
                    .collect();
                for id in due {
                    if let Some(f) = (*timer).on_timer {
                        f(self.plugin, id);
                    }
                }
            }
            let fdx = ext(clap_sys::ext::posix_fd_support::CLAP_EXT_POSIX_FD_SUPPORT) as *const clap_sys::ext::posix_fd_support::clap_plugin_posix_fd_support;
            if !fdx.is_null() {
                let fds: Vec<(i32, u32)> = self.state.fds.borrow().clone();
                for (fd, flags) in fds {
                    if let Some(f) = (*fdx).on_fd {
                        f(self.plugin, fd, flags & clap_sys::ext::posix_fd_support::CLAP_POSIX_FD_READ);
                    }
                }
            }
        }
        true
    }

    /// Full screen for the embedded editor's window (the editor follows when it can resize).
    pub fn set_editor_fullscreen(&mut self, on: bool) -> bool {
        match &mut self.window {
            Some(w) => {
                w.set_fullscreen(on);
                on
            }
            None => false,
        }
    }

    /// Native window of an embedded editor (tests inspect its child windows).
    pub fn editor_window(&self) -> Option<&super::window::EditorWindow> {
        self.window.as_ref()
    }
}

fn note_event(kind: u16, key: u8, velocity: f64) -> clap_event_note {
    clap_event_note {
        header: clap_event_header { size: std::mem::size_of::<clap_event_note>() as u32, time: 0, space_id: CLAP_CORE_EVENT_SPACE_ID, type_: kind, flags: 0 },
        note_id: -1,
        port_index: 0,
        channel: 0,
        key: key as i16,
        velocity,
    }
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        // SAFETY: CLAP teardown order.
        unsafe {
            if self.gui_open {
                self.set_editor(false);
            }
            if self.processing {
                if let Some(f) = (*self.plugin).stop_processing {
                    f(self.plugin);
                }
            }
            if let Some(f) = (*self.plugin).deactivate {
                f(self.plugin);
            }
            if let Some(f) = (*self.plugin).destroy {
                f(self.plugin);
            }
        }
    }
}

impl Effect for ClapPlugin {
    fn set_param(&mut self, id: &str, value: f32) {
        if let Ok(pid) = id.parse::<u32>() {
            let n = value as f64;
            if self.applied.get(&pid).is_some_and(|old| (old - n).abs() < 1e-9) {
                return;
            }
            self.applied.insert(pid, n);
            let v = self.plain(pid, n);
            self.pending.push(HostEvent::Param { id: pid, value: v });
        }
    }
    fn reset(&mut self) {}
    fn process(&mut self, l: &mut [f32], r: &mut [f32], sc: Option<(&[f32], &[f32])>) {
        let n = l.len();
        self.scratch.ensure(n);
        self.scratch.set_sidechain(n, sc);
        self.scratch.in_l[..n].copy_from_slice(l);
        self.scratch.in_r[..n].copy_from_slice(r);
        self.scratch.out_l[..n].copy_from_slice(l);
        self.scratch.out_r[..n].copy_from_slice(r);
        self.run(n);
        l.copy_from_slice(&self.scratch.out_l[..n]);
        r.copy_from_slice(&self.scratch.out_r[..n]);
    }
    fn latency(&self) -> usize {
        self.latency as usize
    }
    fn plugin_params(&self) -> Vec<PluginParam> {
        self.params_list()
    }
    fn plugin_param(&self, id: u32) -> Option<f64> {
        self.value(id)
    }
    fn save_state(&mut self) -> Option<Vec<u8>> {
        self.state()
    }
    fn load_state(&mut self, data: &[u8]) -> bool {
        self.set_state(data)
    }
    fn editor(&mut self, open: bool) -> bool {
        self.set_editor(open)
    }
    fn editor_fullscreen(&mut self, on: bool) -> bool {
        self.set_editor_fullscreen(on)
    }
    fn idle(&mut self) -> bool {
        self.idle_editor()
    }
    fn editor_error(&self) -> Option<String> {
        self.editor_error.clone()
    }
    fn take_param_edits(&mut self) -> Vec<(u32, f64)> {
        self.gui_edits()
    }
}

impl InstrumentVoice for ClapPlugin {
    fn note_on(&mut self, n: NoteOn) {
        self.pending.push(HostEvent::NoteOn { key: (n.note as f32 + n.detune).round().clamp(0.0, 127.0) as u8, velocity: n.velocity });
        self.voices += 1;
    }
    fn note_off(&mut self, note: u8) {
        self.pending.push(HostEvent::NoteOff { key: note });
    }
    fn choke(&mut self, _s: f32) {
        for k in 0..128u8 {
            self.pending.push(HostEvent::NoteOff { key: k });
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        self.scratch.ensure(n);
        self.scratch.set_sidechain(n, None);
        self.scratch.in_l[..n].iter_mut().for_each(|x| *x = 0.0);
        self.scratch.in_r[..n].iter_mut().for_each(|x| *x = 0.0);
        self.scratch.out_l[..n].iter_mut().for_each(|x| *x = 0.0);
        self.scratch.out_r[..n].iter_mut().for_each(|x| *x = 0.0);
        self.run(n);
        for i in 0..n {
            l[i] += self.scratch.out_l[i];
            r[i] += self.scratch.out_r[i];
        }
    }
    fn update(&mut self, _m: &crate::model::Instrument, _c: &SampleCache) {}
    fn active_voices(&self) -> usize {
        // Plugins render their own tails: always process.
        1
    }
    fn set_param(&mut self, id: &str, value: f32) {
        Effect::set_param(self, id, value);
    }
    fn plugin_params(&self) -> Vec<PluginParam> {
        self.params_list()
    }
    fn plugin_param(&self, id: u32) -> Option<f64> {
        self.value(id)
    }
    fn save_state(&mut self) -> Option<Vec<u8>> {
        self.state()
    }
    fn load_state(&mut self, data: &[u8]) -> bool {
        self.set_state(data)
    }
    fn editor(&mut self, open: bool) -> bool {
        self.set_editor(open)
    }
    fn editor_fullscreen(&mut self, on: bool) -> bool {
        self.set_editor_fullscreen(on)
    }
    fn idle(&mut self) -> bool {
        self.idle_editor()
    }
    fn editor_error(&self) -> Option<String> {
        self.editor_error.clone()
    }
    fn take_param_edits(&mut self) -> Vec<(u32, f64)> {
        self.gui_edits()
    }
}
