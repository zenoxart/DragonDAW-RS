//! Real-time audio and MIDI devices: the output stream that drives the [`Renderer`], the input stream for
//! monitoring and recording, the recorder that writes takes to disk on a worker thread, and MIDI input.
//!
//! Threads:
//! * **audio thread** (device callback) – applies commands, renders, monitors input, hands input blocks to
//!   the recorder; never touches the disk.
//! * **recorder thread** – writes armed inputs to WAV files.
//! * **MIDI thread** (midir callback) – forwards messages to the UI with the exact transport beat.

use crate::model::{Id, InputSource};
use crate::render::{LiveEvent, Meters, PlayMode, Renderer, Song, TransportState};
use crate::{EngineError, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DeviceConfig {
    /// Audio host / driver (`ALSA`, `JACK`, `WASAPI`, `ASIO`, `CoreAudio`); `None` = default.
    pub host: Option<String>,
    pub output: Option<String>,
    pub input: Option<String>,
    pub sample_rate: Option<u32>,
    /// Buffer size in frames; `None` = driver default.
    pub buffer: Option<u32>,
    /// Extra recording offset in milliseconds (positive = recordings are moved earlier).
    pub record_offset_ms: f32,
    pub midi_inputs: Vec<String>,
}

pub fn hosts() -> Vec<String> {
    cpal::available_hosts().into_iter().map(|h| h.name().to_string()).collect()
}

fn host(name: Option<&str>) -> cpal::Host {
    name.and_then(|n| cpal::available_hosts().into_iter().find(|h| h.name() == n)).and_then(|id| cpal::host_from_id(id).ok()).unwrap_or_else(cpal::default_host)
}

pub fn output_devices(host_name: Option<&str>) -> Vec<String> {
    host(host_name).output_devices().map(|d| d.filter_map(|x| x.name().ok()).collect()).unwrap_or_default()
}

pub fn input_devices(host_name: Option<&str>) -> Vec<String> {
    host(host_name).input_devices().map(|d| d.filter_map(|x| x.name().ok()).collect()).unwrap_or_default()
}

/// Commands from the UI to the audio thread.
pub enum Command {
    SetSong(Arc<Song>),
    Play(PlayMode),
    Pause,
    Stop,
    Seek(f64),
    Record { count_in_bars: u32 },
    Live(LiveEvent),
}

/// A finished recording of one track.
#[derive(Debug, Clone)]
pub struct RecordedTake {
    pub track: Id,
    pub path: PathBuf,
    /// Beat where the recording starts on the timeline (after count-in).
    pub start_beat: f64,
    pub frames: usize,
    pub rate: u32,
    /// Seconds to skip at the start of the file to compensate the round-trip latency.
    pub latency: f64,
}

/// Where and what to record.
#[derive(Debug, Clone)]
pub struct RecordTarget {
    pub track: Id,
    pub source: InputSource,
    pub path: PathBuf,
}

enum RecMsg {
    Start { targets: Vec<RecordTarget>, rate: u32 },
    Block { first_beat: f64, channels: Vec<Vec<f32>> },
    Finish { latency: f64, reply: Sender<Vec<RecordedTake>> },
}

fn recorder_thread(rx: Receiver<RecMsg>) {
    struct Open {
        target: RecordTarget,
        writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
        frames: usize,
    }
    let mut open: Vec<Open> = Vec::new();
    let mut start_beat: Option<f64> = None;
    let mut rate = 48000;
    for msg in rx {
        match msg {
            RecMsg::Start { targets, rate: r } => {
                rate = r;
                start_beat = None;
                open.clear();
                for t in targets {
                    if let Some(dir) = t.path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let channels = if t.source.right.is_some() { 2 } else { 1 };
                    let spec = hound::WavSpec { channels, sample_rate: r, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
                    match hound::WavWriter::create(&t.path, spec) {
                        Ok(writer) => open.push(Open { target: t, writer, frames: 0 }),
                        Err(e) => tracing::error!("cannot create {}: {e}", t.path.display()),
                    }
                }
            }
            RecMsg::Block { first_beat, channels } => {
                if start_beat.is_none() {
                    start_beat = Some(first_beat);
                }
                for o in &mut open {
                    let l = channels.get(o.target.source.left as usize);
                    let r = o.target.source.right.and_then(|r| channels.get(r as usize));
                    let Some(l) = l else { continue };
                    for i in 0..l.len() {
                        let _ = o.writer.write_sample(l[i]);
                        if let Some(r) = r {
                            let _ = o.writer.write_sample(r.get(i).copied().unwrap_or(0.0));
                        }
                    }
                    o.frames += l.len();
                }
            }
            RecMsg::Finish { latency, reply } => {
                let mut out = Vec::new();
                for o in open.drain(..) {
                    let frames = o.frames;
                    let _ = o.writer.finalize();
                    if frames > 0 {
                        out.push(RecordedTake { track: o.target.track, path: o.target.path, start_beat: start_beat.unwrap_or(0.0), frames, rate, latency });
                    } else {
                        let _ = std::fs::remove_file(&o.target.path);
                    }
                }
                let _ = reply.send(out);
            }
        }
    }
}

/// Shared view of the transport, updated by the audio thread.
#[derive(Debug, Clone, Copy)]
pub struct SharedTransport {
    pub state: TransportState,
    pub at: Instant,
}

impl SharedTransport {
    /// Beat estimated for now (between audio callbacks).
    pub fn beat_now(&self) -> f64 {
        if self.state.playing {
            self.state.beat + self.at.elapsed().as_secs_f64() * self.state.bpm / 60.0
        } else {
            self.state.beat
        }
    }
}

/// The running audio engine.
pub struct AudioEngine {
    tx: Sender<Command>,
    rec_tx: Sender<RecMsg>,
    recording: Arc<Mutex<bool>>,
    pub meters: Meters,
    pub transport: Arc<Mutex<SharedTransport>>,
    pub rate: u32,
    pub buffer: u32,
    pub output_name: String,
    pub input_name: Option<String>,
    pub input_channels: u16,
    pub latency_frames: u32,
    record_offset: f64,
    _out: cpal::Stream,
    _in: Option<cpal::Stream>,
}

impl AudioEngine {
    pub fn start(cfg: &DeviceConfig, song: Arc<Song>) -> Result<AudioEngine> {
        let h = host(cfg.host.as_deref());
        let out_dev = match &cfg.output {
            Some(n) => h.output_devices().map_err(|e| EngineError::Device(e.to_string()))?.find(|d| d.name().ok().as_deref() == Some(n)).or_else(|| h.default_output_device()),
            None => h.default_output_device(),
        }
        .ok_or_else(|| EngineError::Device("no audio output device".into()))?;
        let def = out_dev.default_output_config().map_err(|e| EngineError::Device(e.to_string()))?;
        let rate = cfg.sample_rate.unwrap_or(def.sample_rate().0);
        let channels = def.channels().max(1);
        let buffer = match cfg.buffer {
            Some(b) => cpal::BufferSize::Fixed(b),
            None => cpal::BufferSize::Default,
        };
        let config = cpal::StreamConfig { channels, sample_rate: cpal::SampleRate(rate), buffer_size: buffer };

        // Input (optional).
        let in_dev = match &cfg.input {
            Some(n) => h.input_devices().ok().and_then(|mut d| d.find(|x| x.name().ok().as_deref() == Some(n))),
            None => h.default_input_device(),
        };
        let input_buf: Arc<Mutex<Vec<VecDeque<f32>>>> = Arc::new(Mutex::new(Vec::new()));
        let mut input_channels = 0;
        let mut in_stream = None;
        let mut input_name = None;
        if let Some(dev) = in_dev {
            if let Ok(icfg) = dev.default_input_config() {
                let ich = icfg.channels().max(1);
                let icfg2 = cpal::StreamConfig { channels: ich, sample_rate: cpal::SampleRate(rate), buffer_size: buffer };
                let ib = input_buf.clone();
                *ib.lock().unwrap() = vec![VecDeque::with_capacity(rate as usize); ich as usize];
                let err = |e| tracing::warn!("input stream: {e}");
                let stream = match icfg.sample_format() {
                    cpal::SampleFormat::F32 => dev.build_input_stream(&icfg2, move |d: &[f32], _: &_| push_input(&ib, d, ich as usize, |x| x), err, None),
                    cpal::SampleFormat::I16 => dev.build_input_stream(&icfg2, move |d: &[i16], _: &_| push_input(&ib, d, ich as usize, |x| x as f32 / 32768.0), err, None),
                    cpal::SampleFormat::I32 => dev.build_input_stream(&icfg2, move |d: &[i32], _: &_| push_input(&ib, d, ich as usize, |x| x as f32 / 2_147_483_648.0), err, None),
                    _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
                };
                match stream {
                    Ok(s) => {
                        let _ = s.play();
                        in_stream = Some(s);
                        input_channels = ich;
                        input_name = dev.name().ok();
                    }
                    Err(e) => tracing::warn!("no input stream: {e}"),
                }
            }
        }

        let (tx, rx) = unbounded::<Command>();
        let (rec_tx, rec_rx) = unbounded::<RecMsg>();
        std::thread::Builder::new().name("dragon-recorder".into()).spawn(move || recorder_thread(rec_rx)).map_err(|e| EngineError::Device(e.to_string()))?;
        let recording = Arc::new(Mutex::new(false));
        let mut renderer = Renderer::new(song);
        let meters = renderer.meters.clone();
        let transport = Arc::new(Mutex::new(SharedTransport { state: renderer.transport, at: Instant::now() }));
        let tr = transport.clone();
        let rec_flag = recording.clone();
        let rtx = rec_tx.clone();
        let ib = input_buf.clone();
        let mut l = vec![0.0f32; 8192];
        let mut r = vec![0.0f32; 8192];
        let mut inputs: Vec<Vec<f32>> = Vec::new();
        let ch = channels as usize;
        let mut process = move |n: usize| -> (*const f32, *const f32) {
            while let Ok(c) = rx.try_recv() {
                match c {
                    Command::SetSong(s) => renderer.set_song(s),
                    Command::Play(m) => renderer.play(m),
                    Command::Pause => renderer.pause(),
                    Command::Stop => renderer.stop(),
                    Command::Seek(b) => renderer.seek(b),
                    Command::Record { count_in_bars } => renderer.record(count_in_bars),
                    Command::Live(e) => renderer.push_live(e),
                }
            }
            if l.len() < n {
                l.resize(n, 0.0);
                r.resize(n, 0.0);
            }
            // Pull exactly n frames of input per channel (zeros when the input is late).
            if let Ok(mut q) = ib.try_lock() {
                inputs.resize(q.len(), Vec::new());
                for (k, chq) in q.iter_mut().enumerate() {
                    inputs[k].clear();
                    for _ in 0..n {
                        inputs[k].push(chq.pop_front().unwrap_or(0.0));
                    }
                    // Keep latency bounded if the input runs ahead.
                    while chq.len() > n * 4 {
                        chq.pop_front();
                    }
                }
            } else {
                for v in &mut inputs {
                    v.clear();
                    v.resize(n, 0.0);
                }
            }
            let refs: Vec<&[f32]> = inputs.iter().map(|v| v.as_slice()).collect();
            renderer.render(&mut l[..n], &mut r[..n], &refs);
            // Recording: hand input to the recorder once the count-in is over.
            let st = renderer.transport;
            let recording_now = st.recording && !renderer.counting_in();
            if recording_now && !inputs.is_empty() {
                let _ = rtx.send(RecMsg::Block { first_beat: renderer.last_block_beat.0, channels: inputs.clone() });
            }
            if let Ok(mut f) = rec_flag.try_lock() {
                *f = st.recording;
            }
            if let Ok(mut t) = tr.try_lock() {
                *t = SharedTransport { state: st, at: Instant::now() };
            }
            (l.as_ptr(), r.as_ptr())
        };
        let err = |e| tracing::error!("output stream: {e}");
        let stream = match def.sample_format() {
            cpal::SampleFormat::F32 => out_dev.build_output_stream(
                &config,
                move |data: &mut [f32], _: &_| {
                    let n = data.len() / ch;
                    let (pl, pr) = process(n);
                    // SAFETY: `process` returns pointers into its own buffers of at least n frames that stay
                    // alive (owned by the closure) until the next call.
                    let (l, r) = unsafe { (std::slice::from_raw_parts(pl, n), std::slice::from_raw_parts(pr, n)) };
                    for i in 0..n {
                        data[i * ch] = l[i];
                        if ch > 1 {
                            data[i * ch + 1] = r[i];
                        }
                        for k in 2..ch {
                            data[i * ch + k] = 0.0;
                        }
                    }
                },
                err,
                None,
            ),
            cpal::SampleFormat::I16 => out_dev.build_output_stream(
                &config,
                move |data: &mut [i16], _: &_| {
                    let n = data.len() / ch;
                    let (pl, pr) = process(n);
                    // SAFETY: see above.
                    let (l, r) = unsafe { (std::slice::from_raw_parts(pl, n), std::slice::from_raw_parts(pr, n)) };
                    for i in 0..n {
                        data[i * ch] = (l[i].clamp(-1.0, 1.0) * 32767.0) as i16;
                        if ch > 1 {
                            data[i * ch + 1] = (r[i].clamp(-1.0, 1.0) * 32767.0) as i16;
                        }
                        for k in 2..ch {
                            data[i * ch + k] = 0;
                        }
                    }
                },
                err,
                None,
            ),
            cpal::SampleFormat::I32 => out_dev.build_output_stream(
                &config,
                move |data: &mut [i32], _: &_| {
                    let n = data.len() / ch;
                    let (pl, pr) = process(n);
                    // SAFETY: see above.
                    let (l, r) = unsafe { (std::slice::from_raw_parts(pl, n), std::slice::from_raw_parts(pr, n)) };
                    for i in 0..n {
                        data[i * ch] = (l[i].clamp(-1.0, 1.0) as f64 * 2_147_483_647.0) as i32;
                        if ch > 1 {
                            data[i * ch + 1] = (r[i].clamp(-1.0, 1.0) as f64 * 2_147_483_647.0) as i32;
                        }
                        for k in 2..ch {
                            data[i * ch + k] = 0;
                        }
                    }
                },
                err,
                None,
            ),
            f => return Err(EngineError::Device(format!("unsupported sample format {f:?}"))),
        }
        .map_err(|e| EngineError::Device(e.to_string()))?;
        stream.play().map_err(|e| EngineError::Device(e.to_string()))?;
        let buffer_frames = cfg.buffer.unwrap_or(match def.buffer_size() {
            cpal::SupportedBufferSize::Range { min, max } => (*min).max(256).min(*max),
            cpal::SupportedBufferSize::Unknown => 512,
        });
        // Round trip ≈ one output buffer + one input buffer.
        let latency_frames = buffer_frames * if in_stream.is_some() { 2 } else { 1 };
        Ok(AudioEngine {
            tx,
            rec_tx,
            recording,
            meters,
            transport,
            rate,
            buffer: buffer_frames,
            output_name: out_dev.name().unwrap_or_default(),
            input_name,
            input_channels,
            latency_frames,
            record_offset: cfg.record_offset_ms as f64 / 1000.0,
            _out: stream,
            _in: in_stream,
        })
    }

    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }

    pub fn set_song(&self, s: Arc<Song>) {
        self.send(Command::SetSong(s));
    }

    pub fn transport(&self) -> SharedTransport {
        *self.transport.lock().unwrap()
    }

    pub fn is_recording(&self) -> bool {
        *self.recording.lock().unwrap()
    }

    /// Starts recording the given targets (armed tracks).
    pub fn start_recording(&self, targets: Vec<RecordTarget>, count_in_bars: u32) {
        let _ = self.rec_tx.send(RecMsg::Start { targets, rate: self.rate });
        self.send(Command::Record { count_in_bars });
    }

    /// Stops the transport and returns the recorded takes (blocks until the files are closed).
    pub fn stop_recording(&self) -> Vec<RecordedTake> {
        self.send(Command::Pause);
        // Give the audio thread one buffer to apply the pause before closing the files.
        std::thread::sleep(std::time::Duration::from_millis((self.buffer as u64 * 2000 / self.rate as u64).clamp(5, 200)));
        let (tx, rx) = crossbeam_channel::bounded(1);
        let latency = self.latency_frames as f64 / self.rate as f64 + self.record_offset;
        let _ = self.rec_tx.send(RecMsg::Finish { latency, reply: tx });
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap_or_default()
    }
}

fn push_input<T: Copy>(buf: &Arc<Mutex<Vec<VecDeque<f32>>>>, data: &[T], ch: usize, conv: impl Fn(T) -> f32) {
    if let Ok(mut q) = buf.lock() {
        if q.len() != ch {
            q.resize(ch, VecDeque::new());
        }
        for frame in data.chunks_exact(ch) {
            for (k, s) in frame.iter().enumerate() {
                q[k].push_back(conv(*s));
            }
        }
        // Never grow without bound when nobody consumes (output stopped).
        for c in q.iter_mut() {
            let len = c.len();
            if len > 96_000 {
                c.drain(..len - 48_000);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------- MIDI

/// A MIDI message with the transport beat it arrived at.
#[derive(Debug, Clone, Copy)]
pub struct TimedMidi {
    pub message: crate::midi::MidiMessage,
    pub beat: f64,
    pub seconds: f64,
    pub port: usize,
}

pub fn midi_inputs() -> Vec<String> {
    let Ok(m) = midir::MidiInput::new("Dragon DAW") else { return Vec::new() };
    m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect()
}

/// Open MIDI input connections; messages arrive on `rx`.
pub struct MidiInputs {
    _conns: Vec<midir::MidiInputConnection<()>>,
    pub rx: Receiver<TimedMidi>,
    pub names: Vec<String>,
}

impl MidiInputs {
    /// Opens the named ports (all ports when `names` is empty).
    pub fn open(names: &[String], transport: Option<Arc<Mutex<SharedTransport>>>) -> MidiInputs {
        let (tx, rx) = unbounded();
        let mut conns = Vec::new();
        let mut opened = Vec::new();
        let start = Instant::now();
        let probe = midir::MidiInput::new("Dragon DAW probe");
        let ports: Vec<String> = probe.as_ref().map(|m| m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect()).unwrap_or_default();
        for (idx, name) in ports.iter().enumerate() {
            if !names.is_empty() && !names.contains(name) {
                continue;
            }
            let Ok(mut input) = midir::MidiInput::new(&format!("Dragon DAW in {idx}")) else { continue };
            input.ignore(midir::Ignore::None);
            let Some(port) = input.ports().into_iter().find(|p| input.port_name(p).ok().as_deref() == Some(name.as_str())) else { continue };
            let tx = tx.clone();
            let tr = transport.clone();
            let conn = input.connect(
                &port,
                "dragon-daw",
                move |_, bytes, _| {
                    let beat = tr.as_ref().and_then(|t| t.lock().ok().map(|s| s.beat_now())).unwrap_or(0.0);
                    let _ = tx.send(TimedMidi { message: crate::midi::parse(bytes), beat, seconds: start.elapsed().as_secs_f64(), port: idx });
                },
                (),
            );
            if let Ok(c) = conn {
                conns.push(c);
                opened.push(name.clone());
            }
        }
        MidiInputs { _conns: conns, rx, names: opened }
    }
}
