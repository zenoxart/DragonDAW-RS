use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::state::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::*;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;
use std::ffi::{c_char, c_void, CStr};

struct Desc(clap_plugin_descriptor);
// SAFETY: only points at 'static C strings.
unsafe impl Sync for Desc {}
struct Features([*const c_char; 3]);
unsafe impl Sync for Features {}

static GAIN_FEATURES: Features = Features([c"audio-effect".as_ptr(), c"stereo".as_ptr(), std::ptr::null()]);
static SINE_FEATURES: Features = Features([c"instrument".as_ptr(), c"synthesizer".as_ptr(), std::ptr::null()]);

const fn desc(id: &'static CStr, name: &'static CStr, features: &'static Features) -> Desc {
    Desc(clap_plugin_descriptor {
        clap_version: CLAP_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        vendor: c"ZenoxArt".as_ptr(),
        url: c"".as_ptr(),
        manual_url: c"".as_ptr(),
        support_url: c"".as_ptr(),
        version: c"1.0.0".as_ptr(),
        description: c"Dragon DAW host test plugin".as_ptr(),
        features: features.0.as_ptr(),
    })
}

static GAIN: Desc = desc(c"dragon.test.gain", c"Dragon Test Gain", &GAIN_FEATURES);
static SINE: Desc = desc(c"dragon.test.sine", c"Dragon Test Sine", &SINE_FEATURES);

struct State {
    instrument: bool,
    gain: f64,
    rate: f64,
    note: Option<u8>,
    phase: f64,
}

unsafe fn state<'a>(p: *const clap_plugin) -> &'a mut State {
    &mut *((*p).plugin_data as *mut State)
}

unsafe extern "C" fn init(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn destroy(p: *const clap_plugin) {
    drop(Box::from_raw((*p).plugin_data as *mut State));
    drop(Box::from_raw(p as *mut clap_plugin));
}
unsafe extern "C" fn activate(p: *const clap_plugin, rate: f64, _min: u32, _max: u32) -> bool {
    state(p).rate = rate;
    true
}
unsafe extern "C" fn noop(_p: *const clap_plugin) {}
unsafe extern "C" fn start(_p: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn process(p: *const clap_plugin, pr: *const clap_process) -> clap_process_status {
    let s = state(p);
    let pr = &*pr;
    let ev = &*pr.in_events;
    let n_ev = ev.size.map(|f| f(ev)).unwrap_or(0);
    for i in 0..n_ev {
        let h = ev.get.map(|f| f(ev, i)).unwrap_or(std::ptr::null());
        if h.is_null() || (*h).space_id != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        match (*h).type_ {
            CLAP_EVENT_PARAM_VALUE => {
                let e = &*(h as *const clap_event_param_value);
                if e.param_id == 0 {
                    s.gain = e.value.clamp(0.0, 2.0);
                }
            }
            CLAP_EVENT_NOTE_ON => {
                let e = &*(h as *const clap_event_note);
                s.note = Some(e.key as u8);
            }
            CLAP_EVENT_NOTE_OFF => {
                let e = &*(h as *const clap_event_note);
                if s.note == Some(e.key as u8) {
                    s.note = None;
                }
            }
            _ => {}
        }
    }
    let n = pr.frames_count as usize;
    let out = &*pr.audio_outputs;
    let ol = std::slice::from_raw_parts_mut(*out.data32, n);
    let or = std::slice::from_raw_parts_mut(*out.data32.add(1), n);
    if s.instrument {
        for i in 0..n {
            let v = match s.note {
                Some(k) => {
                    let hz = 440.0 * 2f64.powf((k as f64 - 69.0) / 12.0);
                    s.phase = (s.phase + hz / s.rate).fract();
                    (s.phase * std::f64::consts::TAU).sin() * 0.5 * s.gain
                }
                None => 0.0,
            };
            ol[i] = v as f32;
            or[i] = v as f32;
        }
    } else {
        let inp = &*pr.audio_inputs;
        let il = std::slice::from_raw_parts(*inp.data32, n);
        let ir = std::slice::from_raw_parts(*inp.data32.add(1), n);
        for i in 0..n {
            ol[i] = il[i] * s.gain as f32;
            or[i] = ir[i] * s.gain as f32;
        }
    }
    CLAP_PROCESS_CONTINUE
}

// ---- params
unsafe extern "C" fn p_count(_p: *const clap_plugin) -> u32 {
    1
}
unsafe extern "C" fn p_info(_p: *const clap_plugin, index: u32, info: *mut clap_param_info) -> bool {
    if index != 0 {
        return false;
    }
    let info = &mut *info;
    info.id = 0;
    info.flags = CLAP_PARAM_IS_AUTOMATABLE;
    info.cookie = std::ptr::null_mut();
    for (d, s) in info.name.iter_mut().zip(b"Gain\0") {
        *d = *s as c_char;
    }
    info.module[0] = 0;
    info.min_value = 0.0;
    info.max_value = 2.0;
    info.default_value = 1.0;
    true
}
unsafe extern "C" fn p_value(p: *const clap_plugin, id: clap_id, v: *mut f64) -> bool {
    if id != 0 {
        return false;
    }
    *v = state(p).gain;
    true
}
unsafe extern "C" fn p_to_text(_p: *const clap_plugin, _id: clap_id, _v: f64, _out: *mut c_char, _n: u32) -> bool {
    false
}
unsafe extern "C" fn p_from_text(_p: *const clap_plugin, _id: clap_id, _t: *const c_char, _v: *mut f64) -> bool {
    false
}
unsafe extern "C" fn p_flush(p: *const clap_plugin, inp: *const clap_input_events, _out: *const clap_output_events) {
    let ev = &*inp;
    for i in 0..ev.size.map(|f| f(ev)).unwrap_or(0) {
        let h = ev.get.map(|f| f(ev, i)).unwrap_or(std::ptr::null());
        if !h.is_null() && (*h).type_ == CLAP_EVENT_PARAM_VALUE {
            state(p).gain = (*(h as *const clap_event_param_value)).value;
        }
    }
}
struct Params(clap_plugin_params);
unsafe impl Sync for Params {}
static PARAMS: Params = Params(clap_plugin_params { count: Some(p_count), get_info: Some(p_info), get_value: Some(p_value), value_to_text: Some(p_to_text), text_to_value: Some(p_from_text), flush: Some(p_flush) });

// ---- state
unsafe extern "C" fn s_save(p: *const clap_plugin, out: *const clap_ostream) -> bool {
    let b = state(p).gain.to_le_bytes();
    (*out).write.map(|w| w(out, b.as_ptr() as *const c_void, 8)).unwrap_or(-1) == 8
}
unsafe extern "C" fn s_load(p: *const clap_plugin, inp: *const clap_istream) -> bool {
    let mut b = [0u8; 8];
    if (*inp).read.map(|r| r(inp, b.as_mut_ptr() as *mut c_void, 8)).unwrap_or(-1) != 8 {
        return false;
    }
    state(p).gain = f64::from_le_bytes(b);
    true
}
struct StateExt(clap_plugin_state);
unsafe impl Sync for StateExt {}
static STATE: StateExt = StateExt(clap_plugin_state { save: Some(s_save), load: Some(s_load) });

// ---- audio ports
unsafe extern "C" fn ap_count(p: *const clap_plugin, is_input: bool) -> u32 {
    if is_input && state(p).instrument {
        0
    } else {
        1
    }
}
unsafe extern "C" fn ap_get(_p: *const clap_plugin, index: u32, is_input: bool, info: *mut clap_audio_port_info) -> bool {
    if index != 0 {
        return false;
    }
    let info = &mut *info;
    info.id = if is_input { 0 } else { 1 };
    for (d, s) in info.name.iter_mut().zip(b"Main\0") {
        *d = *s as c_char;
    }
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = clap_sys::id::CLAP_INVALID_ID;
    true
}
struct Ports(clap_plugin_audio_ports);
unsafe impl Sync for Ports {}
static PORTS: Ports = Ports(clap_plugin_audio_ports { count: Some(ap_count), get: Some(ap_get) });

unsafe extern "C" fn get_extension(_p: *const clap_plugin, id: *const c_char) -> *const c_void {
    let id = CStr::from_ptr(id);
    if id == CLAP_EXT_PARAMS {
        &PARAMS.0 as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &STATE.0 as *const _ as *const c_void
    } else if id == CLAP_EXT_AUDIO_PORTS {
        &PORTS.0 as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

// ---- factory / entry
unsafe extern "C" fn f_count(_f: *const clap_plugin_factory) -> u32 {
    2
}
unsafe extern "C" fn f_desc(_f: *const clap_plugin_factory, i: u32) -> *const clap_plugin_descriptor {
    match i {
        0 => &GAIN.0,
        1 => &SINE.0,
        _ => std::ptr::null(),
    }
}
unsafe extern "C" fn f_create(_f: *const clap_plugin_factory, _host: *const clap_host, id: *const c_char) -> *const clap_plugin {
    let id = CStr::from_ptr(id);
    let (d, instrument) = if id == c"dragon.test.gain" {
        (&GAIN.0, false)
    } else if id == c"dragon.test.sine" {
        (&SINE.0, true)
    } else {
        return std::ptr::null();
    };
    let data = Box::into_raw(Box::new(State { instrument, gain: 1.0, rate: 48000.0, note: None, phase: 0.0 }));
    Box::into_raw(Box::new(clap_plugin {
        desc: d,
        plugin_data: data as *mut c_void,
        init: Some(init),
        destroy: Some(destroy),
        activate: Some(activate),
        deactivate: Some(noop),
        start_processing: Some(start),
        stop_processing: Some(noop),
        reset: Some(noop),
        process: Some(process),
        get_extension: Some(get_extension),
        on_main_thread: Some(noop),
    }))
}
struct Factory(clap_plugin_factory);
unsafe impl Sync for Factory {}
static FACTORY: Factory = Factory(clap_plugin_factory { get_plugin_count: Some(f_count), get_plugin_descriptor: Some(f_desc), create_plugin: Some(f_create) });

unsafe extern "C" fn e_init(_path: *const c_char) -> bool {
    true
}
unsafe extern "C" fn e_deinit() {}
unsafe extern "C" fn e_factory(id: *const c_char) -> *const c_void {
    if CStr::from_ptr(id) == CLAP_PLUGIN_FACTORY_ID {
        &FACTORY.0 as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

/// `repr(transparent)` so the exported symbol has exactly the `clap_plugin_entry` layout.
#[repr(transparent)]
#[allow(dead_code)]
struct Entry(clap_plugin_entry);
unsafe impl Sync for Entry {}

#[no_mangle]
#[used]
static clap_entry: Entry = Entry(clap_plugin_entry { clap_version: CLAP_VERSION, init: Some(e_init), deinit: Some(e_deinit), get_factory: Some(e_factory) });
