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
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for NullEngine {
    fn drop(&mut self) {
        // Disconnect, then wait for the clock thread (it owns the renderer and the plugins).
        self.tx = channel().0;
        dragon_engine::device::join_bounded(self.thread.take().into_iter().collect(), Duration::from_secs(5));
    }
}

impl NullEngine {
    pub fn start(song: Arc<Song>) -> Self {
        let (tx, rx) = channel::<Command>();
        let mut r = Renderer::new(song);
        let meters = r.meters.clone();
        let transport = Arc::new(Mutex::new(SharedTransport { state: TransportState::default(), at: Instant::now() }));
        let t2 = transport.clone();
        let thread = std::thread::Builder::new()
            .name("dragon-null-audio".into())
            .spawn(move || {
                const BLOCK: usize = 512;
                let (mut l, mut rr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
                let mut next = Instant::now();
                loop {
                    loop {
                        match rx.try_recv() {
                            Ok(c) => match c {
                                Command::SetSong(s) => {
                                    let _ = r.set_song(s);
                                }
                                Command::Play(m) => r.play(m),
                                Command::Pause => r.pause(),
                                Command::Stop => r.stop(),
                                Command::Seek(b) => r.seek(b),
                                Command::Record { count_in_bars } => r.record(count_in_bars),
                                Command::RecordNotes { count_in_bars, mode } => r.record_in(count_in_bars, mode),
                                Command::Live(e) => r.push_live(e),
                            },
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                        }
                    }
                    // Same guard as the device callback: a failing effect is reported, the app recovers.
                    dragon_engine::fault::set_in_audio(true);
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| r.render(&mut l, &mut rr, &[])));
                    dragon_engine::fault::set_in_audio(false);
                    if let Err(p) = res {
                        dragon_engine::fault::report(dragon_engine::fault::current(), dragon_engine::fault::message(&*p));
                    }
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
        NullEngine { tx, meters, transport, thread: Some(thread) }
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

    /// Stops the audio device and the engine threads (on close, before the windows go away).
    pub fn shutdown(&mut self) {
        match self {
            Audio::Device(e) => e.shutdown(),
            Audio::Null(n, _) => {
                n.tx = channel().0;
                dragon_engine::device::join_bounded(n.thread.take().into_iter().collect(), Duration::from_secs(5));
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
            Audio::Null(n, _) => *n.transport.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
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

    /// The output stream's liveness (a real device only).
    pub fn health(&self) -> Option<&dragon_engine::device::StreamHealth> {
        match self {
            Audio::Device(e) => Some(&e.health),
            Audio::Null(..) => None,
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
            Audio::Device(e) => {
                let rt = if e.input_name.is_some() { format!(", round trip {:.1} ms (recordings are shifted back by this)", e.record_latency() * 1000.0) } else { String::new() };
                format!("{} · {} Hz · {} frames · {:.1} ms{rt}", e.output_name, e.rate, e.buffer_frames(), e.buffer_ms())
            }
            Audio::Null(_, why) => format!("No audio device ({why}) – silent clock"),
        }
    }

    /// The open input device's name and its number of channels (none open: `None`).
    pub fn input(&self) -> Option<(String, u16)> {
        match self {
            Audio::Device(e) => e.input_name.clone().map(|n| (n, e.input_channels)),
            Audio::Null(..) => None,
        }
    }

    /// Why no input is open (`None` when one is, or when there is no audio device at all).
    pub fn input_error(&self) -> Option<String> {
        match self {
            Audio::Device(e) => e.input_error.clone(),
            Audio::Null(_, why) => Some(format!("no audio device ({why})")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use dragon_engine::model::Project;
    use dragon_engine::render::{CompileContext, Pool, StretchCache};
    use dragon_engine::samples::SampleCache;

    /// Shutting the engine down stops its thread and frees the song (and its plugins) right away, on the
    /// calling thread, instead of leaving it to process exit.
    #[test]
    fn shutdown_frees_the_song_and_stops_the_thread() {
        let p = Project::new_default();
        let (cache, stretch, mut pool) = (SampleCache::default(), StretchCache::default(), Pool::new());
        let song = Arc::new(Song::compile(&p, &mut CompileContext { cache: &cache, stretch: &stretch, pool: &mut pool, rate: 48000.0 }));
        let mut a = Audio::Null(NullEngine::start(song.clone()), "test".into());
        a.send(Command::Play(dragon_engine::render::PlayMode::Song));
        std::thread::sleep(Duration::from_millis(30));
        assert!(Arc::strong_count(&song) > 1);
        a.shutdown();
        assert_eq!(Arc::strong_count(&song), 1, "the engine still holds the song");
        a.send(Command::Stop); // ignored, no panic
        a.shutdown(); // idempotent
    }
}
