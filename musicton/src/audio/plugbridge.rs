//! Bridge to sandboxed VST3/CLAP plug-ins. Every plug-in instance runs in its own
//! `musicton-plughost` process; a bridge thread talks to it, and the audio thread exchanges
//! fixed-size blocks with the bridge through lock-free channels. If the process dies, the
//! plug-in falls silent (effects pass the dry signal) and Musicton keeps running.

use crate::model::{PlugParam, PluginRef};
use base64::Engine as _;
use crossbeam_channel::{bounded, select, Receiver, Sender};
use musicton_plugproto::{NoteEvent, PluginDesc, Reply, Request};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{BufReader, BufWriter, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

const MAX_FRAMES: usize = 64;
const MAX_EVENTS: usize = 16;
/// Samples of latency the ring buffer keeps to absorb scheduling jitter.
const PRIME: usize = 128;
const RING: usize = 8192;

#[derive(Clone, Copy)]
pub struct Job {
    frames: u32,
    tempo: f32,
    playing: bool,
    n_ev: u8,
    events: [NoteEvent; MAX_EVENTS],
    input: [f32; MAX_FRAMES * 2],
}

pub struct Out {
    frames: u32,
    data: [f32; MAX_FRAMES * 2],
}

enum Ctrl {
    SetParam(u32, f64),
    GetState(Sender<Vec<u8>>),
    ShowGui(bool, Sender<Result<(), String>>),
    Quit,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LoadInfo {
    pub name: String,
    pub vendor: String,
    pub instrument: bool,
    pub gui: bool,
    pub params: Vec<PlugParam>,
    pub values: Vec<(u32, f64)>,
}

pub struct Bridge {
    pub plugin: PluginRef,
    jobs: Sender<Job>,
    outs: Receiver<Out>,
    ctrl: Sender<Ctrl>,
    pub alive: AtomicBool,
    pub loaded: Mutex<Option<LoadInfo>>,
    pub error: Mutex<Option<String>>,
    /// Parameter changes made inside the plug-in (its editor), for the UI to pick up.
    pub param_updates: Mutex<Vec<(u32, f64)>>,
    pub underruns: AtomicU32,
}

static REGISTRY: LazyLock<Mutex<HashMap<u64, Arc<Bridge>>>> = LazyLock::new(Default::default);

/// While an offline render runs, the live engine leaves the plug-ins alone and the render
/// thread exchanges blocks with them synchronously (no latency, deterministic).
static OFFLINE: AtomicBool = AtomicBool::new(false);
thread_local! {
    static OFFLINE_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub struct OfflineGuard;

impl OfflineGuard {
    pub fn new() -> Self {
        OFFLINE.store(true, Ordering::SeqCst);
        OFFLINE_THREAD.with(|c| c.set(true));
        // Let blocks the live engine already sent come back before we start.
        std::thread::sleep(Duration::from_millis(30));
        for b in REGISTRY.lock().values() {
            while b.outs.try_recv().is_ok() {}
        }
        OfflineGuard
    }
}

impl Drop for OfflineGuard {
    fn drop(&mut self) {
        OFFLINE_THREAD.with(|c| c.set(false));
        OFFLINE.store(false, Ordering::SeqCst);
    }
}

pub fn host_exe() -> std::path::PathBuf {
    let name = if cfg!(windows) { "musicton-plughost.exe" } else { "musicton-plughost" };
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name))).filter(|p| p.exists()).unwrap_or_else(|| name.into())
}

fn spawn_host() -> Result<Child, String> {
    Command::new(host_exe()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().map_err(|e| format!("cannot start musicton-plughost: {e}"))
}

impl Bridge {
    /// Starts the host process and loads the plug-in (asynchronously).
    pub fn start(key: u64, plugin: PluginRef, sample_rate: f32) -> Arc<Bridge> {
        let (jtx, jrx) = bounded::<Job>(64);
        let (otx, orx) = bounded::<Out>(64);
        let (ctx, crx) = bounded::<Ctrl>(256);
        let b = Arc::new(Bridge {
            plugin: plugin.clone(),
            jobs: jtx,
            outs: orx,
            ctrl: ctx,
            alive: AtomicBool::new(true),
            loaded: Mutex::new(None),
            error: Mutex::new(None),
            param_updates: Mutex::new(vec![]),
            underruns: AtomicU32::new(0),
        });
        let b2 = b.clone();
        std::thread::Builder::new()
            .name(format!("plugin-{}", plugin.name))
            .spawn(move || {
                if let Err(e) = run(&b2, plugin, sample_rate, jrx, otx, crx) {
                    *b2.error.lock() = Some(e);
                }
                b2.alive.store(false, Ordering::Relaxed);
            })
            .ok();
        if let Some(old) = REGISTRY.lock().insert(key, b.clone()) {
            old.quit();
        }
        b
    }

    /// Ends the host process. (The bridge thread holds a reference itself, so dropping the
    /// registry entry alone would never stop it.)
    pub fn quit(&self) {
        let _ = self.ctrl.try_send(Ctrl::Quit);
    }

    pub fn set_param(&self, id: u32, value: f64) {
        let _ = self.ctrl.try_send(Ctrl::SetParam(id, value));
    }

    /// Plug-in state (waits up to `timeout`).
    pub fn get_state(&self, timeout: Duration) -> Option<Vec<u8>> {
        let (tx, rx) = bounded(1);
        self.ctrl.try_send(Ctrl::GetState(tx)).ok()?;
        rx.recv_timeout(timeout).ok()
    }

    pub fn show_gui(&self, on: bool) -> Result<(), String> {
        let (tx, rx) = bounded(1);
        self.ctrl.try_send(Ctrl::ShowGui(on, tx)).map_err(|_| "plug-in is not running".to_string())?;
        rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "plug-in did not answer".to_string())?
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.ctrl.try_send(Ctrl::Quit);
    }
}

fn run(b: &Bridge, plugin: PluginRef, sr: f32, jobs: Receiver<Job>, outs: Sender<Out>, ctrl: Receiver<Ctrl>) -> Result<(), String> {
    let mut child = spawn_host()?;
    let mut to = BufWriter::new(child.stdin.take().unwrap());
    let mut from = BufReader::new(child.stdout.take().unwrap());
    let send = |to: &mut BufWriter<_>, r: Request| -> Result<(), String> {
        r.write(to).map_err(|e| e.to_string())?;
        to.flush().map_err(|e| e.to_string())
    };
    // Waits for the reply to a request; parameter notifications are collected on the way.
    let recv = |from: &mut BufReader<_>| -> Result<Reply, String> {
        loop {
            match Reply::read(from).map_err(|e| format!("plug-in process ended: {e}"))? {
                Reply::Param { id, value } => b.param_updates.lock().push((id, value)),
                r => return Ok(r),
            }
        }
    };
    send(&mut to, Request::Load { path: plugin.path.clone(), id: plugin.id.clone(), sample_rate: sr as f64, max_block: 1024 })?;
    match recv(&mut from)? {
        Reply::Loaded { name, vendor, instrument, gui, params } => {
            let infos = params.iter().map(|p| PlugParam { id: p.id, name: p.name.clone(), min: p.min, max: p.max, default: p.default, steps: p.steps }).collect();
            let values = params.iter().map(|p| (p.id, p.value)).collect();
            *b.loaded.lock() = Some(LoadInfo { name, vendor, instrument, gui, params: infos, values });
        }
        Reply::Error(e) => return Err(e),
        other => return Err(format!("unexpected reply {other:?}")),
    }
    // Restore the saved state, then explicit parameter values.
    if let Some(s) = plugin.state.as_deref().and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok()) {
        send(&mut to, Request::SetState(s))?;
        let _ = recv(&mut from)?;
    } else {
        for &(id, v) in &plugin.values {
            send(&mut to, Request::SetParam { id, value: v })?;
            let _ = recv(&mut from)?;
        }
    }
    let mut input = Vec::with_capacity(MAX_FRAMES * 2);
    loop {
        select! {
            recv(jobs) -> job => {
                let Ok(job) = job else { break };
                let n = job.frames as usize;
                input.clear();
                input.extend_from_slice(&job.input[..n * 2]);
                let events = job.events[..job.n_ev as usize].to_vec();
                send(&mut to, Request::Process { frames: job.frames, tempo: job.tempo as f64, playing: job.playing, events, input: std::mem::take(&mut input) })?;
                match recv(&mut from)? {
                    Reply::Audio { frames, output } => {
                        let mut data = [0.0f32; MAX_FRAMES * 2];
                        let k = output.len().min(MAX_FRAMES * 2);
                        data[..k].copy_from_slice(&output[..k]);
                        let _ = outs.try_send(Out { frames, data });
                        input = Vec::with_capacity(MAX_FRAMES * 2);
                    }
                    other => return Err(format!("unexpected reply {other:?}")),
                }
            }
            recv(ctrl) -> c => {
                match c {
                    Ok(Ctrl::SetParam(id, value)) => {
                        send(&mut to, Request::SetParam { id, value })?;
                        let _ = recv(&mut from)?;
                    }
                    Ok(Ctrl::GetState(tx)) => {
                        send(&mut to, Request::GetState)?;
                        if let Reply::State(s) = recv(&mut from)? {
                            let _ = tx.send(s);
                        }
                    }
                    Ok(Ctrl::ShowGui(on, tx)) => {
                        send(&mut to, Request::ShowGui(on))?;
                        let _ = tx.send(match recv(&mut from)? {
                            Reply::Ok => Ok(()),
                            Reply::Error(e) => Err(e),
                            _ => Err("unexpected reply".into()),
                        });
                    }
                    Ok(Ctrl::Quit) | Err(_) => break,
                }
            }
            default(Duration::from_millis(100)) => {}
        }
    }
    let _ = send(&mut to, Request::Quit);
    let _ = child.wait();
    Ok(())
}

pub fn get(key: u64) -> Option<Arc<Bridge>> {
    REGISTRY.try_lock()?.get(&key).cloned()
}

pub fn get_blocking(key: u64) -> Option<Arc<Bridge>> {
    REGISTRY.lock().get(&key).cloned()
}

pub fn remove(key: u64) {
    if let Some(b) = REGISTRY.lock().remove(&key) {
        b.quit();
    }
}

pub fn keys() -> Vec<u64> {
    REGISTRY.lock().keys().copied().collect()
}

/// Audio-thread side of one plug-in instance.
pub struct Runner {
    key: u64,
    bridge: Option<Arc<Bridge>>,
    ring: Box<[[f32; 2]; RING]>,
    read: usize,
    write: usize,
    primed: bool,
    events: Vec<NoteEvent>,
    lookup: u32,
}

impl Runner {
    pub fn new(key: u64) -> Self {
        Self { key, bridge: None, ring: Box::new([[0.0; 2]; RING]), read: 0, write: 0, primed: false, events: Vec::with_capacity(64), lookup: 0 }
    }

    pub fn note(&mut self, on: bool, key: u8, velocity: u8) {
        if self.events.len() < MAX_EVENTS {
            self.events.push(NoteEvent { offset: 0, on, key, velocity });
        }
    }

    fn available(&self) -> usize {
        (self.write + RING - self.read) % RING
    }

    /// Sends the block to the plug-in and fills it with (earlier) output. Returns false while
    /// the plug-in is not running.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], tempo: f32, playing: bool) -> bool {
        if self.bridge.as_ref().is_none_or(|b| !Arc::ptr_eq(b, &get(self.key).unwrap_or_else(|| b.clone()))) {
            self.lookup = self.lookup.wrapping_add(1);
            if self.bridge.is_none() || self.lookup % 64 == 0 {
                self.bridge = get(self.key);
                self.primed = false;
                self.read = 0;
                self.write = 0;
            }
        }
        let offline_here = OFFLINE_THREAD.with(|c| c.get());
        if offline_here && self.bridge.is_none() {
            self.bridge = get_blocking(self.key);
        }
        if OFFLINE.load(Ordering::Relaxed) && !offline_here {
            self.events.clear();
            return false;
        }
        let Some(b) = &self.bridge else { return false };
        if offline_here {
            // Wait for a plug-in that is still loading.
            for _ in 0..200 {
                if b.loaded.lock().is_some() || !b.alive.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if !b.alive.load(Ordering::Relaxed) || b.loaded.try_lock().is_none_or(|l| l.is_none()) {
            self.events.clear();
            return false;
        }
        let n = l.len().min(MAX_FRAMES);
        let mut job = Job { frames: n as u32, tempo, playing, n_ev: 0, events: [NoteEvent { offset: 0, on: false, key: 0, velocity: 0 }; MAX_EVENTS], input: [0.0; MAX_FRAMES * 2] };
        job.input[..n].copy_from_slice(&l[..n]);
        job.input[n..n * 2].copy_from_slice(&r[..n]);
        for (k, e) in self.events.drain(..).enumerate().take(MAX_EVENTS) {
            job.events[k] = e;
            job.n_ev = k as u8 + 1;
        }
        if offline_here {
            if b.jobs.send_timeout(job, Duration::from_secs(2)).is_err() {
                return false;
            }
            match b.outs.recv_timeout(Duration::from_secs(2)) {
                Ok(o) => {
                    let f = (o.frames as usize).min(n);
                    l[..f].copy_from_slice(&o.data[..f]);
                    r[..f].copy_from_slice(&o.data[o.frames as usize..o.frames as usize + f]);
                    return true;
                }
                Err(_) => return false,
            }
        }
        let _ = b.jobs.try_send(job);
        while let Ok(o) = b.outs.try_recv() {
            let f = o.frames as usize;
            for i in 0..f.min(MAX_FRAMES) {
                if self.available() + 1 >= RING {
                    break;
                }
                self.ring[self.write] = [o.data[i], o.data[f + i]];
                self.write = (self.write + 1) % RING;
            }
        }
        if !self.primed && self.available() >= PRIME {
            self.primed = true;
        }
        if self.primed && self.available() >= n {
            for i in 0..n {
                let [a, c] = self.ring[self.read];
                l[i] = a;
                r[i] = c;
                self.read = (self.read + 1) % RING;
            }
        } else {
            if self.primed {
                b.underruns.fetch_add(1, Ordering::Relaxed);
                self.primed = false;
            }
            l[..n].fill(0.0);
            r[..n].fill(0.0);
        }
        true
    }
}

// ---------------------------------------------------------------------------------------------
// Scanning.

pub fn plugin_dirs() -> Vec<std::path::PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut v = vec![home.join(".clap"), home.join(".vst3"), "/usr/lib/clap".into(), "/usr/lib/vst3".into(), "/usr/local/lib/clap".into(), "/usr/local/lib/vst3".into()];
    if let Ok(extra) = std::env::var("CLAP_PATH") {
        v.extend(extra.split(':').map(Into::into));
    }
    v
}

fn cache_path() -> std::path::PathBuf {
    dirs::data_dir().unwrap_or_else(|| ".".into()).join("musicton").join("plugins.txt")
}

pub fn cached_plugins() -> Vec<PluginDesc> {
    std::fs::read_to_string(cache_path()).map(|s| s.lines().filter_map(PluginDesc::from_line).collect()).unwrap_or_default()
}

/// Scans one file/bundle in a separate host process (a crashing plug-in only fails its scan).
pub fn scan_one(path: &str) -> Vec<PluginDesc> {
    let Ok(mut child) = Command::new(host_exe()).arg("--scan").arg(path).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else { return vec![] };
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || std::io::read_to_string(stdout).unwrap_or_default());
    for _ in 0..200 {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    reader.join().unwrap_or_default().lines().filter_map(PluginDesc::from_line).collect()
}

/// Scans every plug-in folder and refreshes the cache.
pub fn scan_all(extra: &[String]) -> Vec<PluginDesc> {
    let mut files = vec![];
    for d in plugin_dirs() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "clap" || x == "vst3") {
                    files.push(p.to_string_lossy().to_string());
                }
            }
        }
    }
    files.extend(extra.iter().cloned());
    files.sort();
    files.dedup();
    let mut out = vec![];
    for f in files {
        out.extend(scan_one(&f));
    }
    if let Some(dir) = cache_path().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(cache_path(), out.iter().map(|d| d.to_line()).collect::<Vec<_>>().join("\n"));
    out
}

pub fn desc_to_ref(d: &PluginDesc) -> PluginRef {
    PluginRef { format: d.format.clone(), path: d.path.clone(), id: d.id.clone(), name: d.name.clone(), vendor: d.vendor.clone(), has_gui: false, infos: vec![], values: vec![], state: None }
}
