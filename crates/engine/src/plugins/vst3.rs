//! VST3 host (via the `vst3` crate's COM bindings).
//!
//! The host side implements the few interfaces a processor needs from its host: parameter change
//! queues, an event list for notes, a memory stream for state, and `IHostApplication`.

use super::{library, HostEvent, PluginInfo, Scratch};
use crate::effects::{Effect, PluginParam};
use crate::instruments::{InstrumentVoice, NoteOn};
use crate::model::{PluginFormat, PluginRef};
use crate::samples::SampleCache;
use crate::{EngineError, Result};
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void, CStr};
use std::path::{Path, PathBuf};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface};

const AUDIO_MODULE_CLASS: &str = "Audio Module Class";

fn chars(s: &[c_char]) -> String {
    let bytes: Vec<u8> = s.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).to_string()
}

fn wide(s: &[TChar]) -> String {
    let v: Vec<u16> = s.iter().take_while(|c| **c != 0).map(|c| *c as u16).collect();
    String::from_utf16_lossy(&v)
}

fn hex(cid: &TUID) -> String {
    cid.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

fn unhex(s: &str) -> Option<TUID> {
    if s.len() != 32 {
        return None;
    }
    let mut out: TUID = [0; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()? as c_char;
    }
    Some(out)
}

/// The module binary inside a `.vst3` bundle (or the file itself for single-file plugins).
fn binary(path: &Path) -> PathBuf {
    if path.is_dir() {
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let c = path.join("Contents");
        let candidates = if cfg!(target_os = "windows") {
            vec![c.join("x86_64-win").join(format!("{name}.vst3"))]
        } else if cfg!(target_os = "macos") {
            vec![c.join("MacOS").join(&name)]
        } else {
            vec![c.join(format!("{}-linux", std::env::consts::ARCH)).join(format!("{name}.so"))]
        };
        for cand in candidates {
            if cand.exists() {
                return cand;
            }
        }
    }
    path.to_path_buf()
}

fn factory(path: &Path) -> Result<ComPtr<IPluginFactory>> {
    let bin = binary(path);
    let lib = library(&bin)?;
    // SAFETY: VST3 module entry points per the SDK (called once; the library stays loaded).
    unsafe {
        #[cfg(target_os = "linux")]
        if let Ok(f) = lib.get::<unsafe extern "system" fn(*mut c_void) -> bool>(b"ModuleEntry\0") {
            static ENTERED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> = std::sync::OnceLock::new();
            let mut set = ENTERED.get_or_init(Default::default).lock().unwrap();
            if set.insert(bin.clone()) && !f(std::ptr::null_mut()) {
                return Err(EngineError::Plugin("ModuleEntry failed".into()));
            }
        }
        #[cfg(target_os = "windows")]
        if let Ok(f) = lib.get::<unsafe extern "system" fn() -> bool>(b"InitDll\0") {
            f();
        }
        let get = lib.get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0").map_err(|e| EngineError::Plugin(format!("not a VST3 module: {e}")))?;
        ComPtr::from_raw(get()).ok_or_else(|| EngineError::Plugin("null factory".into()))
    }
}

pub fn scan(path: &Path) -> Result<Vec<PluginInfo>> {
    let f = factory(path)?;
    let f2 = f.cast::<IPluginFactory2>();
    let mut vendor = String::new();
    let mut out = Vec::new();
    // SAFETY: factory calls with out-structs we own.
    unsafe {
        let mut fi: PFactoryInfo = std::mem::zeroed();
        if f.getFactoryInfo(&mut fi) == kResultOk {
            vendor = chars(&fi.vendor);
        }
        for i in 0..f.countClasses() {
            let mut ci: PClassInfo = std::mem::zeroed();
            if f.getClassInfo(i, &mut ci) != kResultOk || chars(&ci.category) != AUDIO_MODULE_CLASS {
                continue;
            }
            let mut sub = String::new();
            let mut version = String::new();
            let mut v = vendor.clone();
            if let Some(f2) = &f2 {
                let mut c2: PClassInfo2 = std::mem::zeroed();
                if f2.getClassInfo2(i, &mut c2) == kResultOk {
                    sub = chars(&c2.subCategories);
                    version = chars(&c2.version);
                    let cv = chars(&c2.vendor);
                    if !cv.is_empty() {
                        v = cv;
                    }
                }
            }
            out.push(PluginInfo { format: PluginFormat::Vst3, path: path.to_path_buf(), id: hex(&ci.cid), name: chars(&ci.name), vendor: v, version, instrument: sub.split('|').any(|s| s == "Instrument"), category: sub });
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------- host side

struct HostApp;
impl Class for HostApp {
    type Interfaces = (IHostApplication,);
}
impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let n = &mut *name;
        for (i, c) in "Dragon DAW".encode_utf16().chain(std::iter::once(0)).enumerate() {
            n[i] = c as TChar;
        }
        kResultOk
    }
    unsafe fn createInstance(&self, _cid: *mut TUID, _iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        *obj = std::ptr::null_mut();
        kNoInterface
    }
}

struct Queue {
    id: Cell<ParamID>,
    value: Cell<f64>,
}
impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}
impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }
    unsafe fn getPointCount(&self) -> int32 {
        1
    }
    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        *offset = 0;
        *value = self.value.get();
        kResultTrue
    }
    unsafe fn addPoint(&self, _o: int32, _v: ParamValue, _i: *mut int32) -> tresult {
        kResultFalse
    }
}

struct Changes {
    queues: Vec<ComWrapper<Queue>>,
    used: Cell<usize>,
}
impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}
impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        self.used.get() as int32
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if index < 0 || index as usize >= self.used.get() {
            return std::ptr::null_mut();
        }
        self.queues[index as usize].as_com_ref::<IParamValueQueue>().map(|r| r.as_ptr()).unwrap_or(std::ptr::null_mut())
    }
    unsafe fn addParameterData(&self, _id: *const ParamID, _index: *mut int32) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
}

struct Events {
    list: RefCell<Vec<Event>>,
}
impl Class for Events {
    type Interfaces = (IEventList,);
}
impl IEventListTrait for Events {
    unsafe fn getEventCount(&self) -> int32 {
        self.list.borrow().len() as int32
    }
    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        match self.list.borrow().get(index as usize) {
            Some(ev) => {
                *e = *ev;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn addEvent(&self, _e: *mut Event) -> tresult {
        kResultOk
    }
}

struct Stream {
    data: RefCell<Vec<u8>>,
    pos: Cell<usize>,
}
impl Class for Stream {
    type Interfaces = (IBStream,);
}
impl IBStreamTrait for Stream {
    unsafe fn read(&self, buffer: *mut c_void, n: int32, read: *mut int32) -> tresult {
        let d = self.data.borrow();
        let p = self.pos.get();
        let k = (d.len().saturating_sub(p)).min(n.max(0) as usize);
        std::ptr::copy_nonoverlapping(d.as_ptr().add(p), buffer as *mut u8, k);
        self.pos.set(p + k);
        if !read.is_null() {
            *read = k as int32;
        }
        kResultOk
    }
    unsafe fn write(&self, buffer: *mut c_void, n: int32, written: *mut int32) -> tresult {
        let mut d = self.data.borrow_mut();
        let p = self.pos.get();
        let n = n.max(0) as usize;
        if d.len() < p + n {
            d.resize(p + n, 0);
        }
        std::ptr::copy_nonoverlapping(buffer as *const u8, d.as_mut_ptr().add(p), n);
        self.pos.set(p + n);
        if !written.is_null() {
            *written = n as int32;
        }
        kResultOk
    }
    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let len = self.data.borrow().len() as i64;
        let base = match mode {
            0 => 0,
            1 => self.pos.get() as i64,
            _ => len,
        };
        let np = (base + pos).clamp(0, len);
        self.pos.set(np as usize);
        if !result.is_null() {
            *result = np;
        }
        kResultOk
    }
    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        if !pos.is_null() {
            *pos = self.pos.get() as int64;
        }
        kResultOk
    }
}

fn stream(data: Vec<u8>) -> ComWrapper<Stream> {
    ComWrapper::new(Stream { data: RefCell::new(data), pos: Cell::new(0) })
}

/// State layout in the project: `u32 component_len | component | controller`.
fn pack(component: &[u8], controller: &[u8]) -> Vec<u8> {
    let mut v = (component.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(component);
    v.extend_from_slice(controller);
    v
}

fn unpack(state: &[u8]) -> (Vec<u8>, Vec<u8>) {
    if state.len() < 4 {
        return (Vec::new(), Vec::new());
    }
    let n = (u32::from_le_bytes([state[0], state[1], state[2], state[3]]) as usize).min(state.len() - 4);
    (state[4..4 + n].to_vec(), state[4 + n..].to_vec())
}

#[derive(Clone)]
struct ParamMeta {
    id: u32,
    name: String,
    default: f64,
    steps: u32,
    automatable: bool,
}

/// A loaded VST3 plugin instance (component + processor + controller).
pub struct Vst3Plugin {
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    _host: ComWrapper<HostApp>,
    changes: ComWrapper<Changes>,
    events: ComWrapper<Events>,
    pending: Vec<HostEvent>,
    params: Vec<ParamMeta>,
    scratch: Scratch,
    inputs: i32,
    outputs: i32,
    event_inputs: i32,
    view: Option<ComPtr<IPlugView>>,
    pub info: PluginRef,
}

// SAFETY: driven from one thread at a time behind the renderer's mutex.
unsafe impl Send for Vst3Plugin {}

impl Vst3Plugin {
    pub fn load(r: &PluginRef, rate: f32, max_block: usize, _instrument: bool) -> Result<Vst3Plugin> {
        let f = factory(&r.path)?;
        let cid = unhex(&r.id).ok_or_else(|| EngineError::Plugin(format!("bad class id {}", r.id)))?;
        let host = ComWrapper::new(HostApp);
        let host_unknown = host.as_com_ref::<IHostApplication>().unwrap().as_ptr() as *mut FUnknown;
        // SAFETY: VST3 lifecycle per SDK: create → initialize → (controller) → setupProcessing → activate.
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();
            let iid = IComponent::IID;
            if f.createInstance(cid.as_ptr(), iid.as_ptr() as *const c_char, &mut obj) != kResultOk {
                return Err(EngineError::Plugin(format!("cannot create {}", r.name)));
            }
            let component = ComPtr::<IComponent>::from_raw(obj as *mut IComponent).ok_or_else(|| EngineError::Plugin("null component".into()))?;
            if component.initialize(host_unknown) != kResultOk {
                return Err(EngineError::Plugin("initialize failed".into()));
            }
            let processor = component.cast::<IAudioProcessor>().ok_or_else(|| EngineError::Plugin("no IAudioProcessor".into()))?;

            // Controller: same object, or a separate class.
            let mut separate_controller = false;
            let controller = match component.cast::<IEditController>() {
                Some(c) => Some(c),
                None => {
                    let mut ccid: TUID = [0; 16];
                    let mut cobj: *mut c_void = std::ptr::null_mut();
                    let ciid = IEditController::IID;
                    if component.getControllerClassId(&mut ccid) == kResultOk && f.createInstance(ccid.as_ptr(), ciid.as_ptr() as *const c_char, &mut cobj) == kResultOk {
                        let c = ComPtr::<IEditController>::from_raw(cobj as *mut IEditController);
                        if let Some(c) = &c {
                            c.initialize(host_unknown);
                            separate_controller = true;
                            if let (Some(a), Some(b)) = (component.cast::<IConnectionPoint>(), c.cast::<IConnectionPoint>()) {
                                a.connect(b.as_ptr());
                                b.connect(a.as_ptr());
                            }
                        }
                        c
                    } else {
                        None
                    }
                }
            };

            // State.
            let (cs, ks) = unpack(&r.state);
            if !cs.is_empty() {
                let s = stream(cs.clone());
                component.setState(s.as_com_ref::<IBStream>().unwrap().as_ptr());
                if let Some(c) = &controller {
                    let s2 = stream(cs);
                    c.setComponentState(s2.as_com_ref::<IBStream>().unwrap().as_ptr());
                    if !ks.is_empty() {
                        let s3 = stream(ks);
                        c.setState(s3.as_com_ref::<IBStream>().unwrap().as_ptr());
                    }
                }
            }

            // Buses: activate main stereo in/out and the first event input.
            let inputs = component.getBusCount(MediaTypes_::kAudio as MediaType, BusDirections_::kInput as BusDirection).min(1);
            let outputs = component.getBusCount(MediaTypes_::kAudio as MediaType, BusDirections_::kOutput as BusDirection).min(1);
            let event_inputs = component.getBusCount(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection).min(1);
            if inputs > 0 {
                component.activateBus(MediaTypes_::kAudio as MediaType, BusDirections_::kInput as BusDirection, 0, 1);
            }
            if outputs > 0 {
                component.activateBus(MediaTypes_::kAudio as MediaType, BusDirections_::kOutput as BusDirection, 0, 1);
            }
            if event_inputs > 0 {
                component.activateBus(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection, 0, 1);
            }
            let mut stereo: SpeakerArrangement = 3; // kStereo = L | R
            let mut stereo_out: SpeakerArrangement = 3;
            processor.setBusArrangements(if inputs > 0 { &mut stereo } else { std::ptr::null_mut() }, inputs, if outputs > 0 { &mut stereo_out } else { std::ptr::null_mut() }, outputs);
            let mut setup = ProcessSetup { processMode: ProcessModes_::kRealtime as int32, symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32, maxSamplesPerBlock: max_block as int32, sampleRate: rate as f64 };
            if processor.setupProcessing(&mut setup) != kResultOk {
                return Err(EngineError::Plugin("setupProcessing failed".into()));
            }
            component.setActive(1);
            processor.setProcessing(1);

            let mut params = Vec::new();
            if let Some(c) = &controller {
                for i in 0..c.getParameterCount() {
                    let mut pi: ParameterInfo = std::mem::zeroed();
                    if c.getParameterInfo(i, &mut pi) == kResultOk && pi.flags & ParameterInfo_::ParameterFlags_::kIsHidden == 0 {
                        params.push(ParamMeta { id: pi.id, name: wide(&pi.title), default: pi.defaultNormalizedValue, steps: pi.stepCount.max(0) as u32, automatable: pi.flags & ParameterInfo_::ParameterFlags_::kCanAutomate != 0 });
                    }
                }
            }
            let queues = (0..params.len().max(1) + 64).map(|_| ComWrapper::new(Queue { id: Cell::new(0), value: Cell::new(0.0) })).collect();
            let mut me = Vst3Plugin {
                component,
                processor,
                controller,
                separate_controller,
                _host: host,
                changes: ComWrapper::new(Changes { queues, used: Cell::new(0) }),
                events: ComWrapper::new(Events { list: RefCell::new(Vec::with_capacity(256)) }),
                pending: Vec::with_capacity(256),
                params,
                scratch: Scratch::new(max_block),
                inputs,
                outputs,
                event_inputs,
                view: None,
                info: r.clone(),
            };
            for (id, v) in &r.params {
                me.pending.push(HostEvent::Param { id: *id, value: *v });
            }
            Ok(me)
        }
    }

    fn run(&mut self, n: usize) {
        self.scratch.ensure(n);
        // Parameters: last value per id wins within a block.
        let mut used = 0usize;
        {
            let mut evs = self.events.list.borrow_mut();
            evs.clear();
            for e in self.pending.drain(..) {
                // SAFETY: Event is plain data; the union member we set is the one `type` names.
                let mut ev: Event = unsafe { std::mem::zeroed() };
                match e {
                    HostEvent::NoteOn { key, velocity } => {
                        ev.r#type = Event_::EventTypes_::kNoteOnEvent as u16;
                        ev.__field0.noteOn = NoteOnEvent { channel: 0, pitch: key as i16, tuning: 0.0, velocity, length: 0, noteId: -1 };
                        evs.push(ev);
                    }
                    HostEvent::NoteOff { key } => {
                        ev.r#type = Event_::EventTypes_::kNoteOffEvent as u16;
                        ev.__field0.noteOff = NoteOffEvent { channel: 0, pitch: key as i16, velocity: 0.0, noteId: -1, tuning: 0.0 };
                        evs.push(ev);
                    }
                    HostEvent::Param { id, value } => {
                        let q = &self.changes.queues;
                        let existing = (0..used).find(|i| q[*i].id.get() == id);
                        let i = match existing {
                            Some(i) => i,
                            None if used < q.len() => {
                                used += 1;
                                used - 1
                            }
                            None => continue,
                        };
                        q[i].id.set(id);
                        q[i].value.set(value.clamp(0.0, 1.0));
                        if let Some(c) = &self.controller {
                            // SAFETY: keeps the controller's view of the value in sync.
                            unsafe { c.setParamNormalized(id, value.clamp(0.0, 1.0)) };
                        }
                    }
                }
            }
        }
        self.changes.used.set(used);
        let s = &mut self.scratch;
        let mut in_ptrs = [s.in_l.as_mut_ptr(), s.in_r.as_mut_ptr()];
        let mut out_ptrs = [s.out_l.as_mut_ptr(), s.out_r.as_mut_ptr()];
        let mut inb = AudioBusBuffers { numChannels: 2, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: in_ptrs.as_mut_ptr() } };
        let mut outb = AudioBusBuffers { numChannels: 2, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: out_ptrs.as_mut_ptr() } };
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: n as int32,
            numInputs: self.inputs,
            numOutputs: self.outputs,
            inputs: if self.inputs > 0 { &mut inb } else { std::ptr::null_mut() },
            outputs: if self.outputs > 0 { &mut outb } else { std::ptr::null_mut() },
            inputParameterChanges: self.changes.as_com_ref::<IParameterChanges>().unwrap().as_ptr(),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: if self.event_inputs > 0 { self.events.as_com_ref::<IEventList>().unwrap().as_ptr() } else { std::ptr::null_mut() },
            outputEvents: std::ptr::null_mut(),
            processContext: std::ptr::null_mut(),
        };
        // SAFETY: buffers and host objects outlive the call.
        unsafe {
            self.processor.process(&mut data);
        }
    }

    fn params_list(&self) -> Vec<PluginParam> {
        self.params.iter().map(|p| PluginParam { id: p.id, name: p.name.clone(), default: p.default, steps: p.steps, automatable: p.automatable }).collect()
    }

    fn value(&self, id: u32) -> Option<f64> {
        let c = self.controller.as_ref()?;
        self.params.iter().any(|p| p.id == id).then(|| unsafe { c.getParamNormalized(id) })
    }

    fn state(&mut self) -> Option<Vec<u8>> {
        let cs = stream(Vec::new());
        // SAFETY: state calls into our own stream object.
        unsafe {
            if self.component.getState(cs.as_com_ref::<IBStream>().unwrap().as_ptr()) != kResultOk {
                return None;
            }
            let comp = cs.data.borrow().clone();
            let mut ctrl = Vec::new();
            if self.separate_controller {
                if let Some(c) = &self.controller {
                    let ks = stream(Vec::new());
                    if c.getState(ks.as_com_ref::<IBStream>().unwrap().as_ptr()) == kResultOk {
                        ctrl = ks.data.borrow().clone();
                    }
                }
            }
            Some(pack(&comp, &ctrl))
        }
    }

    /// Plugin editors need a native parent window; the app embeds them through
    /// [`Vst3Plugin::attach_editor`]. `editor(true)` reports whether an editor exists.
    fn set_editor(&mut self, open: bool) -> bool {
        if !open {
            if let Some(v) = self.view.take() {
                // SAFETY: detaching the view we attached.
                unsafe { v.removed() };
            }
            return false;
        }
        let Some(c) = &self.controller else { return false };
        // SAFETY: createView("editor") per SDK.
        let v = unsafe { ComPtr::<IPlugView>::from_raw(c.createView(c"editor".as_ptr())) };
        let has = v.is_some();
        self.view = v;
        has
    }

    /// Attaches the editor to a native parent window handle (HWND / NSView / X11 window id).
    pub fn attach_editor(&mut self, parent: *mut c_void) -> Option<(i32, i32)> {
        if self.view.is_none() && !self.set_editor(true) {
            return None;
        }
        let v = self.view.as_ref()?;
        let platform: &CStr = if cfg!(target_os = "windows") {
            c"HWND"
        } else if cfg!(target_os = "macos") {
            c"NSView"
        } else {
            c"X11EmbedWindowID"
        };
        // SAFETY: IPlugView calls with a valid parent handle supplied by the app.
        unsafe {
            if v.isPlatformTypeSupported(platform.as_ptr()) != kResultOk {
                return None;
            }
            if v.attached(parent, platform.as_ptr()) != kResultOk {
                return None;
            }
            let mut rect: ViewRect = std::mem::zeroed();
            v.getSize(&mut rect);
            Some((rect.right - rect.left, rect.bottom - rect.top))
        }
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        // SAFETY: SDK teardown order.
        unsafe {
            if let Some(v) = self.view.take() {
                v.removed();
            }
            self.processor.setProcessing(0);
            self.component.setActive(0);
            if self.separate_controller {
                if let Some(c) = &self.controller {
                    if let (Some(a), Some(b)) = (self.component.cast::<IConnectionPoint>(), c.cast::<IConnectionPoint>()) {
                        a.disconnect(b.as_ptr());
                        b.disconnect(a.as_ptr());
                    }
                    c.terminate();
                }
            }
            self.component.terminate();
        }
    }
}

impl Effect for Vst3Plugin {
    fn set_param(&mut self, id: &str, value: f32) {
        if let Ok(pid) = id.parse::<u32>() {
            self.pending.push(HostEvent::Param { id: pid, value: value as f64 });
        }
    }
    fn reset(&mut self) {}
    fn process(&mut self, l: &mut [f32], r: &mut [f32], _sc: Option<(&[f32], &[f32])>) {
        let n = l.len();
        self.scratch.ensure(n);
        self.scratch.in_l[..n].copy_from_slice(l);
        self.scratch.in_r[..n].copy_from_slice(r);
        self.scratch.out_l[..n].copy_from_slice(l);
        self.scratch.out_r[..n].copy_from_slice(r);
        self.run(n);
        l.copy_from_slice(&self.scratch.out_l[..n]);
        r.copy_from_slice(&self.scratch.out_r[..n]);
    }
    fn latency(&self) -> usize {
        // SAFETY: simple query.
        unsafe { self.processor.getLatencySamples() as usize }
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
    fn editor(&mut self, open: bool) -> bool {
        self.set_editor(open)
    }
}

impl InstrumentVoice for Vst3Plugin {
    fn note_on(&mut self, n: NoteOn) {
        self.pending.push(HostEvent::NoteOn { key: (n.note as f32 + n.detune).round().clamp(0.0, 127.0) as u8, velocity: n.velocity });
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
        for b in [&mut self.scratch.in_l, &mut self.scratch.in_r, &mut self.scratch.out_l, &mut self.scratch.out_r] {
            b[..n].iter_mut().for_each(|x| *x = 0.0);
        }
        self.run(n);
        for i in 0..n {
            l[i] += self.scratch.out_l[i];
            r[i] += self.scratch.out_r[i];
        }
    }
    fn update(&mut self, _m: &crate::model::Instrument, _c: &SampleCache) {}
    fn active_voices(&self) -> usize {
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
    fn editor(&mut self, open: bool) -> bool {
        self.set_editor(open)
    }
}

#[allow(dead_code)]
fn _assert_comref(_: ComRef<'_, IBStream>) {}
