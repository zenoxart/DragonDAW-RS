use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void};
use std::rc::Rc;
use vst3::ComPtr;
use vst3::Steinberg::Linux::{IRunLoop, IRunLoopTrait, ITimerHandler, ITimerHandlerTrait};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{uid, Class, ComRef, ComWrapper};

pub const CID: TUID = uid(0x44524147, 0x4F4E5445, 0x53544741, 0x494E0001);

fn cstring(src: &str, dst: &mut [c_char]) {
    for (d, s) in dst.iter_mut().zip(src.bytes().chain(std::iter::once(0))) {
        *d = s as c_char;
    }
}

fn wstring(src: &str, dst: &mut [TChar]) {
    for (d, s) in dst.iter_mut().zip(src.encode_utf16().chain(std::iter::once(0))) {
        *d = s as TChar;
    }
}

/// Single-component gain: processor and controller in one object. Normalized 0..1 maps to gain 0..2.
struct Gain {
    normalized: Rc<Cell<f64>>,
    handler: Rc<RefCell<Option<ComPtr<IComponentHandler>>>>,
}

impl Class for Gain {
    type Interfaces = (IComponent, IAudioProcessor, IEditController);
}

impl IPluginBaseTrait for Gain {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Gain {
    unsafe fn getControllerClassId(&self, _class_id: *mut TUID) -> tresult {
        kNotImplemented
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> int32 {
        match (media == MediaTypes_::kAudio as MediaType, dir == BusDirections_::kInput as BusDirection) {
            // Main input + sidechain (aux) input; one output.
            (true, true) => 2,
            (true, false) => 1,
            _ => 0,
        }
    }
    unsafe fn getBusInfo(&self, media: MediaType, dir: BusDirection, index: int32, bus: *mut BusInfo) -> tresult {
        let input = dir == BusDirections_::kInput as BusDirection;
        if media != MediaTypes_::kAudio as MediaType || index > 1 || (index == 1 && !input) {
            return kInvalidArgument;
        }
        let bus = &mut *bus;
        bus.mediaType = media;
        bus.direction = dir;
        bus.channelCount = 2;
        wstring(if index == 1 { "Sidechain" } else { "Main" }, &mut bus.name);
        bus.busType = if index == 1 { BusTypes_::kAux as BusType } else { BusTypes_::kMain as BusType };
        bus.flags = if index == 1 { 0 } else { BusInfo_::BusFlags_::kDefaultActive as uint32 };
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _i: *mut RoutingInfo, _o: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(&self, _m: MediaType, _d: BusDirection, _i: int32, _s: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        let Some(s) = ComRef::from_raw(state) else { return kInvalidArgument };
        let mut b = [0u8; 8];
        let mut read = 0;
        if s.read(b.as_mut_ptr() as *mut c_void, 8, &mut read) != kResultOk || read != 8 {
            return kResultFalse;
        }
        self.normalized.set(f64::from_le_bytes(b));
        kResultOk
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        let Some(s) = ComRef::from_raw(state) else { return kInvalidArgument };
        let mut b = self.normalized.get().to_le_bytes();
        let mut written = 0;
        s.write(b.as_mut_ptr() as *mut c_void, 8, &mut written)
    }
}

impl IAudioProcessorTrait for Gain {
    unsafe fn setBusArrangements(&self, _i: *mut SpeakerArrangement, n_in: int32, _o: *mut SpeakerArrangement, n_out: int32) -> tresult {
        if (1..=2).contains(&n_in) && n_out == 1 {
            kResultTrue
        } else {
            kResultFalse
        }
    }
    unsafe fn getBusArrangement(&self, _d: BusDirection, index: int32, arr: *mut SpeakerArrangement) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        *arr = 3;
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: int32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 as int32 {
            kResultOk
        } else {
            kNotImplemented
        }
    }
    unsafe fn getLatencySamples(&self) -> uint32 {
        0
    }
    unsafe fn setupProcessing(&self, _setup: *mut ProcessSetup) -> tresult {
        kResultOk
    }
    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let d = &*data;
        if let Some(changes) = ComRef::from_raw(d.inputParameterChanges) {
            for i in 0..changes.getParameterCount() {
                if let Some(q) = ComRef::from_raw(changes.getParameterData(i)) {
                    let (mut off, mut v) = (0, 0.0);
                    if q.getParameterId() == 0 && q.getPoint(q.getPointCount() - 1, &mut off, &mut v) == kResultTrue {
                        self.normalized.set(v);
                    }
                }
            }
        }
        if d.numInputs < 1 || d.numOutputs != 1 {
            return kResultOk;
        }
        let n = d.numSamples as usize;
        let g = (self.normalized.get() * 2.0) as f32;
        let inb = &*d.inputs;
        let outb = &*d.outputs;
        // Ducking by the aux input: out = in · gain · (1 − |sidechain|).
        let sc = (d.numInputs >= 2).then(|| std::slice::from_raw_parts(*(*d.inputs.add(1)).__field0.channelBuffers32, n));
        for ch in 0..2 {
            let i = std::slice::from_raw_parts(*inb.__field0.channelBuffers32.add(ch), n);
            let o = std::slice::from_raw_parts_mut(*outb.__field0.channelBuffers32.add(ch), n);
            for k in 0..n {
                let duck = 1.0 - sc.map(|x| x[k].abs().min(1.0)).unwrap_or(0.0);
                o[k] = i[k] * g * duck;
            }
        }
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> uint32 {
        0
    }
}

impl IEditControllerTrait for Gain {
    unsafe fn setComponentState(&self, _s: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, _s: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _s: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getParameterCount(&self) -> int32 {
        1
    }
    unsafe fn getParameterInfo(&self, index: int32, info: *mut ParameterInfo) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        let info = &mut *info;
        info.id = 0;
        wstring("Gain", &mut info.title);
        wstring("Gain", &mut info.shortTitle);
        wstring("", &mut info.units);
        info.stepCount = 0;
        info.defaultNormalizedValue = 0.5;
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, _id: ParamID, _v: ParamValue, _s: *mut String128) -> tresult {
        kNotImplemented
    }
    unsafe fn getParamValueByString(&self, _id: ParamID, _s: *mut TChar, _v: *mut ParamValue) -> tresult {
        kNotImplemented
    }
    unsafe fn normalizedParamToPlain(&self, _id: ParamID, v: ParamValue) -> ParamValue {
        v * 2.0
    }
    unsafe fn plainParamToNormalized(&self, _id: ParamID, v: ParamValue) -> ParamValue {
        v / 2.0
    }
    unsafe fn getParamNormalized(&self, id: ParamID) -> ParamValue {
        if id == 0 {
            self.normalized.get()
        } else {
            0.0
        }
    }
    unsafe fn setParamNormalized(&self, id: ParamID, v: ParamValue) -> tresult {
        if id == 0 {
            self.normalized.set(v);
        }
        kResultOk
    }
    unsafe fn setComponentHandler(&self, h: *mut IComponentHandler) -> tresult {
        *self.handler.borrow_mut() = ComRef::from_raw(h).map(|h| h.to_com_ptr());
        kResultOk
    }
    unsafe fn createView(&self, _name: FIDString) -> *mut IPlugView {
        let view = ComWrapper::new(View {
            param: self.normalized.clone(),
            handler: self.handler.clone(),
            child: RefCell::new(None),
            frame: RefCell::new(None),
            sized: Cell::new(false),
            me_timer: Cell::new(std::ptr::null_mut()),
            me_view: Cell::new(std::ptr::null_mut()),
        });
        view.me_timer.set(view.as_com_ref::<ITimerHandler>().unwrap().as_ptr());
        view.me_view.set(view.as_com_ref::<IPlugView>().unwrap().as_ptr());
        view.to_com_ptr::<IPlugView>().unwrap().into_raw()
    }
}

/// Test editor: embeds a child window (X11 or Win32), asks the host to resize it, and – like a real GUI –
/// reports a control change through the host's component handler. On Linux the change happens in a host
/// run-loop timer, once the resize arrived (0.25 before, 0.75 after); on Windows (no run loop) when the host
/// applies the size. The host test observes the whole round trip.
struct View {
    param: Rc<Cell<f64>>,
    handler: Rc<RefCell<Option<ComPtr<IComponentHandler>>>>,
    child: RefCell<Option<crate::x11child::Child>>,
    frame: RefCell<Option<ComPtr<IPlugFrame>>>,
    sized: Cell<bool>,
    me_timer: Cell<*mut ITimerHandler>,
    me_view: Cell<*mut IPlugView>,
}

impl Class for View {
    type Interfaces = (IPlugView, ITimerHandler);
}

impl View {
    /// What a GUI does when a control moves: change the value and tell the host.
    unsafe fn edit(&self, v: f64) {
        self.param.set(v);
        if let Some(h) = self.handler.borrow().as_ref() {
            h.beginEdit(0);
            h.performEdit(0, v);
            h.endEdit(0);
        }
    }
}

impl ITimerHandlerTrait for View {
    unsafe fn onTimer(&self) {
        self.edit(if self.sized.get() { 0.75 } else { 0.25 });
    }
}

impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, t: FIDString) -> tresult {
        let want = if cfg!(windows) { c"HWND" } else { c"X11EmbedWindowID" };
        if std::ffi::CStr::from_ptr(t) == want {
            kResultTrue
        } else {
            kResultFalse
        }
    }
    unsafe fn attached(&self, parent: *mut c_void, _t: FIDString) -> tresult {
        let Some(child) = crate::x11child::Child::new(parent as usize as u64, 320, 200) else { return kResultFalse };
        *self.child.borrow_mut() = Some(child);
        if let Some(f) = self.frame.borrow().as_ref() {
            let mut r = ViewRect { left: 0, top: 0, right: 500, bottom: 400 };
            f.resizeView(self.me_view.get(), &mut r);
        }
        kResultOk
    }
    unsafe fn removed(&self) -> tresult {
        *self.child.borrow_mut() = None;
        kResultOk
    }
    unsafe fn onWheel(&self, _d: f32) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyDown(&self, _k: char16, _c: int16, _m: int16) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyUp(&self, _k: char16, _c: int16, _m: int16) -> tresult {
        kResultFalse
    }
    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        let (w, h) = if self.sized.get() { (500, 400) } else { (320, 200) };
        *size = ViewRect { left: 0, top: 0, right: w, bottom: h };
        kResultOk
    }
    unsafe fn onSize(&self, _s: *mut ViewRect) -> tresult {
        self.sized.set(true);
        if cfg!(not(target_os = "linux")) {
            self.edit(0.75);
        }
        kResultOk
    }
    unsafe fn onFocus(&self, _s: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        // Unregister from the old frame's run loop, register with the new one.
        if let Some(old) = self.frame.borrow_mut().take() {
            if let Some(rl) = old.cast::<IRunLoop>() {
                rl.unregisterTimer(self.me_timer.get());
            }
        }
        if let Some(f) = vst3::ComRef::from_raw(frame) {
            let f = f.to_com_ptr();
            if let Some(rl) = f.cast::<IRunLoop>() {
                rl.registerTimer(self.me_timer.get(), 5);
            }
            *self.frame.borrow_mut() = Some(f);
        }
        kResultOk
    }
    unsafe fn canResize(&self) -> tresult {
        kResultFalse
    }
    unsafe fn checkSizeConstraint(&self, _r: *mut ViewRect) -> tresult {
        kResultOk
    }
}

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory,);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = &mut *info;
        cstring("ZenoxArt", &mut info.vendor);
        cstring("", &mut info.url);
        cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }
    unsafe fn countClasses(&self) -> int32 {
        1
    }
    unsafe fn getClassInfo(&self, index: int32, info: *mut PClassInfo) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        let info = &mut *info;
        info.cid = CID;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        cstring("Audio Module Class", &mut info.category);
        cstring("Dragon Test Gain", &mut info.name);
        kResultOk
    }
    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, obj: *mut *mut c_void) -> tresult {
        if *(cid as *const TUID) != CID {
            return kInvalidArgument;
        }
        let instance = ComWrapper::new(Gain { normalized: Rc::new(Cell::new(0.5)), handler: Rc::new(RefCell::new(None)) }).to_com_ptr::<FUnknown>().unwrap();
        let ptr = instance.as_ptr();
        ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj)
    }
}

#[cfg(target_os = "linux")]
#[no_mangle]
extern "system" fn ModuleEntry(_h: *mut c_void) -> bool {
    true
}
#[cfg(target_os = "linux")]
#[no_mangle]
extern "system" fn ModuleExit() -> bool {
    true
}
#[cfg(target_os = "windows")]
#[no_mangle]
extern "system" fn InitDll() -> bool {
    true
}
#[cfg(target_os = "windows")]
#[no_mangle]
extern "system" fn ExitDll() -> bool {
    true
}
#[cfg(target_os = "macos")]
#[no_mangle]
extern "system" fn BundleEntry(_b: *mut c_void) -> bool {
    true
}
#[cfg(target_os = "macos")]
#[no_mangle]
extern "system" fn BundleExit() -> bool {
    true
}

#[no_mangle]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>().unwrap().into_raw()
}
