//! CLAP backend: loads a `.clap` module, instantiates one plug-in and runs it.

use crate::{Backend, Loaded, Scheduler};
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::gui::*;
use clap_sys::ext::log::*;
use clap_sys::ext::params::*;
use clap_sys::ext::posix_fd_support::*;
use clap_sys::ext::state::*;
use clap_sys::ext::thread_check::*;
use clap_sys::ext::timer_support::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::stream::*;
use clap_sys::version::CLAP_VERSION;
use musicton_plugproto::{NoteEvent, ParamInfo, PluginDesc};
use std::cell::RefCell;
use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr::{null, null_mut};

fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

fn is_instrument(features: *const *const c_char) -> bool {
    if features.is_null() {
        return false;
    }
    let mut i = 0;
    loop {
        let f = unsafe { *features.add(i) };
        if f.is_null() {
            return false;
        }
        if cstr(f) == "instrument" {
            return true;
        }
        i += 1;
    }
}

struct Module {
    _lib: libloading::Library,
    entry: *const clap_plugin_entry,
}

impl Module {
    fn open(path: &str) -> Result<Module, String> {
        let lib = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
        let entry: *const clap_plugin_entry = unsafe { *lib.get::<*const clap_plugin_entry>(b"clap_entry\0").map_err(|e| e.to_string())? };
        if entry.is_null() {
            return Err("no clap_entry".into());
        }
        let p = CString::new(path).unwrap();
        let ok = unsafe { ((*entry).init.ok_or("no init")?)(p.as_ptr()) };
        if !ok {
            return Err("clap_entry.init failed".into());
        }
        Ok(Module { _lib: lib, entry })
    }
    fn factory(&self) -> Result<*const clap_plugin_factory, String> {
        let f = unsafe { ((*self.entry).get_factory.ok_or("no get_factory")?)(CLAP_PLUGIN_FACTORY_ID.as_ptr()) } as *const clap_plugin_factory;
        if f.is_null() {
            Err("no plugin factory".into())
        } else {
            Ok(f)
        }
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            if let Some(d) = (*self.entry).deinit {
                d();
            }
        }
    }
}

pub fn scan(path: &str) -> Result<Vec<PluginDesc>, String> {
    let m = Module::open(path)?;
    let f = m.factory()?;
    let n = unsafe { ((*f).get_plugin_count.ok_or("no count")?)(f) };
    let mut out = vec![];
    for i in 0..n {
        let d = unsafe { ((*f).get_plugin_descriptor.ok_or("no descriptor")?)(f, i) };
        if d.is_null() {
            continue;
        }
        let d = unsafe { &*d };
        out.push(PluginDesc { format: "clap".into(), path: path.into(), id: cstr(d.id), name: cstr(d.name), vendor: cstr(d.vendor), instrument: is_instrument(d.features) });
    }
    Ok(out)
}

/// Host-side state the plug-in can reach through `host_data`.
struct HostData {
    param_changes: RefCell<Vec<(u32, f64)>>,
    wants_callback: RefCell<bool>,
    sched: *mut Scheduler,
    plugin: RefCell<*const clap_plugin>,
}

unsafe extern "C" fn host_get_extension(_h: *const clap_host, id: *const c_char) -> *const c_void {
    let id = CStr::from_ptr(id);
    if id == CLAP_EXT_LOG {
        &HOST_LOG as *const _ as *const c_void
    } else if id == CLAP_EXT_THREAD_CHECK {
        &HOST_THREAD_CHECK as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &HOST_PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_TIMER_SUPPORT {
        &HOST_TIMER as *const _ as *const c_void
    } else if id == CLAP_EXT_POSIX_FD_SUPPORT {
        &HOST_FD as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &HOST_GUI as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &HOST_STATE as *const _ as *const c_void
    } else {
        null()
    }
}
unsafe extern "C" fn host_noop(_h: *const clap_host) {}
unsafe extern "C" fn host_request_callback(h: *const clap_host) {
    let d = &*((*h).host_data as *const HostData);
    *d.wants_callback.borrow_mut() = true;
}
unsafe extern "C" fn host_log(_h: *const clap_host, _sev: clap_log_severity, msg: *const c_char) {
    eprintln!("[plugin] {}", cstr(msg));
}
static HOST_LOG: clap_host_log = clap_host_log { log: Some(host_log) };
unsafe extern "C" fn yes(_h: *const clap_host) -> bool {
    true
}
// Everything runs on one thread: it is both the main and the audio thread.
static HOST_THREAD_CHECK: clap_host_thread_check = clap_host_thread_check { is_main_thread: Some(yes), is_audio_thread: Some(yes) };
unsafe extern "C" fn params_rescan(_h: *const clap_host, _f: clap_param_rescan_flags) {}
unsafe extern "C" fn params_clear(_h: *const clap_host, _id: clap_id, _f: clap_param_clear_flags) {}
static HOST_PARAMS: clap_host_params = clap_host_params { rescan: Some(params_rescan), clear: Some(params_clear), request_flush: Some(host_noop) };
unsafe extern "C" fn timer_register(h: *const clap_host, period_ms: u32, id: *mut clap_id) -> bool {
    let d = &*((*h).host_data as *const HostData);
    *id = (*d.sched).add_timer(period_ms);
    true
}
unsafe extern "C" fn timer_unregister(h: *const clap_host, id: clap_id) -> bool {
    let d = &*((*h).host_data as *const HostData);
    (*d.sched).remove_timer(id);
    true
}
static HOST_TIMER: clap_host_timer_support = clap_host_timer_support { register_timer: Some(timer_register), unregister_timer: Some(timer_unregister) };
unsafe extern "C" fn fd_register(h: *const clap_host, fd: i32, flags: clap_posix_fd_flags) -> bool {
    let d = &*((*h).host_data as *const HostData);
    (*d.sched).add_fd(fd, flags);
    true
}
unsafe extern "C" fn fd_modify(h: *const clap_host, fd: i32, flags: clap_posix_fd_flags) -> bool {
    fd_register(h, fd, flags)
}
unsafe extern "C" fn fd_unregister(h: *const clap_host, fd: i32) -> bool {
    let d = &*((*h).host_data as *const HostData);
    (*d.sched).remove_fd(fd);
    true
}
static HOST_FD: clap_host_posix_fd_support = clap_host_posix_fd_support { register_fd: Some(fd_register), modify_fd: Some(fd_modify), unregister_fd: Some(fd_unregister) };
unsafe extern "C" fn gui_resize_hints_changed(_h: *const clap_host) {}
unsafe extern "C" fn gui_request_resize(h: *const clap_host, w: u32, hh: u32) -> bool {
    let d = &*((*h).host_data as *const HostData);
    (*d.sched).resize_window(w, hh);
    true
}
unsafe extern "C" fn gui_request_show(_h: *const clap_host) -> bool {
    true
}
unsafe extern "C" fn gui_closed(h: *const clap_host, _destroyed: bool) {
    let d = &*((*h).host_data as *const HostData);
    (*d.sched).gui_closed = true;
}
static HOST_GUI: clap_host_gui = clap_host_gui {
    resize_hints_changed: Some(gui_resize_hints_changed),
    request_resize: Some(gui_request_resize),
    request_show: Some(gui_request_show),
    request_hide: Some(gui_request_show),
    closed: Some(gui_closed),
};
static HOST_STATE: clap_host_state = clap_host_state { mark_dirty: Some(host_noop) };

unsafe extern "C" fn in_size(list: *const clap_input_events) -> u32 {
    let evs = &*((*list).ctx as *const Vec<Ev>);
    evs.len() as u32
}
unsafe extern "C" fn in_get(list: *const clap_input_events, i: u32) -> *const clap_event_header {
    let evs = &*((*list).ctx as *const Vec<Ev>);
    match evs.get(i as usize) {
        Some(Ev::Note(n)) => &n.header,
        Some(Ev::Param(p)) => &p.header,
        None => null(),
    }
}
unsafe extern "C" fn out_push(list: *const clap_output_events, e: *const clap_event_header) -> bool {
    let d = &*((*list).ctx as *const HostData);
    if (*e).space_id == CLAP_CORE_EVENT_SPACE_ID && (*e).type_ == CLAP_EVENT_PARAM_VALUE {
        let p = &*(e as *const clap_event_param_value);
        d.param_changes.borrow_mut().push((p.param_id, p.value));
    }
    true
}

enum Ev {
    Note(clap_event_note),
    Param(clap_event_param_value),
}

unsafe extern "C" fn ostream_write(s: *const clap_ostream, buf: *const c_void, size: u64) -> i64 {
    let v = &mut *((*s).ctx as *mut Vec<u8>);
    v.extend_from_slice(std::slice::from_raw_parts(buf as *const u8, size as usize));
    size as i64
}
unsafe extern "C" fn istream_read(s: *const clap_istream, buf: *mut c_void, size: u64) -> i64 {
    let (data, pos) = &mut *((*s).ctx as *mut (Vec<u8>, usize));
    let n = (size as usize).min(data.len() - *pos);
    std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buf as *mut u8, n);
    *pos += n;
    n as i64
}

pub struct ClapPlugin {
    _module: Module,
    host: Box<clap_host>,
    data: Box<HostData>,
    plugin: *const clap_plugin,
    params: *const clap_plugin_params,
    state: *const clap_plugin_state,
    gui: *const clap_plugin_gui,
    timer: *const clap_plugin_timer_support,
    fd: *const clap_plugin_posix_fd_support,
    ins: (u32, u32),
    outs: (u32, u32),
    pending: Vec<Ev>,
    steady: i64,
    gui_open: bool,
    name: String,
}

unsafe fn ext<T>(p: *const clap_plugin, id: &CStr) -> *const T {
    match (*p).get_extension {
        Some(f) => f(p, id.as_ptr()) as *const T,
        None => null(),
    }
}

impl ClapPlugin {
    pub fn load(path: &str, id: &str, sample_rate: f64, max_block: u32, sched: *mut Scheduler) -> Result<(ClapPlugin, Loaded), String> {
        let module = Module::open(path)?;
        let factory = module.factory()?;
        let data = Box::new(HostData { param_changes: RefCell::new(vec![]), wants_callback: RefCell::new(false), sched, plugin: RefCell::new(null()) });
        let host = Box::new(clap_host {
            clap_version: CLAP_VERSION,
            host_data: &*data as *const HostData as *mut c_void,
            name: c"Musicton".as_ptr(),
            vendor: c"Musicton".as_ptr(),
            url: c"https://codekiste.dev".as_ptr(),
            version: c"0.1".as_ptr(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_noop),
            request_process: Some(host_noop),
            request_callback: Some(host_request_callback),
        });
        unsafe {
            // Pick the requested plug-in, or the first one in the file.
            let n = ((*factory).get_plugin_count.ok_or("no count")?)(factory);
            let mut chosen = None;
            for i in 0..n {
                let d = ((*factory).get_plugin_descriptor.ok_or("no descriptor")?)(factory, i);
                if !d.is_null() && (id.is_empty() || cstr((*d).id) == id) {
                    chosen = Some(d);
                    break;
                }
            }
            let desc = chosen.ok_or_else(|| format!("plug-in '{id}' not found in {path}"))?;
            let plugin = ((*factory).create_plugin.ok_or("no create")?)(factory, &*host, (*desc).id);
            if plugin.is_null() {
                return Err("create_plugin failed".into());
            }
            *data.plugin.borrow_mut() = plugin;
            if !((*plugin).init.ok_or("no init")?)(plugin) {
                return Err("plugin init failed".into());
            }
            let ports: *const clap_plugin_audio_ports = ext(plugin, CLAP_EXT_AUDIO_PORTS);
            let port = |input: bool| -> (u32, u32) {
                if ports.is_null() {
                    return (0, if input { 0 } else { 2 });
                }
                let count = ((*ports).count.unwrap())(plugin, input);
                let mut info: clap_audio_port_info = std::mem::zeroed();
                if count > 0 && ((*ports).get.unwrap())(plugin, 0, input, &mut info) {
                    (count, info.channel_count)
                } else {
                    (0, 0)
                }
            };
            let ins = port(true);
            let outs = port(false);
            if !((*plugin).activate.ok_or("no activate")?)(plugin, sample_rate, 1, max_block) {
                return Err("activate failed".into());
            }
            if let Some(sp) = (*plugin).start_processing {
                sp(plugin);
            }
            let params: *const clap_plugin_params = ext(plugin, CLAP_EXT_PARAMS);
            let gui: *const clap_plugin_gui = ext(plugin, CLAP_EXT_GUI);
            let has_gui = !gui.is_null() && ((*gui).is_api_supported.unwrap())(plugin, CLAP_WINDOW_API_X11.as_ptr(), false);
            let mut p = ClapPlugin {
                _module: module,
                host,
                data,
                plugin,
                params,
                state: ext(plugin, CLAP_EXT_STATE),
                gui,
                timer: ext(plugin, CLAP_EXT_TIMER_SUPPORT),
                fd: ext(plugin, CLAP_EXT_POSIX_FD_SUPPORT),
                ins,
                outs,
                pending: vec![],
                steady: 0,
                gui_open: false,
                name: cstr((*desc).name),
            };
            let loaded = Loaded {
                name: p.name.clone(),
                vendor: cstr((*desc).vendor),
                instrument: is_instrument((*desc).features),
                gui: has_gui,
                params: p.param_infos(),
            };
            let _ = &mut p;
            Ok((p, loaded))
        }
    }

    fn param_infos(&self) -> Vec<ParamInfo> {
        if self.params.is_null() {
            return vec![];
        }
        unsafe {
            let n = ((*self.params).count.unwrap())(self.plugin);
            let mut out = vec![];
            for i in 0..n {
                let mut info: clap_param_info = std::mem::zeroed();
                if !((*self.params).get_info.unwrap())(self.plugin, i, &mut info) || info.flags & CLAP_PARAM_IS_HIDDEN != 0 {
                    continue;
                }
                let mut value = info.default_value;
                ((*self.params).get_value.unwrap())(self.plugin, info.id, &mut value);
                let steps = if info.flags & CLAP_PARAM_IS_STEPPED != 0 { (info.max_value - info.min_value).round().max(1.0) as u32 } else { 0 };
                out.push(ParamInfo { id: info.id, name: cstr(info.name.as_ptr()), min: info.min_value, max: info.max_value, default: info.default_value, value, steps });
            }
            out
        }
    }
}

impl Backend for ClapPlugin {
    fn process(&mut self, frames: usize, tempo: f64, playing: bool, events: &[NoteEvent], input: &[f32], output: &mut Vec<f32>) {
        let frames = frames.max(1);
        for e in events {
            self.pending.push(Ev::Note(clap_event_note {
                header: clap_event_header {
                    size: std::mem::size_of::<clap_event_note>() as u32,
                    time: e.offset.min(frames as u32 - 1),
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    type_: if e.on { CLAP_EVENT_NOTE_ON } else { CLAP_EVENT_NOTE_OFF },
                    flags: 0,
                },
                note_id: -1,
                port_index: 0,
                channel: 0,
                key: e.key as i16,
                velocity: e.velocity as f64 / 127.0,
            }));
        }
        // Events must be sorted by time.
        self.pending.sort_by_key(|e| match e {
            Ev::Note(n) => n.header.time,
            Ev::Param(p) => p.header.time,
        });
        let mut in_l = input.get(..frames).map(|s| s.to_vec()).unwrap_or_else(|| vec![0.0; frames]);
        let mut in_r = input.get(frames..frames * 2).map(|s| s.to_vec()).unwrap_or_else(|| vec![0.0; frames]);
        let oc = self.outs.1.max(1) as usize;
        let mut outb: Vec<Vec<f32>> = vec![vec![0.0; frames]; oc];
        let mut in_ptrs: Vec<*mut f32> = match self.ins.1 {
            0 => vec![],
            1 => {
                for i in 0..frames {
                    in_l[i] = (in_l[i] + in_r[i]) * 0.5;
                }
                vec![in_l.as_mut_ptr()]
            }
            _ => vec![in_l.as_mut_ptr(), in_r.as_mut_ptr()],
        };
        let mut out_ptrs: Vec<*mut f32> = outb.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let in_buf = clap_audio_buffer { data32: in_ptrs.as_mut_ptr(), data64: null_mut(), channel_count: in_ptrs.len() as u32, latency: 0, constant_mask: 0 };
        let mut out_buf = clap_audio_buffer { data32: out_ptrs.as_mut_ptr(), data64: null_mut(), channel_count: oc as u32, latency: 0, constant_mask: 0 };
        let in_events = clap_input_events { ctx: &self.pending as *const Vec<Ev> as *mut c_void, size: Some(in_size), get: Some(in_get) };
        let out_events = clap_output_events { ctx: &*self.data as *const HostData as *mut c_void, try_push: Some(out_push) };
        let transport = clap_event_transport {
            header: clap_event_header { size: std::mem::size_of::<clap_event_transport>() as u32, time: 0, space_id: CLAP_CORE_EVENT_SPACE_ID, type_: CLAP_EVENT_TRANSPORT, flags: 0 },
            flags: CLAP_TRANSPORT_HAS_TEMPO | if playing { CLAP_TRANSPORT_IS_PLAYING } else { 0 },
            song_pos_beats: 0,
            song_pos_seconds: 0,
            tempo,
            tempo_inc: 0.0,
            loop_start_beats: 0,
            loop_end_beats: 0,
            loop_start_seconds: 0,
            loop_end_seconds: 0,
            bar_start: 0,
            bar_number: 0,
            tsig_num: 4,
            tsig_denom: 4,
        };
        let process = clap_process {
            steady_time: self.steady,
            frames_count: frames as u32,
            transport: &transport,
            audio_inputs: if self.ins.0 > 0 { &in_buf } else { null() },
            audio_outputs: if self.outs.0 > 0 { &mut out_buf } else { null_mut() },
            audio_inputs_count: self.ins.0.min(1),
            audio_outputs_count: self.outs.0.min(1),
            in_events: &in_events,
            out_events: &out_events,
        };
        unsafe {
            if let Some(f) = (*self.plugin).process {
                f(self.plugin, &process);
            }
        }
        self.steady += frames as i64;
        self.pending.clear();
        output.clear();
        output.extend_from_slice(&outb[0]);
        output.extend_from_slice(&outb[oc.min(2) - 1]);
    }

    fn set_param(&mut self, id: u32, value: f64) {
        self.pending.push(Ev::Param(clap_event_param_value {
            header: clap_event_header {
                size: std::mem::size_of::<clap_event_param_value>() as u32,
                time: 0,
                space_id: CLAP_CORE_EVENT_SPACE_ID,
                type_: CLAP_EVENT_PARAM_VALUE,
                flags: 0,
            },
            param_id: id,
            cookie: null_mut(),
            note_id: -1,
            port_index: -1,
            channel: -1,
            key: -1,
            value,
        }));
    }

    fn take_param_changes(&mut self) -> Vec<(u32, f64)> {
        std::mem::take(&mut *self.data.param_changes.borrow_mut())
    }

    fn get_state(&mut self) -> Vec<u8> {
        let mut buf: Vec<u8> = vec![];
        if !self.state.is_null() {
            let s = clap_ostream { ctx: &mut buf as *mut Vec<u8> as *mut c_void, write: Some(ostream_write) };
            unsafe { ((*self.state).save.unwrap())(self.plugin, &s) };
        }
        buf
    }

    fn set_state(&mut self, data: &[u8]) -> bool {
        if self.state.is_null() {
            return false;
        }
        let mut ctx = (data.to_vec(), 0usize);
        let s = clap_istream { ctx: &mut ctx as *mut (Vec<u8>, usize) as *mut c_void, read: Some(istream_read) };
        unsafe { ((*self.state).load.unwrap())(self.plugin, &s) }
    }

    fn show_gui(&mut self, on: bool, sched: &mut Scheduler) -> bool {
        // Editor windows are embedded into an X11 window; other systems are not supported yet.
        if self.gui.is_null() || !cfg!(target_os = "linux") {
            return false;
        }
        unsafe {
            let g = &*self.gui;
            if !on {
                if self.gui_open {
                    (g.hide.unwrap())(self.plugin);
                    (g.destroy.unwrap())(self.plugin);
                    self.gui_open = false;
                }
                sched.close_window();
                return true;
            }
            if self.gui_open {
                return true;
            }
            if !(g.create.unwrap())(self.plugin, CLAP_WINDOW_API_X11.as_ptr(), false) {
                return false;
            }
            let (mut w, mut h) = (640u32, 400u32);
            (g.get_size.unwrap())(self.plugin, &mut w, &mut h);
            let Some(xid) = sched.open_window(&self.name, w, h) else {
                (g.destroy.unwrap())(self.plugin);
                return false;
            };
            let window = clap_window { api: CLAP_WINDOW_API_X11.as_ptr(), specific: clap_window_handle { x11: xid as std::ffi::c_ulong } };
            (g.set_parent.unwrap())(self.plugin, &window);
            (g.show.unwrap())(self.plugin);
            self.gui_open = true;
            true
        }
    }

    fn on_timer(&mut self, id: u32) {
        if !self.timer.is_null() {
            unsafe { ((*self.timer).on_timer.unwrap())(self.plugin, id) };
        }
    }

    fn on_fd(&mut self, fd: i32, flags: u32) {
        if !self.fd.is_null() {
            unsafe { ((*self.fd).on_fd.unwrap())(self.plugin, fd, flags) };
        }
    }

    fn idle(&mut self, sched: &mut Scheduler) {
        let wants = std::mem::take(&mut *self.data.wants_callback.borrow_mut());
        if wants {
            unsafe {
                if let Some(f) = (*self.plugin).on_main_thread {
                    f(self.plugin);
                }
            }
        }
        if sched.gui_closed && self.gui_open {
            sched.gui_closed = false;
            self.show_gui(false, sched);
        }
        if sched.window_closed_by_user() && self.gui_open {
            self.show_gui(false, sched);
        }
    }
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        unsafe {
            if self.gui_open && !self.gui.is_null() {
                ((*self.gui).destroy.unwrap())(self.plugin);
            }
            if let Some(f) = (*self.plugin).stop_processing {
                f(self.plugin);
            }
            if let Some(f) = (*self.plugin).deactivate {
                f(self.plugin);
            }
            if let Some(f) = (*self.plugin).destroy {
                f(self.plugin);
            }
        }
        let _ = &self.host;
    }
}
