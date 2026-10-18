//! VST3 backend: loads a `.vst3` bundle, instantiates component + controller and runs them.

use crate::{Backend, Loaded, Scheduler};
use musicton_plugproto::{NoteEvent, ParamInfo, PluginDesc};
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void, CStr};
use std::ptr::null_mut;
use vst3::Steinberg::Linux::*;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

fn c(s: &[c_char]) -> String {
    let bytes: Vec<u8> = s.iter().take_while(|&&b| b != 0).map(|&b| b as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn u16s(s: &[u16]) -> String {
    String::from_utf16_lossy(&s.iter().take_while(|&&b| b != 0).copied().collect::<Vec<u16>>())
}

fn hex(id: &TUID) -> String {
    id.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

/// The shared library inside a bundle (`X.vst3/Contents/x86_64-linux/X.so`), or the file itself.
fn binary_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    if p.is_dir() {
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let arch = if cfg!(target_arch = "aarch64") { "aarch64-linux" } else { "x86_64-linux" };
        let cand = p.join("Contents").join(arch).join(format!("{stem}.so"));
        if cand.exists() {
            return cand.to_string_lossy().to_string();
        }
        // Some bundles name the binary differently: take the first .so.
        if let Ok(rd) = std::fs::read_dir(p.join("Contents").join(arch)) {
            for e in rd.flatten() {
                if e.path().extension().is_some_and(|x| x == "so") {
                    return e.path().to_string_lossy().to_string();
                }
            }
        }
    }
    path.to_string()
}

struct Module {
    /// Released before ModuleExit; the library itself is unloaded last.
    factory_ptr: Option<ComPtr<IPluginFactory>>,
    lib: libloading::Library,
}

impl Module {
    fn open(path: &str) -> Result<Module, String> {
        let bin = binary_path(path);
        // ModuleEntry wants the dlopen handle on Linux.
        #[cfg(unix)]
        let (lib, handle) = {
            let l = unsafe { libloading::os::unix::Library::open(Some(&bin), libc::RTLD_NOW | libc::RTLD_LOCAL) }.map_err(|e| e.to_string())?;
            let h = l.into_raw();
            (libloading::Library::from(unsafe { libloading::os::unix::Library::from_raw(h) }), h)
        };
        #[cfg(not(unix))]
        let (lib, handle) = (unsafe { libloading::Library::new(&bin) }.map_err(|e| e.to_string())?, null_mut::<c_void>());
        unsafe {
            if let Ok(entry) = lib.get::<unsafe extern "C" fn(*mut c_void) -> bool>(b"ModuleEntry\0") {
                if !entry(handle) {
                    return Err("ModuleEntry failed".into());
                }
            }
            let get = lib.get::<unsafe extern "C" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0").map_err(|e| e.to_string())?;
            let f = ComPtr::from_raw(get()).ok_or("no plugin factory")?;
            Ok(Module { factory_ptr: Some(f), lib })
        }
    }

    fn factory(&self) -> &ComPtr<IPluginFactory> {
        self.factory_ptr.as_ref().unwrap()
    }

    /// (class id, name, instrument) of every audio module class.
    fn classes(&self) -> Vec<(TUID, String, bool, String)> {
        let mut out = vec![];
        unsafe {
            let f2 = self.factory().cast::<IPluginFactory2>();
            let mut finfo: PFactoryInfo = std::mem::zeroed();
            self.factory().getFactoryInfo(&mut finfo);
            let vendor = c(&finfo.vendor);
            for i in 0..self.factory().countClasses() {
                let mut info: PClassInfo = std::mem::zeroed();
                if self.factory().getClassInfo(i, &mut info) != kResultOk || c(&info.category) != "Audio Module Class" {
                    continue;
                }
                let mut instrument = false;
                if let Some(f2) = &f2 {
                    let mut i2: PClassInfo2 = std::mem::zeroed();
                    if f2.getClassInfo2(i, &mut i2) == kResultOk {
                        instrument = c(&i2.subCategories).contains("Instrument");
                    }
                }
                out.push((info.cid, c(&info.name), instrument, vendor.clone()));
            }
        }
        out
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        self.factory_ptr = None;
        unsafe {
            if let Ok(exit) = self.lib.get::<unsafe extern "C" fn() -> bool>(b"ModuleExit\0") {
                exit();
            }
        }
    }
}

pub fn scan(path: &str) -> Result<Vec<PluginDesc>, String> {
    let m = Module::open(path)?;
    Ok(m.classes()
        .into_iter()
        .map(|(cid, name, instrument, vendor)| PluginDesc { format: "vst3".into(), path: path.into(), id: hex(&cid), name, vendor, instrument })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Host-side COM objects.

struct HostApp;
impl Class for HostApp {
    type Interfaces = (IHostApplication,);
}
impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let n: Vec<u16> = "Musicton".encode_utf16().collect();
        let dst = &mut *name;
        dst.fill(0);
        dst[..n.len()].copy_from_slice(&n);
        kResultOk
    }
    /// Plug-ins create messages through the host to talk between component and controller.
    unsafe fn createInstance(&self, _cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        *obj = null_mut();
        let want = std::slice::from_raw_parts(iid as *const u8, 16);
        if want == &IMessage::IID[..] {
            let m = ComWrapper::new(Message { id: RefCell::new(std::ffi::CString::default()), attrs: ComWrapper::new(AttrList { map: RefCell::new(Default::default()) }) });
            *obj = m.to_com_ptr::<IMessage>().unwrap().into_raw() as *mut c_void;
            return kResultOk;
        }
        if want == &IAttributeList::IID[..] {
            let a = ComWrapper::new(AttrList { map: RefCell::new(Default::default()) });
            *obj = a.to_com_ptr::<IAttributeList>().unwrap().into_raw() as *mut c_void;
            return kResultOk;
        }
        kNotImplemented
    }
}

enum AttrVal {
    Int(i64),
    Float(f64),
    Str(Vec<u16>),
    Bin(Vec<u8>),
}

struct AttrList {
    map: RefCell<std::collections::HashMap<String, AttrVal>>,
}
impl Class for AttrList {
    type Interfaces = (IAttributeList,);
}
impl IAttributeListTrait for AttrList {
    unsafe fn setInt(&self, id: IAttributeList_::AttrID, v: int64) -> tresult {
        self.map.borrow_mut().insert(CStr::from_ptr(id).to_string_lossy().into_owned(), AttrVal::Int(v));
        kResultOk
    }
    unsafe fn getInt(&self, id: IAttributeList_::AttrID, v: *mut int64) -> tresult {
        match self.map.borrow().get(CStr::from_ptr(id).to_string_lossy().as_ref()) {
            Some(AttrVal::Int(x)) => {
                *v = *x;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setFloat(&self, id: IAttributeList_::AttrID, v: f64) -> tresult {
        self.map.borrow_mut().insert(CStr::from_ptr(id).to_string_lossy().into_owned(), AttrVal::Float(v));
        kResultOk
    }
    unsafe fn getFloat(&self, id: IAttributeList_::AttrID, v: *mut f64) -> tresult {
        match self.map.borrow().get(CStr::from_ptr(id).to_string_lossy().as_ref()) {
            Some(AttrVal::Float(x)) => {
                *v = *x;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setString(&self, id: IAttributeList_::AttrID, s: *const TChar) -> tresult {
        let mut v = vec![];
        let mut i = 0;
        while *s.add(i) != 0 {
            v.push(*s.add(i) as u16);
            i += 1;
        }
        self.map.borrow_mut().insert(CStr::from_ptr(id).to_string_lossy().into_owned(), AttrVal::Str(v));
        kResultOk
    }
    unsafe fn getString(&self, id: IAttributeList_::AttrID, s: *mut TChar, size: uint32) -> tresult {
        match self.map.borrow().get(CStr::from_ptr(id).to_string_lossy().as_ref()) {
            Some(AttrVal::Str(v)) => {
                let cap = (size as usize / 2).saturating_sub(1);
                let n = v.len().min(cap);
                for (k, c) in v.iter().take(n).enumerate() {
                    *s.add(k) = *c as TChar;
                }
                *s.add(n) = 0;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
    unsafe fn setBinary(&self, id: IAttributeList_::AttrID, data: *const c_void, size: uint32) -> tresult {
        let b = std::slice::from_raw_parts(data as *const u8, size as usize).to_vec();
        self.map.borrow_mut().insert(CStr::from_ptr(id).to_string_lossy().into_owned(), AttrVal::Bin(b));
        kResultOk
    }
    unsafe fn getBinary(&self, id: IAttributeList_::AttrID, data: *mut *const c_void, size: *mut uint32) -> tresult {
        match self.map.borrow().get(CStr::from_ptr(id).to_string_lossy().as_ref()) {
            Some(AttrVal::Bin(b)) => {
                *data = b.as_ptr() as *const c_void;
                *size = b.len() as uint32;
                kResultOk
            }
            _ => kResultFalse,
        }
    }
}

struct Message {
    id: RefCell<std::ffi::CString>,
    attrs: ComWrapper<AttrList>,
}
impl Class for Message {
    type Interfaces = (IMessage,);
}
impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.borrow().as_ptr()
    }
    unsafe fn setMessageID(&self, id: FIDString) {
        *self.id.borrow_mut() = if id.is_null() { Default::default() } else { CStr::from_ptr(id).to_owned() };
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attrs.as_com_ref::<IAttributeList>().map(|r| r.as_ptr()).unwrap_or(null_mut())
    }
}

struct Handler {
    changes: RefCell<Vec<(u32, f64)>>,
}
impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}
impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, id: ParamID, v: ParamValue) -> tresult {
        self.changes.borrow_mut().push((id, v));
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, _flags: int32) -> tresult {
        kResultOk
    }
}

struct MemStream {
    data: RefCell<Vec<u8>>,
    pos: Cell<usize>,
}
impl Class for MemStream {
    type Interfaces = (IBStream,);
}
impl IBStreamTrait for MemStream {
    unsafe fn read(&self, buffer: *mut c_void, n: int32, read: *mut int32) -> tresult {
        let d = self.data.borrow();
        let p = self.pos.get();
        let k = (n.max(0) as usize).min(d.len().saturating_sub(p));
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
            1 => self.pos.get() as i64,
            2 => len,
            _ => 0,
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
            *pos = self.pos.get() as i64;
        }
        kResultOk
    }
}

fn stream(data: Vec<u8>) -> ComWrapper<MemStream> {
    ComWrapper::new(MemStream { data: RefCell::new(data), pos: Cell::new(0) })
}

struct ParamQueue {
    id: u32,
    points: RefCell<Vec<(i32, f64)>>,
}
impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}
impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }
    unsafe fn getPointCount(&self) -> int32 {
        self.points.borrow().len() as int32
    }
    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        match self.points.borrow().get(index as usize) {
            Some(&(o, v)) => {
                *offset = o;
                *value = v;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn addPoint(&self, offset: int32, value: ParamValue, index: *mut int32) -> tresult {
        let mut p = self.points.borrow_mut();
        p.push((offset, value));
        if !index.is_null() {
            *index = p.len() as int32 - 1;
        }
        kResultOk
    }
}

struct ParamChanges {
    queues: RefCell<Vec<ComWrapper<ParamQueue>>>,
}
impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}
impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        self.queues.borrow().len() as int32
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        self.queues.borrow().get(index as usize).and_then(|q| q.as_com_ref::<IParamValueQueue>()).map(|r| r.as_ptr()).unwrap_or(null_mut())
    }
    unsafe fn addParameterData(&self, id: *const ParamID, index: *mut int32) -> *mut IParamValueQueue {
        let mut qs = self.queues.borrow_mut();
        let i = match qs.iter().position(|q| q.id == *id) {
            Some(i) => i,
            None => {
                qs.push(ComWrapper::new(ParamQueue { id: *id, points: RefCell::new(vec![]) }));
                qs.len() - 1
            }
        };
        if !index.is_null() {
            *index = i as int32;
        }
        qs[i].as_com_ref::<IParamValueQueue>().map(|r| r.as_ptr()).unwrap_or(null_mut())
    }
}

struct EventList {
    events: RefCell<Vec<Event>>,
}
impl Class for EventList {
    type Interfaces = (IEventList,);
}
impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.events.borrow().len() as int32
    }
    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        match self.events.borrow().get(index as usize) {
            Some(ev) => {
                *e = *ev;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn addEvent(&self, e: *mut Event) -> tresult {
        self.events.borrow_mut().push(*e);
        kResultOk
    }
}

/// Editor frame: resizes the X11 window and provides the run loop (timers, fds) on Linux.
struct Frame {
    sched: *mut Scheduler,
    timers: RefCell<Vec<(u32, ComPtr<ITimerHandler>)>>,
    fds: RefCell<Vec<(i32, ComPtr<IEventHandler>)>>,
}
impl Class for Frame {
    type Interfaces = (IPlugFrame, IRunLoop);
}
impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, view: *mut IPlugView, size: *mut ViewRect) -> tresult {
        let r = &*size;
        (*self.sched).resize_window((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32);
        if let Some(v) = vst3::ComRef::from_raw(view) {
            v.onSize(size);
        }
        kResultOk
    }
}
impl IRunLoopTrait for Frame {
    unsafe fn registerEventHandler(&self, handler: *mut IEventHandler, fd: FileDescriptor) -> tresult {
        let Some(h) = vst3::ComRef::from_raw(handler) else { return kInvalidArgument };
        self.fds.borrow_mut().push((fd, h.to_com_ptr()));
        (*self.sched).add_fd(fd, 1);
        kResultOk
    }
    unsafe fn unregisterEventHandler(&self, handler: *mut IEventHandler) -> tresult {
        let mut fds = self.fds.borrow_mut();
        for (fd, h) in fds.iter() {
            if h.as_ptr() == handler {
                (*self.sched).remove_fd(*fd);
            }
        }
        fds.retain(|(_, h)| h.as_ptr() != handler);
        kResultOk
    }
    unsafe fn registerTimer(&self, handler: *mut ITimerHandler, ms: TimerInterval) -> tresult {
        let Some(h) = vst3::ComRef::from_raw(handler) else { return kInvalidArgument };
        let id = (*self.sched).add_timer(ms as u32);
        self.timers.borrow_mut().push((id, h.to_com_ptr()));
        kResultOk
    }
    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> tresult {
        let mut t = self.timers.borrow_mut();
        for (id, h) in t.iter() {
            if h.as_ptr() == handler {
                (*self.sched).remove_timer(*id);
            }
        }
        t.retain(|(_, h)| h.as_ptr() != handler);
        kResultOk
    }
}

// ---------------------------------------------------------------------------------------------

pub struct Vst3Plugin {
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    handler: ComWrapper<Handler>,
    _host: ComWrapper<HostApp>,
    frame: ComWrapper<Frame>,
    view: Option<ComPtr<IPlugView>>,
    in_ch: i32,
    out_ch: i32,
    has_in: bool,
    pending: Vec<(u32, f64)>,
    name: String,
    connection: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    // Kept last so the library outlives every interface above.
    _module: Module,
}

impl Vst3Plugin {
    pub fn load(path: &str, id: &str, sample_rate: f64, max_block: u32, sched: *mut Scheduler) -> Result<(Vst3Plugin, Loaded), String> {
        let module = Module::open(path)?;
        let classes = module.classes();
        let (cid, name, instrument, vendor) = classes.into_iter().find(|c| id.is_empty() || hex(&c.0) == id).ok_or_else(|| format!("class {id} not found in {path}"))?;
        unsafe {
            let host = ComWrapper::new(HostApp);
            let host_unknown = host.to_com_ptr::<FUnknown>().unwrap();
            let mut obj: *mut c_void = null_mut();
            if module.factory().createInstance(cid.as_ptr(), IComponent::IID.as_ptr() as *const c_char, &mut obj) != kResultOk || obj.is_null() {
                return Err("createInstance(IComponent) failed".into());
            }
            let component = ComPtr::from_raw(obj as *mut IComponent).ok_or("null component")?;
            if component.initialize(host_unknown.as_ptr()) != kResultOk {
                return Err("component initialize failed".into());
            }
            let processor = component.cast::<IAudioProcessor>().ok_or("no IAudioProcessor")?;
            // Controller: either the component itself or a separate class.
            let mut separate = false;
            let controller = match component.cast::<IEditController>() {
                Some(c) => Some(c),
                None => {
                    let mut ccid: TUID = [0; 16];
                    let mut ctrl = None;
                    if component.getControllerClassId(&mut ccid) == kResultOk {
                        let mut o: *mut c_void = null_mut();
                        if module.factory().createInstance(ccid.as_ptr(), IEditController::IID.as_ptr() as *const c_char, &mut o) == kResultOk && !o.is_null() {
                            let c = ComPtr::from_raw(o as *mut IEditController).unwrap();
                            if c.initialize(host_unknown.as_ptr()) == kResultOk {
                                ctrl = Some(c);
                                separate = true;
                            }
                        }
                    }
                    ctrl
                }
            };
            // Separate component and controller talk through connection points.
            let mut connection = None;
            if let (true, Some(c)) = (separate, &controller) {
                if let (Some(a), Some(b)) = (component.cast::<IConnectionPoint>(), c.cast::<IConnectionPoint>()) {
                    a.connect(b.as_ptr());
                    b.connect(a.as_ptr());
                    connection = Some((a, b));
                }
            }
            let handler = ComWrapper::new(Handler { changes: RefCell::new(vec![]) });
            if let Some(c) = &controller {
                c.setComponentHandler(handler.to_com_ptr::<IComponentHandler>().unwrap().as_ptr());
                if separate {
                    // Give the controller the component's initial state.
                    let s = stream(vec![]);
                    let sp = s.to_com_ptr::<IBStream>().unwrap();
                    if component.getState(sp.as_ptr()) == kResultOk {
                        let s2 = stream(s.data.borrow().clone());
                        c.setComponentState(s2.to_com_ptr::<IBStream>().unwrap().as_ptr());
                    }
                }
            }
            // Buses: main stereo in/out, first event input.
            let n_in = component.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32);
            let n_out = component.getBusCount(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32);
            let mut ins: Vec<SpeakerArrangement> = vec![SpeakerArr::kStereo; n_in.max(0) as usize];
            let mut outs: Vec<SpeakerArrangement> = vec![SpeakerArr::kStereo; n_out.max(0) as usize];
            processor.setBusArrangements(ins.as_mut_ptr(), ins.len() as i32, outs.as_mut_ptr(), outs.len() as i32);
            let mut in_arr: SpeakerArrangement = SpeakerArr::kStereo;
            let mut out_arr: SpeakerArrangement = SpeakerArr::kStereo;
            if n_in > 0 {
                processor.getBusArrangement(BusDirections_::kInput as i32, 0, &mut in_arr);
                component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kInput as i32, 0, 1);
            }
            if n_out > 0 {
                processor.getBusArrangement(BusDirections_::kOutput as i32, 0, &mut out_arr);
                component.activateBus(MediaTypes_::kAudio as i32, BusDirections_::kOutput as i32, 0, 1);
            }
            if component.getBusCount(MediaTypes_::kEvent as i32, BusDirections_::kInput as i32) > 0 {
                component.activateBus(MediaTypes_::kEvent as i32, BusDirections_::kInput as i32, 0, 1);
            }
            let mut setup = ProcessSetup { processMode: ProcessModes_::kRealtime as i32, symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32, maxSamplesPerBlock: max_block as i32, sampleRate: sample_rate };
            if processor.setupProcessing(&mut setup) != kResultOk {
                return Err("setupProcessing failed".into());
            }
            component.setActive(1);
            processor.setProcessing(1);
            let gui = controller.as_ref().is_some_and(|c| {
                let v = c.createView(c"editor".as_ptr());
                match ComPtr::from_raw(v) {
                    Some(v) => v.isPlatformTypeSupported(kPlatformTypeX11EmbedWindowID) == kResultOk,
                    None => false,
                }
            });
            let frame = ComWrapper::new(Frame { sched, timers: RefCell::new(vec![]), fds: RefCell::new(vec![]) });
            let p = Vst3Plugin {
                component,
                processor,
                controller,
                separate_controller: separate,
                handler,
                _host: host,
                frame,
                view: None,
                in_ch: (in_arr.count_ones() as i32).min(2),
                out_ch: (out_arr.count_ones() as i32).clamp(1, 2),
                has_in: n_in > 0,
                pending: vec![],
                name: name.clone(),
                connection,
                _module: module,
            };
            let params = p.param_infos();
            Ok((p, Loaded { name, vendor, instrument, gui, params }))
        }
    }

    fn param_infos(&self) -> Vec<ParamInfo> {
        let Some(c) = &self.controller else { return vec![] };
        unsafe {
            (0..c.getParameterCount())
                .filter_map(|i| {
                    let mut info: ParameterInfo = std::mem::zeroed();
                    if c.getParameterInfo(i, &mut info) != kResultOk || info.flags & ParameterInfo_::ParameterFlags_::kIsHidden != 0 {
                        return None;
                    }
                    Some(ParamInfo {
                        id: info.id,
                        name: u16s(&info.title),
                        min: 0.0,
                        max: 1.0,
                        default: info.defaultNormalizedValue,
                        value: c.getParamNormalized(info.id),
                        steps: info.stepCount.max(0) as u32,
                    })
                })
                .collect()
        }
    }
}

impl Backend for Vst3Plugin {
    fn process(&mut self, frames: usize, tempo: f64, playing: bool, events: &[NoteEvent], input: &[f32], output: &mut Vec<f32>) {
        let frames = frames.max(1);
        let mut in_l = input.get(..frames).map(|s| s.to_vec()).unwrap_or_else(|| vec![0.0; frames]);
        let mut in_r = input.get(frames..frames * 2).map(|s| s.to_vec()).unwrap_or_else(|| vec![0.0; frames]);
        if self.in_ch == 1 {
            for i in 0..frames {
                in_l[i] = (in_l[i] + in_r[i]) * 0.5;
            }
        }
        let mut out_l = vec![0.0f32; frames];
        let mut out_r = vec![0.0f32; frames];
        let mut in_ptrs = [in_l.as_mut_ptr(), in_r.as_mut_ptr()];
        let mut out_ptrs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        let mut in_bus = AudioBusBuffers { numChannels: self.in_ch, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: in_ptrs.as_mut_ptr() } };
        let mut out_bus = AudioBusBuffers { numChannels: self.out_ch, silenceFlags: 0, __field0: AudioBusBuffers__type0 { channelBuffers32: out_ptrs.as_mut_ptr() } };
        let evs: Vec<Event> = events
            .iter()
            .map(|e| unsafe {
                let mut ev: Event = std::mem::zeroed();
                ev.busIndex = 0;
                ev.sampleOffset = e.offset.min(frames as u32 - 1) as i32;
                if e.on {
                    ev.r#type = Event_::EventTypes_::kNoteOnEvent as u16;
                    ev.__field0.noteOn = NoteOnEvent { channel: 0, pitch: e.key as i16, tuning: 0.0, velocity: e.velocity as f32 / 127.0, length: 0, noteId: -1 };
                } else {
                    ev.r#type = Event_::EventTypes_::kNoteOffEvent as u16;
                    ev.__field0.noteOff = NoteOffEvent { channel: 0, pitch: e.key as i16, velocity: 0.0, noteId: -1, tuning: 0.0 };
                }
                ev
            })
            .collect();
        let event_list = ComWrapper::new(EventList { events: RefCell::new(evs) });
        let changes = ComWrapper::new(ParamChanges { queues: RefCell::new(vec![]) });
        for (id, v) in self.pending.drain(..) {
            let q = ComWrapper::new(ParamQueue { id, points: RefCell::new(vec![(0, v)]) });
            changes.queues.borrow_mut().push(q);
        }
        let out_changes = ComWrapper::new(ParamChanges { queues: RefCell::new(vec![]) });
        let mut ctx: ProcessContext = unsafe { std::mem::zeroed() };
        ctx.tempo = tempo;
        ctx.state = (if playing { ProcessContext_::StatesAndFlags_::kPlaying } else { 0 } | ProcessContext_::StatesAndFlags_::kTempoValid) as u32;
        ctx.timeSigNumerator = 4;
        ctx.timeSigDenominator = 4;
        ctx.sampleRate = 0.0;
        let el = event_list.to_com_ptr::<IEventList>().unwrap();
        let pc = changes.to_com_ptr::<IParameterChanges>().unwrap();
        let opc = out_changes.to_com_ptr::<IParameterChanges>().unwrap();
        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as i32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as i32,
            numSamples: frames as i32,
            numInputs: if self.has_in { 1 } else { 0 },
            numOutputs: 1,
            inputs: if self.has_in { &mut in_bus } else { null_mut() },
            outputs: &mut out_bus,
            inputParameterChanges: pc.as_ptr(),
            outputParameterChanges: opc.as_ptr(),
            inputEvents: el.as_ptr(),
            outputEvents: null_mut(),
            processContext: &mut ctx,
        };
        unsafe {
            self.processor.process(&mut data);
        }
        // Parameter values the processor reports (e.g. meters, its own automation).
        for q in out_changes.queues.borrow().iter() {
            if let Some(&(_, v)) = q.points.borrow().last() {
                self.handler.changes.borrow_mut().push((q.id, v));
            }
        }
        output.clear();
        output.extend_from_slice(&out_l);
        if self.out_ch == 1 {
            output.extend_from_slice(&out_l);
        } else {
            output.extend_from_slice(&out_r);
        }
    }

    fn set_param(&mut self, id: u32, value: f64) {
        if let Some(c) = &self.controller {
            unsafe { c.setParamNormalized(id, value) };
        }
        self.pending.push((id, value));
    }

    fn take_param_changes(&mut self) -> Vec<(u32, f64)> {
        let changes = std::mem::take(&mut *self.handler.changes.borrow_mut());
        // Edits made in the plug-in's own editor also have to reach the processor.
        for &(id, v) in &changes {
            self.pending.push((id, v));
        }
        changes
    }

    fn get_state(&mut self) -> Vec<u8> {
        unsafe {
            let a = stream(vec![]);
            self.component.getState(a.to_com_ptr::<IBStream>().unwrap().as_ptr());
            let b = stream(vec![]);
            if let Some(c) = &self.controller {
                c.getState(b.to_com_ptr::<IBStream>().unwrap().as_ptr());
            }
            let (a, b) = (a.data.borrow().clone(), b.data.borrow().clone());
            let mut out = (a.len() as u32).to_le_bytes().to_vec();
            out.extend_from_slice(&a);
            out.extend_from_slice(&b);
            out
        }
    }

    fn set_state(&mut self, data: &[u8]) -> bool {
        if data.len() < 4 {
            return false;
        }
        let n = (u32::from_le_bytes(data[..4].try_into().unwrap()) as usize).min(data.len() - 4);
        let (a, b) = (data[4..4 + n].to_vec(), data[4 + n..].to_vec());
        unsafe {
            let ok = self.component.setState(stream(a.clone()).to_com_ptr::<IBStream>().unwrap().as_ptr()) == kResultOk;
            if let Some(c) = &self.controller {
                if self.separate_controller {
                    c.setComponentState(stream(a).to_com_ptr::<IBStream>().unwrap().as_ptr());
                }
                if !b.is_empty() {
                    c.setState(stream(b).to_com_ptr::<IBStream>().unwrap().as_ptr());
                }
            }
            ok
        }
    }

    fn show_gui(&mut self, on: bool, sched: &mut Scheduler) -> bool {
        unsafe {
            if !on {
                if let Some(v) = self.view.take() {
                    v.removed();
                    v.setFrame(null_mut());
                }
                sched.close_window();
                return true;
            }
            if self.view.is_some() {
                return true;
            }
            let Some(c) = &self.controller else { return false };
            let Some(view) = ComPtr::from_raw(c.createView(c"editor".as_ptr())) else { return false };
            if view.isPlatformTypeSupported(kPlatformTypeX11EmbedWindowID) != kResultOk {
                return false;
            }
            let mut r = ViewRect { left: 0, top: 0, right: 640, bottom: 400 };
            view.getSize(&mut r);
            let Some(xid) = sched.open_window(&self.name, (r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32) else { return false };
            view.setFrame(self.frame.to_com_ptr::<IPlugFrame>().unwrap().as_ptr());
            if view.attached(xid as usize as *mut c_void, kPlatformTypeX11EmbedWindowID) != kResultOk {
                sched.close_window();
                return false;
            }
            self.view = Some(view);
            true
        }
    }

    fn on_timer(&mut self, id: u32) {
        let handler = self.frame.timers.borrow().iter().find(|t| t.0 == id).map(|t| t.1.clone());
        if let Some(h) = handler {
            unsafe { h.onTimer() };
        }
    }

    fn on_fd(&mut self, fd: i32, _flags: u32) {
        let handler = self.frame.fds.borrow().iter().find(|t| t.0 == fd).map(|t| t.1.clone());
        if let Some(h) = handler {
            unsafe { h.onFDIsSet(fd) };
        }
    }

    fn idle(&mut self, sched: &mut Scheduler) {
        if self.view.is_some() && sched.window_closed_by_user() {
            self.show_gui(false, sched);
        }
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        unsafe {
            if let Some(v) = self.view.take() {
                v.removed();
            }
            self.processor.setProcessing(0);
            self.component.setActive(0);
            if let Some((a, b)) = self.connection.take() {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
            if let Some(c) = &self.controller {
                c.setComponentHandler(null_mut());
                if self.separate_controller {
                    c.terminate();
                }
            }
            self.component.terminate();
        }
        let _ = CStr::from_bytes_with_nul(b"\0");
    }
}
