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
use vst3::Steinberg::Linux::{IEventHandlerTrait, ITimerHandlerTrait};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface};

const AUDIO_MODULE_CLASS: &str = "Audio Module Class";

fn chars(s: &[c_char]) -> String {
    let bytes: Vec<u8> = s.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).to_string()
}

fn wide(s: &[TChar]) -> String {
    let v: Vec<u16> = s.iter().take_while(|c| **c != 0).copied().collect();
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
            out.push(PluginInfo {
                format: PluginFormat::Vst3,
                path: path.to_path_buf(),
                id: hex(&ci.cid),
                name: chars(&ci.name),
                vendor: v,
                version,
                instrument: sub.split('|').any(|s| s == "Instrument"),
                category: sub,
            });
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
    /// Plugins with a separate edit controller talk to their processor through `IMessage`s the host creates
    /// (`IConnectionPoint::notify`); many refuse to build their GUI without them.
    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        if obj.is_null() || cid.is_null() {
            return kInvalidArgument;
        }
        *obj = std::ptr::null_mut();
        let class = std::slice::from_raw_parts(cid as *const u8, 16);
        let wanted = if iid.is_null() { class } else { std::slice::from_raw_parts(iid as *const u8, 16) };
        let is = |id: &[u8; 16]| class == id.as_slice() || wanted == id.as_slice();
        if is(&IMessage::IID) {
            let m = ComWrapper::new(Message { id: RefCell::new(None), attrs: ComWrapper::new(Attributes::default()) });
            *obj = m.to_com_ptr::<IMessage>().unwrap().into_raw() as *mut c_void;
            kResultOk
        } else if is(&IAttributeList::IID) {
            let a = ComWrapper::new(Attributes::default());
            *obj = a.to_com_ptr::<IAttributeList>().unwrap().into_raw() as *mut c_void;
            kResultOk
        } else {
            kNoInterface
        }
    }
}

/// Host-created message (`IMessage`).
struct Message {
    id: RefCell<Option<std::ffi::CString>>,
    attrs: ComWrapper<Attributes>,
}
impl Class for Message {
    type Interfaces = (IMessage,);
}
impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.borrow().as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null())
    }
    unsafe fn setMessageID(&self, id: FIDString) {
        *self.id.borrow_mut() = if id.is_null() { None } else { Some(CStr::from_ptr(id).to_owned()) };
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attrs.as_com_ref::<IAttributeList>().unwrap().as_ptr()
    }
}

#[derive(Clone)]
enum Attr {
    Int(i64),
    Float(f64),
    Str(Vec<TChar>),
    Bin(Vec<u8>),
}

/// Host-created attribute list (`IAttributeList`) carried by messages.
#[derive(Default)]
struct Attributes {
    map: RefCell<std::collections::HashMap<Vec<u8>, Attr>>,
}
impl Class for Attributes {
    type Interfaces = (IAttributeList,);
}
fn attr_key(id: IAttributeList_::AttrID) -> Option<Vec<u8>> {
    // SAFETY: attribute ids are NUL-terminated C strings.
    (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_bytes().to_vec())
}
impl IAttributeListTrait for Attributes {
    unsafe fn setInt(&self, id: IAttributeList_::AttrID, value: int64) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        self.map.borrow_mut().insert(k, Attr::Int(value));
        kResultOk
    }
    unsafe fn getInt(&self, id: IAttributeList_::AttrID, value: *mut int64) -> tresult {
        match attr_key(id).and_then(|k| self.map.borrow().get(&k).cloned()) {
            Some(Attr::Int(v)) if !value.is_null() => {
                *value = v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setFloat(&self, id: IAttributeList_::AttrID, value: f64) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        self.map.borrow_mut().insert(k, Attr::Float(value));
        kResultOk
    }
    unsafe fn getFloat(&self, id: IAttributeList_::AttrID, value: *mut f64) -> tresult {
        match attr_key(id).and_then(|k| self.map.borrow().get(&k).cloned()) {
            Some(Attr::Float(v)) if !value.is_null() => {
                *value = v;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setString(&self, id: IAttributeList_::AttrID, string: *const TChar) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut v = Vec::new();
        let mut i = 0;
        while *string.add(i) != 0 {
            v.push(*string.add(i));
            i += 1;
        }
        self.map.borrow_mut().insert(k, Attr::Str(v));
        kResultOk
    }
    unsafe fn getString(&self, id: IAttributeList_::AttrID, string: *mut TChar, size_in_bytes: uint32) -> tresult {
        match attr_key(id).and_then(|k| self.map.borrow().get(&k).cloned()) {
            Some(Attr::Str(v)) if !string.is_null() && size_in_bytes >= 2 => {
                let cap = size_in_bytes as usize / std::mem::size_of::<TChar>() - 1;
                let n = v.len().min(cap);
                std::ptr::copy_nonoverlapping(v.as_ptr(), string, n);
                *string.add(n) = 0;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setBinary(&self, id: IAttributeList_::AttrID, data: *const c_void, size_in_bytes: uint32) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        let v = if data.is_null() { Vec::new() } else { std::slice::from_raw_parts(data as *const u8, size_in_bytes as usize).to_vec() };
        self.map.borrow_mut().insert(k, Attr::Bin(v));
        kResultOk
    }
    unsafe fn getBinary(&self, id: IAttributeList_::AttrID, data: *mut *const c_void, size_in_bytes: *mut uint32) -> tresult {
        let Some(k) = attr_key(id) else { return kInvalidArgument };
        // The data must stay valid while the message lives: hand out a pointer into the stored vector.
        match self.map.borrow().get(&k) {
            Some(Attr::Bin(v)) if !data.is_null() && !size_in_bytes.is_null() => {
                *data = v.as_ptr() as *const c_void;
                *size_in_bytes = v.len() as uint32;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

/// Host side of `IComponentHandler`: the plugin's own GUI reports parameter edits here. They are queued for
/// the audio processor (applied in the next block) and for the app (stored in the project).
#[derive(Default)]
struct Handler {
    /// Edits for the processor.
    audio: std::sync::Mutex<Vec<(u32, f64)>>,
    /// Edits for the project.
    ui: std::sync::Mutex<Vec<(u32, f64)>>,
    /// `restartComponent` asked for a latency / parameter refresh.
    restart: std::sync::atomic::AtomicI32,
}
impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}
impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, id: ParamID, value: ParamValue) -> tresult {
        if let Ok(mut q) = self.audio.lock() {
            q.push((id, value));
        }
        if let Ok(mut q) = self.ui.lock() {
            q.push((id, value));
        }
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, flags: int32) -> tresult {
        self.restart.fetch_or(flags, std::sync::atomic::Ordering::Relaxed);
        kResultOk
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
    handler: ComWrapper<Handler>,
    /// Last value the host sent per parameter, so unchanged project values are not re-sent on every
    /// recompile (they would undo edits made in the plugin's own GUI).
    applied: std::collections::HashMap<u32, f64>,
    editor_error: Option<String>,
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
    editor: Option<Editor>,
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

            // State. With a separate controller the controller always gets the component's state (SDK host
            // behaviour); some plugins build their editor from it.
            let (mut cs, ks) = unpack(&r.state);
            if cs.is_empty() && separate_controller {
                let s = stream(Vec::new());
                if component.getState(s.as_com_ref::<IBStream>().unwrap().as_ptr()) == kResultOk {
                    let data = s.data.borrow().clone();
                    if let Some(c) = &controller {
                        let s2 = stream(data);
                        c.setComponentState(s2.as_com_ref::<IBStream>().unwrap().as_ptr());
                    }
                }
                cs.clear();
            }
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
            // Main input plus an optional aux (sidechain) input bus.
            let inputs = component.getBusCount(MediaTypes_::kAudio as MediaType, BusDirections_::kInput as BusDirection).min(2);
            let outputs = component.getBusCount(MediaTypes_::kAudio as MediaType, BusDirections_::kOutput as BusDirection).min(1);
            let event_inputs = component.getBusCount(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection).min(1);
            for bus in 0..inputs {
                component.activateBus(MediaTypes_::kAudio as MediaType, BusDirections_::kInput as BusDirection, bus, 1);
            }
            if outputs > 0 {
                component.activateBus(MediaTypes_::kAudio as MediaType, BusDirections_::kOutput as BusDirection, 0, 1);
            }
            if event_inputs > 0 {
                component.activateBus(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection, 0, 1);
            }
            let mut stereo: [SpeakerArrangement; 2] = [3, 3]; // kStereo = L | R
            let mut stereo_out: SpeakerArrangement = 3;
            processor.setBusArrangements(if inputs > 0 { stereo.as_mut_ptr() } else { std::ptr::null_mut() }, inputs, if outputs > 0 { &mut stereo_out } else { std::ptr::null_mut() }, outputs);
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
                        params.push(ParamMeta {
                            id: pi.id,
                            name: wide(&pi.title),
                            default: pi.defaultNormalizedValue,
                            steps: pi.stepCount.max(0) as u32,
                            automatable: pi.flags & ParameterInfo_::ParameterFlags_::kCanAutomate != 0,
                        });
                    }
                }
            }
            let handler = ComWrapper::new(Handler::default());
            if let Some(c) = &controller {
                c.setComponentHandler(handler.as_com_ref::<IComponentHandler>().unwrap().as_ptr());
            }
            let queues = (0..params.len().max(1) + 64).map(|_| ComWrapper::new(Queue { id: Cell::new(0), value: Cell::new(0.0) })).collect();
            let mut me = Vst3Plugin {
                handler,
                applied: std::collections::HashMap::new(),
                editor_error: None,
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
                editor: None,
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
        // Parameters: last value per id wins within a block. Edits from the plugin's GUI go to the processor
        // only (the controller made them).
        let gui: Vec<(u32, f64)> = self.handler.audio.try_lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        let mut used = 0usize;
        {
            let q = &self.changes.queues;
            for (id, value) in gui {
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
            }
        }
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
        let mut sc_ptrs = [s.sc_l.as_mut_ptr(), s.sc_r.as_mut_ptr()];
        let mut out_ptrs = [s.out_l.as_mut_ptr(), s.out_r.as_mut_ptr()];
        let mut inb = [
            AudioBusBuffers { numChannels: 2, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: in_ptrs.as_mut_ptr() } },
            AudioBusBuffers { numChannels: 2, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: sc_ptrs.as_mut_ptr() } },
        ];
        let mut outb = AudioBusBuffers { numChannels: 2, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: out_ptrs.as_mut_ptr() } };
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: n as int32,
            numInputs: self.inputs,
            numOutputs: self.outputs,
            inputs: if self.inputs > 0 { inb.as_mut_ptr() } else { std::ptr::null_mut() },
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

    /// Opens the plugin's editor embedded in its own native window, or closes it.
    fn set_editor(&mut self, open: bool) -> bool {
        if !open {
            if let Some(ed) = self.editor.take() {
                // SAFETY: detach in SDK order: removed, then drop the frame.
                unsafe {
                    ed.view.removed();
                    ed.view.setFrame(std::ptr::null_mut());
                }
            }
            return false;
        }
        if self.editor.is_some() {
            return true;
        }
        match self.open_view() {
            Ok(ed) => {
                self.editor = Some(ed);
                self.editor_error = None;
                true
            }
            Err(e) => {
                tracing::warn!("{}: {e}", self.info.name);
                self.editor_error = Some(e);
                false
            }
        }
    }

    /// Creates the plugin's view and embeds it into a native window (SDK order: createView, platform check,
    /// content scale, getSize, setFrame, attached).
    fn open_view(&mut self) -> std::result::Result<Editor, String> {
        let c = self.controller.as_ref().ok_or("the plugin has no edit controller")?;
        let platform: &CStr = if cfg!(target_os = "windows") {
            c"HWND"
        } else if cfg!(target_os = "macos") {
            c"NSView"
        } else {
            c"X11EmbedWindowID"
        };
        // SAFETY: IPlugView lifecycle per the SDK, on the UI thread.
        unsafe {
            let view = ComPtr::<IPlugView>::from_raw(c.createView(c"editor".as_ptr())).ok_or("the plugin has no editor view (createView returned nothing)")?;
            if view.isPlatformTypeSupported(platform.as_ptr()) != kResultTrue {
                return Err(format!("the editor does not support the {} window type", platform.to_string_lossy()));
            }
            let size = |v: &ComPtr<IPlugView>| {
                let mut rect: ViewRect = std::mem::zeroed();
                v.getSize(&mut rect);
                ((rect.right - rect.left).max(64) as u32, (rect.bottom - rect.top).max(64) as u32)
            };
            let (w, h) = size(&view);
            let mut window = super::window::EditorWindow::open(&self.info.name, w, h).map_err(|e| format!("cannot open the editor window: {e}"))?;
            // HiDPI: tell the editor the window's scale, then take its (possibly larger) size.
            let scale = window.scale();
            if (scale - 1.0).abs() > 0.01 {
                if let Some(cs) = view.cast::<IPlugViewContentScaleSupport>() {
                    cs.setContentScaleFactor(scale);
                    let (w2, h2) = size(&view);
                    if (w2, h2) != (w, h) {
                        window.resize(w2, h2);
                    }
                }
            }
            let frame = ComWrapper::new(Frame::default());
            view.setFrame(frame.as_com_ref::<IPlugFrame>().unwrap().as_ptr());
            let r = view.attached(window.handle(), platform.as_ptr());
            if r != kResultOk {
                view.setFrame(std::ptr::null_mut());
                return Err(format!("the editor refused to attach to the window (result {r})"));
            }
            // Many editors only know their real size once attached.
            let (w3, h3) = size(&view);
            if (w3, h3) != (w, h) {
                window.resize(w3, h3);
            }
            // Views that can resize get a resizable window.
            let resizable = view.canResize() == kResultTrue;
            let mut window = window;
            window.set_resizable(resizable, window.size());
            let last_size = window.size();
            Ok(Editor { view, window, frame, resizable, last_size })
        }
    }

    /// Parameter edits made in the plugin's own GUI since the last call (normalised values).
    pub fn gui_edits(&mut self) -> Vec<(u32, f64)> {
        let edits: Vec<(u32, f64)> = self.handler.ui.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for (id, v) in &edits {
            // The project will store this value; it is already applied, so do not send it back.
            self.applied.insert(*id, *v);
        }
        edits
    }

    /// Pumps the editor window and the plugin's run-loop handlers. Returns `false` once the editor closed.
    fn idle_editor(&mut self) -> bool {
        let Some(ed) = &mut self.editor else { return false };
        if !ed.window.pump() {
            self.set_editor(false);
            return false;
        }
        // Resize requests from the plugin.
        if let Some(mut r) = ed.frame.pending_resize.take() {
            ed.window.resize((r.right - r.left) as u32, (r.bottom - r.top) as u32);
            // SAFETY: tell the view its new size (SDK: host resizes, then calls onSize).
            unsafe { ed.view.onSize(&mut r) };
            ed.last_size = ((r.right - r.left) as u32, (r.bottom - r.top) as u32);
        }
        // The user (or full screen) resized the window: constrain it to what the view allows, then tell it.
        let now = ed.window.size();
        if now != ed.last_size && now.0 > 0 && now.1 > 0 {
            ed.last_size = now;
            if ed.resizable {
                let mut r = ViewRect { left: 0, top: 0, right: now.0 as i32, bottom: now.1 as i32 };
                // SAFETY: IPlugView size negotiation on the UI thread.
                unsafe {
                    ed.view.checkSizeConstraint(&mut r);
                    ed.view.onSize(&mut r);
                }
            }
        }
        ed.frame.dispatch();
        true
    }

    /// Full screen for the editor's window (the view follows when it can resize).
    pub fn set_editor_fullscreen(&mut self, on: bool) -> bool {
        match &mut self.editor {
            Some(ed) => {
                ed.window.set_fullscreen(on);
                on
            }
            None => false,
        }
    }

    /// Native window of the open editor (tests use it to inspect the embedded child window).
    pub fn editor_window(&self) -> Option<&super::window::EditorWindow> {
        self.editor.as_ref().map(|e| &e.window)
    }
}

/// An open, embedded plugin editor.
struct Editor {
    view: ComPtr<IPlugView>,
    window: super::window::EditorWindow,
    frame: ComWrapper<Frame>,
    resizable: bool,
    last_size: (u32, u32),
}

/// Host side of the editor: `IPlugFrame` (resize requests) and, for Linux, `IRunLoop` (the plugin's file
/// descriptors and timers are serviced from the UI thread).
#[derive(Default)]
struct Frame {
    pending_resize: Cell<Option<ViewRect>>,
    handlers: RefCell<Vec<(ComPtr<Linux::IEventHandler>, i32)>>,
    timers: RefCell<Vec<(ComPtr<Linux::ITimerHandler>, std::time::Duration, std::time::Instant)>>,
}

impl Frame {
    fn dispatch(&self) {
        let handlers: Vec<(ComPtr<Linux::IEventHandler>, i32)> = self.handlers.borrow().clone();
        for (h, fd) in handlers {
            // SAFETY: handler registered by the plugin; called on the UI thread.
            unsafe { h.onFDIsSet(fd) };
        }
        let now = std::time::Instant::now();
        let due: Vec<ComPtr<Linux::ITimerHandler>> = self
            .timers
            .borrow_mut()
            .iter_mut()
            .filter(|t| now.duration_since(t.2) >= t.1)
            .map(|t| {
                t.2 = now;
                t.0.clone()
            })
            .collect();
        for t in due {
            // SAFETY: as above.
            unsafe { t.onTimer() };
        }
    }
}

impl Class for Frame {
    type Interfaces = (IPlugFrame, Linux::IRunLoop);
}

impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, _view: *mut IPlugView, new_size: *mut ViewRect) -> tresult {
        if new_size.is_null() {
            return kInvalidArgument;
        }
        self.pending_resize.set(Some(*new_size));
        kResultOk
    }
}

impl Linux::IRunLoopTrait for Frame {
    unsafe fn registerEventHandler(&self, handler: *mut Linux::IEventHandler, fd: Linux::FileDescriptor) -> tresult {
        match ComRef::from_raw(handler) {
            Some(h) => {
                self.handlers.borrow_mut().push((h.to_com_ptr(), fd));
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn unregisterEventHandler(&self, handler: *mut Linux::IEventHandler) -> tresult {
        self.handlers.borrow_mut().retain(|(h, _)| h.as_ptr() != handler);
        kResultOk
    }
    unsafe fn registerTimer(&self, handler: *mut Linux::ITimerHandler, ms: Linux::TimerInterval) -> tresult {
        match ComRef::from_raw(handler) {
            Some(h) => {
                self.timers.borrow_mut().push((h.to_com_ptr(), std::time::Duration::from_millis(ms.max(1)), std::time::Instant::now()));
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn unregisterTimer(&self, handler: *mut Linux::ITimerHandler) -> tresult {
        self.timers.borrow_mut().retain(|(h, _, _)| h.as_ptr() != handler);
        kResultOk
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        // SAFETY: SDK teardown order.
        unsafe {
            if let Some(ed) = self.editor.take() {
                ed.view.removed();
                ed.view.setFrame(std::ptr::null_mut());
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
            let v = value as f64;
            if self.applied.get(&pid).is_some_and(|old| (old - v).abs() < 1e-9) {
                return;
            }
            self.applied.insert(pid, v);
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
        self.scratch.set_sidechain(n, None);
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

#[allow(dead_code)]
fn _assert_comref(_: ComRef<'_, IBStream>) {}

#[cfg(test)]
mod host_tests {
    use super::*;

    /// Plugins with a separate controller create messages through the host; they must round-trip.
    #[test]
    fn host_creates_messages_and_attribute_lists() {
        let host = ComWrapper::new(HostApp);
        let h = host.as_com_ref::<IHostApplication>().unwrap();
        // SAFETY: COM calls on our own objects.
        unsafe {
            let mut iid: TUID = std::mem::transmute(IMessage::IID);
            let mut obj: *mut c_void = std::ptr::null_mut();
            assert_eq!(h.createInstance(&mut iid, &mut iid, &mut obj), kResultOk);
            let m = ComPtr::<IMessage>::from_raw(obj as *mut IMessage).unwrap();
            m.setMessageID(c"Spectrum".as_ptr());
            assert_eq!(CStr::from_ptr(m.getMessageID()), c"Spectrum");
            let a = ComRef::<IAttributeList>::from_raw(m.getAttributes()).unwrap();
            assert_eq!(a.setInt(c"n".as_ptr(), 42), kResultOk);
            let mut n = 0;
            assert_eq!(a.getInt(c"n".as_ptr(), &mut n), kResultOk);
            assert_eq!(n, 42);
            let data = [1u8, 2, 3];
            a.setBinary(c"bin".as_ptr(), data.as_ptr() as *const c_void, 3);
            let (mut p, mut len) = (std::ptr::null(), 0u32);
            assert_eq!(a.getBinary(c"bin".as_ptr(), &mut p, &mut len), kResultOk);
            assert_eq!(std::slice::from_raw_parts(p as *const u8, len as usize), &data);
            let text: Vec<TChar> = "hi".encode_utf16().chain(std::iter::once(0)).map(|c| c as TChar).collect();
            a.setString(c"s".as_ptr(), text.as_ptr());
            let mut out = [0 as TChar; 8];
            assert_eq!(a.getString(c"s".as_ptr(), out.as_mut_ptr(), 16), kResultOk);
            assert_eq!(&out[..3], &text[..3]);
            assert_eq!(a.getInt(c"missing".as_ptr(), &mut n), kResultFalse);
            // Unknown classes are refused.
            let mut other: TUID = [7; 16];
            assert_eq!(h.createInstance(&mut other, &mut other, &mut obj), kNoInterface);
        }
    }
}
