//! Minimal X11 top-level window used as the parent of a plug-in editor (libX11 is loaded at
//! runtime, so there is no build-time dependency on X11 headers).

use std::ffi::CString;
use x11_dl::xlib;

pub struct Window {
    x: xlib::Xlib,
    display: *mut xlib::Display,
    win: xlib::Window,
    wm_delete: xlib::Atom,
}

impl Window {
    pub fn open(title: &str, w: u32, h: u32) -> Option<Window> {
        let x = xlib::Xlib::open().ok()?;
        unsafe {
            let display = (x.XOpenDisplay)(std::ptr::null());
            if display.is_null() {
                return None;
            }
            let screen = (x.XDefaultScreen)(display);
            let root = (x.XRootWindow)(display, screen);
            let win = (x.XCreateSimpleWindow)(display, root, 0, 0, w.max(64), h.max(48), 0, 0, (x.XBlackPixel)(display, screen));
            let t = CString::new(title).unwrap_or_default();
            (x.XStoreName)(display, win, t.as_ptr());
            (x.XSelectInput)(display, win, xlib::StructureNotifyMask);
            let name = CString::new("WM_DELETE_WINDOW").unwrap();
            let mut wm_delete = (x.XInternAtom)(display, name.as_ptr(), 0);
            (x.XSetWMProtocols)(display, win, &mut wm_delete, 1);
            (x.XMapWindow)(display, win);
            (x.XFlush)(display);
            Some(Window { x, display, win, wm_delete })
        }
    }
    pub fn id(&self) -> u64 {
        self.win as u64
    }
    pub fn fd(&self) -> i32 {
        unsafe { (self.x.XConnectionNumber)(self.display) }
    }
    pub fn resize(&self, w: u32, h: u32) {
        unsafe {
            (self.x.XResizeWindow)(self.display, self.win, w.max(64), h.max(48));
            (self.x.XFlush)(self.display);
        }
    }
    /// Drains pending events; true if the user closed the window.
    pub fn poll_closed(&self) -> bool {
        let mut closed = false;
        unsafe {
            while (self.x.XPending)(self.display) > 0 {
                let mut ev: xlib::XEvent = std::mem::zeroed();
                (self.x.XNextEvent)(self.display, &mut ev);
                if ev.get_type() == xlib::ClientMessage && ev.client_message.data.get_long(0) as xlib::Atom == self.wm_delete {
                    closed = true;
                }
            }
        }
        closed
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            (self.x.XDestroyWindow)(self.display, self.win);
            (self.x.XCloseDisplay)(self.display);
        }
    }
}
