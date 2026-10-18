//! musicton-plughost: runs exactly one VST3 or CLAP plug-in in its own process, so a crashing
//! plug-in can never take Musicton down. Musicton talks to it over stdin/stdout using the
//! `musicton-plugproto` messages; `--scan <path>` lists the plug-ins in a file or bundle.

mod clap;
mod vst3host;
#[cfg(target_os = "linux")]
mod x11win;

use musicton_plugproto::{NoteEvent, ParamInfo, Reply, Request};
use std::io::Write;
use std::time::{Duration, Instant};

pub struct Loaded {
    pub name: String,
    pub vendor: String,
    pub instrument: bool,
    pub gui: bool,
    pub params: Vec<ParamInfo>,
}

pub trait Backend {
    fn process(&mut self, frames: usize, tempo: f64, playing: bool, events: &[NoteEvent], input: &[f32], output: &mut Vec<f32>);
    fn set_param(&mut self, id: u32, value: f64);
    fn take_param_changes(&mut self) -> Vec<(u32, f64)>;
    fn get_state(&mut self) -> Vec<u8>;
    fn set_state(&mut self, data: &[u8]) -> bool;
    fn show_gui(&mut self, on: bool, sched: &mut Scheduler) -> bool;
    fn on_timer(&mut self, _id: u32) {}
    fn on_fd(&mut self, _fd: i32, _flags: u32) {}
    fn idle(&mut self, _sched: &mut Scheduler) {}
}

/// Timers, file descriptors and the editor window the plug-in's GUI lives in.
pub struct Scheduler {
    timers: Vec<(u32, Duration, Instant)>,
    next_timer: u32,
    pub fds: Vec<(i32, u32)>,
    pub gui_closed: bool,
    #[cfg(target_os = "linux")]
    window: Option<x11win::Window>,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            timers: vec![],
            next_timer: 1,
            fds: vec![],
            gui_closed: false,
            #[cfg(target_os = "linux")]
            window: None,
        }
    }
    pub fn add_timer(&mut self, period_ms: u32) -> u32 {
        let id = self.next_timer;
        self.next_timer += 1;
        let p = Duration::from_millis(period_ms.max(1) as u64);
        self.timers.push((id, p, Instant::now() + p));
        id
    }
    pub fn remove_timer(&mut self, id: u32) {
        self.timers.retain(|t| t.0 != id);
    }
    pub fn add_fd(&mut self, fd: i32, flags: u32) {
        self.fds.retain(|f| f.0 != fd);
        self.fds.push((fd, flags));
    }
    pub fn remove_fd(&mut self, fd: i32) {
        self.fds.retain(|f| f.0 != fd);
    }
    /// Opens the editor window; returns its native handle.
    pub fn open_window(&mut self, title: &str, w: u32, h: u32) -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            if self.window.is_none() {
                self.window = x11win::Window::open(title, w, h);
            }
            self.window.as_ref().map(|w| w.id())
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (title, w, h);
            None
        }
    }
    pub fn resize_window(&mut self, w: u32, h: u32) {
        #[cfg(target_os = "linux")]
        if let Some(win) = &self.window {
            win.resize(w, h);
        }
        let _ = (w, h);
    }
    pub fn close_window(&mut self) {
        #[cfg(target_os = "linux")]
        {
            self.window = None;
        }
    }
    pub fn window_closed_by_user(&mut self) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(w) = &self.window {
            return w.poll_closed();
        }
        false
    }
    pub fn window_fd(&self) -> Option<i32> {
        #[cfg(target_os = "linux")]
        if let Some(w) = &self.window {
            return Some(w.fd());
        }
        None
    }
    fn due_timers(&mut self) -> Vec<u32> {
        let now = Instant::now();
        let mut out = vec![];
        for t in &mut self.timers {
            if t.2 <= now {
                out.push(t.0);
                t.2 = now + t.1;
            }
        }
        out
    }
    fn next_wait(&self) -> Duration {
        let now = Instant::now();
        self.timers.iter().map(|t| t.2.saturating_duration_since(now)).min().unwrap_or(Duration::from_millis(30)).min(Duration::from_millis(30))
    }
}

/// Moves the protocol to a private copy of stdout and points fd 1 at stderr, so anything a
/// plug-in prints cannot corrupt the message stream.
fn protocol_stdout() -> Box<dyn Write> {
    #[cfg(unix)]
    unsafe {
        use std::os::fd::FromRawFd;
        let fd = libc::dup(1);
        if fd >= 0 {
            libc::dup2(2, 1);
            return Box::new(std::io::BufWriter::new(std::fs::File::from_raw_fd(fd)));
        }
    }
    Box::new(std::io::BufWriter::new(std::io::stdout()))
}

fn poll_fds(sched: &Scheduler, backend: &mut dyn Backend) {
    #[cfg(unix)]
    {
        if sched.fds.is_empty() {
            return;
        }
        let mut pfds: Vec<libc::pollfd> = sched
            .fds
            .iter()
            .map(|&(fd, flags)| libc::pollfd { fd, events: (if flags & 1 != 0 { libc::POLLIN } else { 0 }) | (if flags & 2 != 0 { libc::POLLOUT } else { 0 }), revents: 0 })
            .collect();
        let n = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, 0) };
        if n > 0 {
            for p in pfds {
                if p.revents != 0 {
                    let mut flags = 0u32;
                    if p.revents & libc::POLLIN != 0 {
                        flags |= 1;
                    }
                    if p.revents & libc::POLLOUT != 0 {
                        flags |= 2;
                    }
                    if p.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                        flags |= 4;
                    }
                    backend.on_fd(p.fd, flags);
                }
            }
        }
    }
    let _ = (sched, backend);
}

fn scan(path: &str) {
    let lower = path.to_lowercase();
    let res = if lower.ends_with(".vst3") { vst3host::scan(path) } else { clap::scan(path) };
    match res {
        Ok(list) => {
            for d in list {
                println!("{}", d.to_line());
            }
        }
        Err(e) => {
            eprintln!("scan failed: {e}");
            std::process::exit(2);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--scan" {
        scan(&args[2]);
        return;
    }
    let mut out = protocol_stdout();
    let (tx, rx) = std::sync::mpsc::channel::<Request>();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            match Request::read(&mut stdin) {
                Ok(r) => {
                    if tx.send(r).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(Request::Quit);
                    break;
                }
            }
        }
    });
    let mut sched = Box::new(Scheduler::new());
    let sched_ptr: *mut Scheduler = &mut *sched;
    let mut backend: Option<Box<dyn Backend>> = None;
    let mut output = Vec::with_capacity(4096);
    loop {
        let wait = sched.next_wait();
        match rx.recv_timeout(wait) {
            Ok(req) => {
                let reply = match req {
                    Request::Load { path, id, sample_rate, max_block } => {
                        let res: Result<(Box<dyn Backend>, Loaded), String> = if path.to_lowercase().ends_with(".vst3") {
                            vst3host::Vst3Plugin::load(&path, &id, sample_rate, max_block, sched_ptr).map(|(p, l)| (Box::new(p) as Box<dyn Backend>, l))
                        } else {
                            clap::ClapPlugin::load(&path, &id, sample_rate, max_block, sched_ptr).map(|(p, l)| (Box::new(p) as Box<dyn Backend>, l))
                        };
                        match res {
                            Ok((b, l)) => {
                                backend = Some(b);
                                Reply::Loaded { name: l.name, vendor: l.vendor, instrument: l.instrument, gui: l.gui, params: l.params }
                            }
                            Err(e) => Reply::Error(e),
                        }
                    }
                    Request::Process { frames, tempo, playing, events, input } => match backend.as_mut() {
                        Some(b) => {
                            b.process(frames as usize, tempo, playing, &events, &input, &mut output);
                            Reply::Audio { frames, output: std::mem::take(&mut output) }
                        }
                        None => Reply::Audio { frames, output: vec![0.0; frames as usize * 2] },
                    },
                    Request::SetParam { id, value } => {
                        if let Some(b) = backend.as_mut() {
                            b.set_param(id, value);
                        }
                        Reply::Ok
                    }
                    Request::GetState => Reply::State(backend.as_mut().map(|b| b.get_state()).unwrap_or_default()),
                    Request::SetState(data) => match backend.as_mut().map(|b| b.set_state(&data)) {
                        Some(true) => Reply::Ok,
                        _ => Reply::Error("state not accepted".into()),
                    },
                    Request::ShowGui(on) => match backend.as_mut().map(|b| b.show_gui(on, &mut sched)) {
                        Some(true) => Reply::Ok,
                        _ => Reply::Error("this plug-in has no editor that can be shown".into()),
                    },
                    Request::Quit => break,
                };
                if reply.write(&mut out).is_err() {
                    break;
                }
                if let Reply::Audio { output: o, .. } = reply {
                    output = o;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
        if let Some(b) = backend.as_mut() {
            for id in sched.due_timers() {
                b.on_timer(id);
            }
            poll_fds(&sched, b.as_mut());
            b.idle(&mut sched);
            for (id, value) in b.take_param_changes() {
                if (Reply::Param { id, value }).write(&mut out).is_err() {
                    return;
                }
            }
        }
        let _ = sched.window_fd();
    }
    drop(backend);
}
