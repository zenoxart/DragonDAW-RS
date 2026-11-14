//! Audio backend: the real device engine, or a silent clock when no device can be opened (so the app
//! stays usable – editing, transport, meters and export all keep working without a sound card).

use dragon_engine::device::{AudioEngine, Command, DeviceConfig, RecordTarget, RecordedTake, SharedTransport};
use dragon_engine::render::{Meters, Renderer, Song, TransportState};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct NullEngine {
    tx: Sender<Command>,
    pub meters: Meters,
    pub transport: Arc<Mutex<SharedTransport>>,
}

impl NullEngine {
    pub fn start(song: Arc<Song>) -> Self {
        let (tx, rx) = channel::<Command>();
        let mut r = Renderer::new(song);
        let meters = r.meters.clone();
        let transport = Arc::new(Mutex::new(SharedTransport { state: TransportState::default(), at: Instant::now() }));
        let t2 = transport.clone();
        std::thread::Builder::new()
            .name("dragon-null-audio".into())
            .spawn(move || {
                const BLOCK: usize = 512;
                let (mut l, mut rr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
                let mut next = Instant::now();
                loop {
                    loop {
                        match rx.try_recv() {
                            Ok(c) => match c {
                                Command::SetSong(s) => r.set_song(s),
                                Command::Play(m) => r.play(m),
                                Command::Pause => r.pause(),
                                Command::Stop => r.stop(),
                                Command::Seek(b) => r.seek(b),
                                Command::Record { count_in_bars } => r.record(count_in_bars),
                                Command::Live(e) => r.push_live(e),
                            },
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                        }
                    }
                    r.render(&mut l, &mut rr, &[]);
                    if let Ok(mut t) = t2.lock() {
                        *t = SharedTransport { state: r.transport, at: Instant::now() };
                    }
                    next += Duration::from_secs_f64(BLOCK as f64 / r.rate as f64);
                    let now = Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now;
                    }
                }
            })
            .expect("spawn audio thread");
        NullEngine { tx, meters, transport }
    }
}

pub enum Audio {
    Device(AudioEngine),
    Null(NullEngine, String),
}

impl Audio {
    pub fn start(cfg: &DeviceConfig, song: Arc<Song>) -> Audio {
        match AudioEngine::start(cfg, song.clone()) {
            Ok(e) => Audio::Device(e),
            Err(e) => {
                tracing::warn!("audio device unavailable: {e}");
                Audio::Null(NullEngine::start(song), e.to_string())
            }
        }
    }

    pub fn send(&self, c: Command) {
        match self {
            Audio::Device(e) => e.send(c),
            Audio::Null(n, _) => {
                let _ = n.tx.send(c);
            }
        }
    }

    pub fn transport(&self) -> SharedTransport {
        match self {
            Audio::Device(e) => e.transport(),
            Audio::Null(n, _) => *n.transport.lock().unwrap(),
        }
    }

    pub fn shared_transport(&self) -> Arc<Mutex<SharedTransport>> {
        match self {
            Audio::Device(e) => e.transport.clone(),
            Audio::Null(n, _) => n.transport.clone(),
        }
    }

    pub fn meters(&self) -> &Meters {
        match self {
            Audio::Device(e) => &e.meters,
            Audio::Null(n, _) => &n.meters,
        }
    }

    pub fn rate(&self) -> u32 {
        match self {
            Audio::Device(e) => e.rate,
            Audio::Null(..) => 48000,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Audio::Device(e) => format!("{} · {} Hz · {} frames ({:.1} ms)", e.output_name, e.rate, e.buffer, e.latency_frames as f64 * 1000.0 / e.rate as f64),
            Audio::Null(_, why) => format!("No audio device ({why}) – silent clock"),
        }
    }

    pub fn can_record(&self) -> bool {
        matches!(self, Audio::Device(e) if e.input_name.is_some())
    }

    pub fn is_recording(&self) -> bool {
        matches!(self, Audio::Device(e) if e.is_recording())
    }

    pub fn start_recording(&self, targets: Vec<RecordTarget>, count_in: u32) {
        if let Audio::Device(e) = self {
            e.start_recording(targets, count_in);
        }
    }

    pub fn stop_recording(&self) -> Vec<RecordedTake> {
        match self {
            Audio::Device(e) => e.stop_recording(),
            Audio::Null(..) => Vec::new(),
        }
    }
}
