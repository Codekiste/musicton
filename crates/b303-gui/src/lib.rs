//! The B-303 editor (Roland-Cloud-style front panel), shared by Musicton's plug-in window and
//! the VST3/CLAP plug-in. Hosts connect it to their state through the [`Host`] trait.

use b303_core::{p, random_pattern, SeqPattern, SeqStep, SPECS};
use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke};
use musicton_dsp::{gain_to_db, Rng};
use std::f32::consts::PI;

/// Design size of the editor in points (the editor scales to the space it gets).
pub const W: f32 = 1100.0;
pub const H: f32 = 475.0;
pub const PANEL_H: f32 = 92.0;

const TEXT: Color32 = Color32::from_rgb(0xd2, 0xd2, 0xd2);
const TEXT_DIM: Color32 = Color32::from_rgb(0x8c, 0x8c, 0x8c);
const BLUE: Color32 = Color32::from_rgb(0x5d, 0xb8, 0xff);
const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

/// What the editor needs from its surroundings.
pub trait Host {
    fn param(&self, idx: usize) -> f32;
    fn set_param(&mut self, idx: usize, v: f32);
    /// The currently selected internal pattern.
    fn pattern(&self) -> SeqPattern;
    fn set_pattern(&mut self, p: SeqPattern);
    /// [current step, running (0/1), pattern index]
    fn display(&self) -> [f32; 3];
    /// Output peak level (linear) for the meter.
    fn level(&self) -> f32;
    fn note_on(&mut self, pitch: u8, vel: u8);
    fn note_off(&mut self, pitch: u8);
    /// A short preview note (step entry). Hosts that can't schedule just play note on/off.
    fn audition(&mut self, pitch: u8, vel: u8) {
        self.note_on(pitch, vel);
    }
    /// Lowest pitch of the keyboard (C of the bass octave, shifted to the song key).
    fn root(&self) -> i32 {
        45
    }
    fn scale(&self) -> &'static [u8] {
        &MINOR
    }
    fn seed(&self) -> u32;
    fn hint(&mut self, _title: &str, _body: &str) {}
    fn toggle_help(&mut self) {}
    /// Extra right-click entries for a knob (e.g. MIDI learn).
    fn knob_menu(&mut self, _ui: &mut egui::Ui, _idx: usize) {}
    /// Extra entries for the OPTION menu.
    fn option_menu(&mut self, _ui: &mut egui::Ui) {}
    /// The pattern WRITE button.
    fn write_pattern(&mut self) {}
}

fn hint(host: &mut dyn Host, r: &egui::Response, title: &str, body: &str) {
    if r.hovered() {
        host.hint(title, body);
    }
}

fn to_norm(sp: &b303_core::Spec, v: f32) -> f32 {
    ((v - sp.min) / (sp.max - sp.min)).clamp(0.0, 1.0)
}

fn from_norm(sp: &b303_core::Spec, n: f32) -> f32 {
    let v = sp.min + n.clamp(0.0, 1.0) * (sp.max - sp.min);
    if sp.step > 0.0 {
        ((v - sp.min) / sp.step).round() * sp.step + sp.min
    } else {
        v
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum KeyMode {
    #[default]
    Pattern,
    Keyboard,
    Edit,
}

#[derive(Clone, Default)]
pub struct EditorState {
    pub mode: KeyMode,
    pub sel_step: usize,
    pub panel: bool,
    pub accent_live: bool,
    pub octave: i32,
    pub held_key: Option<u8>,
    pub undo_random: Option<SeqPattern>,
    pub undo_modify: Option<SeqPattern>,
    pub about: bool,
    pub patch: usize,
}

impl EditorState {
    pub fn new() -> Self {
        Self { panel: true, ..Default::default() }
    }
}

const SILVER: Color32 = Color32::from_rgb(0xcf, 0xd1, 0xd4);
const SILVER_LIGHT: Color32 = Color32::from_rgb(0xdc, 0xde, 0xe1);
const SILVER_DARK: Color32 = Color32::from_rgb(0xa8, 0xaa, 0xad);
const INK: Color32 = Color32::from_rgb(0x16, 0x16, 0x18);
const BAND: Color32 = Color32::from_rgb(0x1b, 0x1b, 0x1d);
const ORANGE: Color32 = Color32::from_rgb(0xf0, 0x7a, 0x1a);
const LCD: Color32 = Color32::from_rgb(0xd8, 0xda, 0xdc);

/// Maps the 1100-point design space of the editor onto the window.
struct Canvas {
    p: egui::Painter,
    o: Pos2,
    s: f32,
}

impl Canvas {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.o.x + x * self.s, self.o.y + y * self.s)
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.pt(x, y), vec2(w * self.s, h * self.s))
    }
    fn text(&self, x: f32, y: f32, a: Align2, t: impl ToString, size: f32, c: Color32) {
        self.p.text(self.pt(x, y), a, t.to_string(), FontId::proportional(size * self.s), c);
    }
    fn fill(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color32) {
        self.p.rect_filled(self.rect(x, y, w, h), r * self.s, c);
    }
    fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, w: f32, c: Color32) {
        self.p.line_segment([self.pt(x1, y1), self.pt(x2, y2)], Stroke::new(w * self.s, c));
    }
}

fn led(c: &Canvas, x: f32, y: f32, on: bool) {
    let p = c.pt(x, y);
    if on {
        c.p.circle_filled(p, 7.0 * c.s, Color32::from_rgba_unmultiplied(255, 40, 30, 60));
    }
    c.p.circle_filled(p, 4.0 * c.s, if on { Color32::from_rgb(0xff, 0x2a, 0x1e) } else { Color32::from_rgb(0x55, 0x10, 0x10) });
    c.p.circle_stroke(p, 4.0 * c.s, Stroke::new(0.8_f32 * c.s, Color32::from_rgb(0x22, 0x08, 0x08)));
}

/// White key-cap style button.
fn cap(c: &Canvas, ui: &mut egui::Ui, id: impl std::hash::Hash, x: f32, y: f32, w: f32, h: f32, pressed: bool) -> egui::Response {
    let r = c.rect(x, y, w, h);
    let resp = ui.interact(r, ui.id().with(id), Sense::click_and_drag());
    let down = pressed || resp.is_pointer_button_down_on();
    c.p.rect_filled(r.translate(vec2(1.5, 2.0) * c.s), 2.0 * c.s, Color32::from_black_alpha(90));
    let top = if down { Color32::from_rgb(0xc6, 0xc8, 0xcb) } else if resp.hovered() { Color32::from_rgb(0xfb, 0xfb, 0xfc) } else { Color32::from_rgb(0xf2, 0xf3, 0xf4) };
    c.p.rect_filled(r, 2.0 * c.s, top);
    let inner = r.shrink(3.0 * c.s);
    c.p.rect_filled(inner, 1.5 * c.s, if down { Color32::from_rgb(0xbd, 0xbf, 0xc2) } else { Color32::from_rgb(0xe6, 0xe7, 0xe9) });
    c.p.rect_stroke(r, 2.0 * c.s, Stroke::new(1.0_f32 * c.s, Color32::from_rgb(0x55, 0x57, 0x5a)), egui::StrokeKind::Inside);
    resp
}

/// Chrome knob with black skirt and tick marks.
fn chrome_knob(c: &Canvas, ui: &mut egui::Ui, id: impl std::hash::Hash, cx: f32, cy: f32, r: f32, norm: &mut f32, ticks: bool) -> egui::Response {
    let rect = Rect::from_center_size(c.pt(cx, cy), vec2(r * 2.4, r * 2.4) * c.s);
    let resp = ui.interact(rect, ui.id().with(id), Sense::click_and_drag());
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let d = resp.drag_delta();
        *norm = (*norm + (d.x - d.y) / if fine { 900.0 } else { 180.0 }).clamp(0.0, 1.0);
    }
    if resp.hovered() {
        let sc = ui.input(|i| i.raw_scroll_delta.y);
        if sc != 0.0 {
            *norm = (*norm + sc.signum() * 0.02).clamp(0.0, 1.0);
        }
    }
    let ctr = c.pt(cx, cy);
    let rr = r * c.s;
    if ticks {
        for k in 0..=10 {
            let a = 1.25 * PI - 1.5 * PI * k as f32 / 10.0;
            let (s, co) = a.sin_cos();
            c.p.line_segment([ctr + vec2(co, -s) * rr * 1.12, ctr + vec2(co, -s) * rr * 1.32], Stroke::new(1.3_f32 * c.s, INK));
        }
    }
    c.p.circle_filled(ctr + vec2(1.5, 2.5) * c.s, rr, Color32::from_black_alpha(80));
    c.p.circle_filled(ctr, rr, Color32::from_rgb(0x20, 0x20, 0x22));
    // Chrome cap with a soft highlight.
    for (k, col) in [(0.86, 0xb4u8), (0.80, 0xc8), (0.70, 0xdb), (0.55, 0xea), (0.35, 0xf5)] {
        c.p.circle_filled(ctr - vec2(0.0, rr * (0.86 - k) * 0.25), rr * k, Color32::from_gray(col));
    }
    let a = 1.25 * PI - 1.5 * PI * *norm;
    let (s, co) = a.sin_cos();
    c.p.line_segment([ctr + vec2(co, -s) * rr * 0.15, ctr + vec2(co, -s) * rr * 0.82], Stroke::new(2.4_f32 * c.s, INK));
    c.p.line_segment([ctr + vec2(co, -s) * rr * 0.86, ctr + vec2(co, -s) * rr], Stroke::new(3.0_f32 * c.s, Color32::from_gray(0xee)));
    resp
}

fn tri(c: &Canvas, x: f32, y: f32, size: f32, up: bool, col: Color32) {
    let h = size * 0.5;
    let pts = if up {
        vec![c.pt(x - h, y + h * 0.6), c.pt(x + h, y + h * 0.6), c.pt(x, y - h * 0.6)]
    } else {
        vec![c.pt(x - h, y - h * 0.6), c.pt(x + h, y - h * 0.6), c.pt(x, y + h * 0.6)]
    };
    c.p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
}

fn lcd(c: &Canvas, x: f32, y: f32, w: f32, h: f32, text: &str, size: f32) {
    c.fill(x, y, w, h, 2.0, Color32::from_rgb(0x2a, 0x2a, 0x2c));
    c.fill(x + 2.0, y + 2.0, w - 4.0, h - 4.0, 1.5, LCD);
    c.text(x + 8.0, y + h / 2.0, Align2::LEFT_CENTER, text, size, INK);
}

fn small_btn(c: &Canvas, ui: &mut egui::Ui, id: impl std::hash::Hash, x: f32, y: f32, w: f32, h: f32, label: &str, label_above: bool) -> egui::Response {
    if label_above {
        c.text(x + w / 2.0, y - 3.0, Align2::CENTER_BOTTOM, label, 10.0, INK);
    }
    cap(c, ui, id, x, y, w, h, false)
}


pub fn editor(ui: &mut egui::Ui, host: &mut dyn Host, st: &mut EditorState) {
    let h = H + if st.panel { PANEL_H } else { 0.0 };
    let avail = ui.available_rect_before_wrap();
    let s = (avail.width() / W).min(avail.height() / h).max(0.2);
    let origin = pos2(avail.center().x - W * s / 2.0, avail.top());
    ui.allocate_rect(avail, Sense::hover());
    let c = Canvas { p: ui.painter().clone(), o: origin, s };
    let disp = host.display().to_vec();
    let (play_step, running) = (disp.first().copied().unwrap_or(0.0) as usize, disp.get(1).copied().unwrap_or(0.0) > 0.5);
    let mut pat = host.pattern();
    let mut pat_changed = false;

    // Body
    c.fill(0.0, 0.0, W, h, 6.0, SILVER);
    c.fill(4.0, 4.0, W - 8.0, 92.0, 3.0, SILVER_LIGHT);
    c.line(4.0, 97.0, W - 4.0, 97.0, 2.0, SILVER_DARK);
    c.fill(4.0, 99.0, W - 8.0, 136.0, 3.0, SILVER_LIGHT);
    c.line(4.0, 237.0, W - 4.0, 237.0, 2.0, SILVER_DARK);

    // --- Top row
    let saw = host.param(p::WAVE) < 0.5;
    c.fill(105.0, 10.0, 46.0, 14.0, 2.0, BAND);
    c.fill(if saw { 107.0 } else { 129.0 }, 12.0, 20.0, 10.0, 1.5, Color32::from_gray(0xe8));
    let wr = ui.interact(c.rect(105.0, 8.0, 46.0, 18.0), ui.id().with("wave"), Sense::click());
    if wr.clicked() {
        host.set_param(p::WAVE, if saw { 1.0 } else { 0.0 });
    }
    hint(host, &wr, "Waveform", "Sawtooth (left) or square (right).");
    // Saw and square icons.
    let sw = 1.4;
    c.line(108.0, 40.0, 116.0, 32.0, sw, INK);
    c.line(116.0, 32.0, 116.0, 40.0, sw, INK);
    c.line(116.0, 40.0, 124.0, 32.0, sw, INK);
    c.line(124.0, 32.0, 124.0, 40.0, sw, INK);
    for (x1, y1, x2, y2) in [(132.0, 40.0, 132.0, 32.0), (132.0, 32.0, 138.0, 32.0), (138.0, 32.0, 138.0, 40.0), (138.0, 40.0, 144.0, 40.0), (144.0, 40.0, 144.0, 32.0), (144.0, 32.0, 150.0, 32.0)] {
        c.line(x1, y1, x2, y2, sw, INK);
    }
    c.text(128.0, 44.0, Align2::CENTER_TOP, "WAVEFORM", 10.5, INK);
    // Logo
    c.fill(62.0, 60.0, 30.0, 26.0, 3.0, INK);
    c.text(77.0, 73.0, Align2::CENTER_CENTER, "M", 20.0, SILVER_LIGHT);
    c.text(98.0, 73.0, Align2::LEFT_CENTER, "Musicton", 30.0, INK);
    c.line(303.0, 12.0, 303.0, 88.0, 1.5, INK);
    let big = [(p::TUNING, "TUNING", 347.0), (p::CUTOFF, "CUT OFF FREQ", 417.0), (p::RES, "RESONANCE", 487.0), (p::ENV_MOD, "ENV MOD", 560.0), (p::DECAY, "DECAY", 630.0), (p::ACCENT, "ACCENT", 700.0)];
    for (idx, label, x) in big {
        c.text(x, 12.0, Align2::CENTER_TOP, label, 10.5, INK);
        knob(&c, ui, host, idx, x, 56.0, 23.0);
    }
    let small = [(p::VCF_TRIM, "VCF TRIM", 760.0, 58.0, 36.0), (p::MASTER_TUNE, "MASTER TUNE", 814.0, 36.0, 15.0), (p::CONDITION, "CONDITION", 814.0, 78.0, 57.0)];
    for (idx, label, x, y, ly) in small {
        c.text(x, ly, Align2::CENTER_TOP, label, 9.0, INK);
        knob(&c, ui, host, idx, x, y, 10.0);
    }
    c.line(850.0, 12.0, 850.0, 88.0, 1.5, INK);
    let fx_row = [
        (p::DRIVE_TYPE, "DRIVE TYPE", 875.0, 36.0, 15.0),
        (p::DRIVE_TONE, "TONE", 960.0, 36.0, 15.0),
        (p::DRIVE, "DEPTH", 1005.0, 36.0, 15.0),
        (p::DELAY_TYPE, "DELAY TYPE", 875.0, 78.0, 57.0),
        (p::DELAY_TIME, "TIME", 960.0, 78.0, 57.0),
        (p::DELAY_LEVEL, "LEVEL", 1005.0, 78.0, 57.0),
    ];
    for (idx, label, x, y, ly) in fx_row {
        c.text(x + if label.ends_with("TYPE") { 18.0 } else { 0.0 }, ly, Align2::CENTER_TOP, label, 9.0, INK);
        knob(&c, ui, host, idx, x, y, 10.0);
        if label.ends_with("TYPE") {
            let v = host.param(idx);
            lcd(&c, x + 14.0, y - 7.0, 40.0, 14.0, SPECS[idx].options.get(v.round() as usize).unwrap_or(&""), 8.5);
        }
    }
    let sync = host.param(p::TEMPO_SYNC) > 0.5;
    c.text(1060.0, 50.0, Align2::CENTER_TOP, "TEMPO SYNC", 9.0, INK);
    c.fill(1040.0, 64.0, 40.0, 12.0, 2.0, BAND);
    c.fill(if sync { 1060.0 } else { 1042.0 }, 66.0, 18.0, 8.0, 1.0, Color32::from_gray(0xe8));
    c.text(1043.0, 80.0, Align2::LEFT_TOP, "OFF", 8.0, INK);
    c.text(1079.0, 80.0, Align2::RIGHT_TOP, "ON", 8.0, INK);
    let sr = ui.interact(c.rect(1038.0, 60.0, 44.0, 20.0), ui.id().with("sync"), Sense::click());
    if sr.clicked() {
        host.set_param(p::TEMPO_SYNC, if sync { 0.0 } else { 1.0 });
    }
    hint(host, &sr, "Tempo Sync", "Delay time in note values (ON) or milliseconds (OFF).");
    // PANEL toggle
    c.fill(1032.0, 10.0, 56.0, 14.0, 1.0, SILVER_DARK);
    c.text(1060.0, 17.0, Align2::CENTER_CENTER, "PANEL", 8.5, INK);
    tri(&c, 1040.0, 17.0, 6.0, st.panel, INK);
    tri(&c, 1080.0, 17.0, 6.0, st.panel, INK);
    let pr = ui.interact(c.rect(1032.0, 10.0, 56.0, 14.0), ui.id().with("panel"), Sense::click());
    if pr.clicked() {
        st.panel = !st.panel;
    }
    hint(host, &pr, "Panel", "Shows or hides the step panel with the 16 steps of the pattern.");

    // --- Middle row
    let tops = [("OPTION", 30.0), ("HELP", 74.0), ("ABOUT", 117.0)];
    for (label, x) in tops {
        let r = small_btn(&c, ui, ("top", label), x, 122.0, 38.0, 10.0, label, true);
        if r.clicked() {
            match label {
                "HELP" => host.toggle_help(),
                "ABOUT" => st.about = !st.about,
                _ => {}
            }
        }
        if label == "OPTION" {
            r.context_menu(|ui| {
                host.option_menu(ui);
                if ui.button("Clear pattern").clicked() {
                    for s in &mut pat.steps {
                        s.time = 2;
                        s.accent = false;
                        s.slide = false;
                    }
                    pat_changed = true;
                    ui.close_menu();
                }
            });
            hint(host, &r, "Option", "Right-click for pattern options (clear, and host-specific actions).");
        }
    }
    let cur_pat = host.param(p::PATTERN).round() as usize;
    for k in 0..8 {
        let (x, y) = (30.0 + (k % 4) as f32 * 43.5, 165.0 + (k / 4) as f32 * 24.0);
        let r = cap(&c, ui, ("pat", k), x, y, 38.0, 18.0, cur_pat == k);
        c.text(x + 19.0, y + 9.0, Align2::CENTER_CENTER, k + 1, 11.0, if cur_pat == k { Color32::from_rgb(0xe0, 0x20, 0x20) } else { INK });
        if r.clicked() {
            host.set_param(p::PATTERN, k as f32);
        }
        hint(host, &r, "Pattern", "Selects one of the 8 internal patterns.");
    }
    c.text(246.0, 112.0, Align2::CENTER_TOP, "SHUFFLE", 10.5, INK);
    knob(&c, ui, host, p::SHUFFLE, 246.0, 172.0, 28.0);
    c.text(337.0, 112.0, Align2::CENTER_TOP, "SCALE", 10.5, INK);
    for (k, label) in b303_core::spec::SCALES.iter().enumerate() {
        let a = 2.2 - k as f32 * 0.5;
        c.text(337.0 + a.cos() * 40.0, 172.0 - a.sin() * 40.0, Align2::CENTER_CENTER, label, 8.0, INK);
    }
    knob(&c, ui, host, p::SCALE, 337.0, 172.0, 22.0);
    c.text(426.0, 112.0, Align2::CENTER_TOP, "PLAY MODE", 10.5, INK);
    knob(&c, ui, host, p::PLAY_MODE, 426.0, 172.0, 26.0);
    let pm = host.param(p::PLAY_MODE).round() as usize;
    for (k, label) in b303_core::spec::PLAY_MODES.iter().enumerate() {
        let y = 136.0 + k as f32 * 18.0;
        if k == pm {
            c.fill(456.0, y - 7.0, 62.0, 14.0, 1.0, INK);
        }
        c.text(460.0, y, Align2::LEFT_CENTER, label.to_uppercase(), 8.5, if k == pm { Color32::WHITE } else { INK });
        let r = ui.interact(c.rect(456.0, y - 7.0, 62.0, 14.0), ui.id().with(("pm", k)), Sense::click());
        if r.clicked() {
            host.set_param(p::PLAY_MODE, k as f32);
        }
    }
    // Pattern / patch displays
    c.text(540.0, 112.0, Align2::LEFT_TOP, "LIST", 10.0, INK);
    let list = cap(&c, ui, "list", 532.0, 124.0, 40.0, 9.0, false);
    list.context_menu(|ui| {
        for k in 0..8 {
            if ui.selectable_label(cur_pat == k, format!("Pattern {}", k + 1)).clicked() {
                host.set_param(p::PATTERN, k as f32);
                ui.close_menu();
            }
        }
    });
    hint(host, &list, "List", "Right-click to pick a pattern from the list.");
    c.text(532.0, 142.0, Align2::LEFT_TOP, "PATTERN", 10.0, INK);
    lcd(&c, 532.0, 154.0, 168.0, 24.0, &format!("Pattern {}", cur_pat + 1), 14.0);
    for (k, d) in [1i32, -1].iter().enumerate() {
        let r = cap(&c, ui, ("patarrow", k), 700.0, 154.0 + k as f32 * 12.0, 12.0, 12.0, false);
        tri(&c, 706.0, 160.0 + k as f32 * 12.0, 5.0, k == 0, INK);
        if r.clicked() {
            host.set_param(p::PATTERN, (cur_pat as i32 + d).rem_euclid(8) as f32);
        }
    }
    let w1 = small_btn(&c, ui, "write1", 720.0, 156.0, 34.0, 9.0, "", false);
    c.text(737.0, 170.0, Align2::CENTER_TOP, "WRITE", 9.0, INK);
    if w1.clicked() {
        host.write_pattern();
    }
    hint(host, &w1, "Write Pattern", "Writes the current pattern into the next empty clip slot of this track.");
    c.text(532.0, 182.0, Align2::LEFT_TOP, "PATCH", 10.0, INK);
    let presets = b303_core::spec::PRESETS;
    let patch_idx = st.patch.min(presets.len() - 1);
    lcd(&c, 532.0, 194.0, 168.0, 24.0, presets[patch_idx].0, 14.0);
    for (k, d) in [-1i32, 1].iter().enumerate() {
        let r = cap(&c, ui, ("patcharrow", k), 700.0, 194.0 + k as f32 * 12.0, 12.0, 12.0, false);
        tri(&c, 706.0, 200.0 + k as f32 * 12.0, 5.0, k == 0, INK);
        if r.clicked() {
            let ni = (patch_idx as i32 + d).rem_euclid(presets.len() as i32) as usize;
            st.patch = ni;
            // Load the sound, keep the sequencer settings.
            let vals = b303_core::spec::preset_params(ni);
            for i in p::SOUND {
                host.set_param(i, vals[i]);
            }
        }
        hint(host, &r, "Patch", presets[patch_idx].1);
    }
    let w2 = small_btn(&c, ui, "write2", 720.0, 196.0, 34.0, 9.0, "", false);
    c.text(737.0, 210.0, Align2::CENTER_TOP, "INIT", 9.0, INK);
    if w2.clicked() {
        let init = b303_core::spec::defaults();
        for i in p::SOUND {
            host.set_param(i, init[i]);
        }
    }
    hint(host, &w2, "Init Patch", "Resets the sound to the initial patch (the patterns are kept).");
    // Output meter under the patch display.
    let m = [host.level(); 2];
    let lvl = ((gain_to_db(m[0].max(m[1])) + 48.0) / 48.0).clamp(0.0, 1.0);
    c.fill(532.0, 224.0, 222.0, 6.0, 1.0, Color32::from_gray(0xf4));
    c.fill(532.0, 224.0, 222.0 * lvl, 6.0, 1.0, Color32::from_rgb(0xd0, 0x30, 0x30));
    c.text(990.0, 150.0, Align2::RIGHT_CENTER, "B-303", 34.0, INK);
    c.line(770.0, 168.0, 990.0, 168.0, 1.0, INK);
    c.text(990.0, 186.0, Align2::RIGHT_CENTER, "Computer Controlled", 21.0, INK);
    c.text(1040.0, 112.0, Align2::CENTER_TOP, "VOLUME", 10.5, INK);
    knob(&c, ui, host, p::VOLUME, 1040.0, 172.0, 28.0);

    // --- Bottom: transport, keyboard and step buttons
    c.fill(12.0, 243.0, W - 24.0, 222.0, 3.0, Color32::from_gray(0xc0));
    let boxes = [(16.0, 247.0, 104.0, 72.0), (16.0, 323.0, 104.0, 138.0), (124.0, 247.0, 104.0, 72.0), (124.0, 323.0, 104.0, 138.0), (960.0, 247.0, 124.0, 72.0), (960.0, 323.0, 124.0, 138.0)];
    for (x, y, w, hh) in boxes {
        c.fill(x, y, w, hh, 2.0, SILVER_LIGHT);
    }
    c.fill(124.0, 247.0, 104.0, 72.0, 2.0, BAND);
    // Position lock
    let lock = host.param(p::POS_LOCK) > 0.5;
    c.text(68.0, 252.0, Align2::CENTER_TOP, "POSITION LOCK TO DAW", 8.5, INK);
    led(&c, 68.0, 274.0, lock);
    let r = cap(&c, ui, "lock", 46.0, 290.0, 44.0, 20.0, false);
    if r.clicked() {
        host.set_param(p::POS_LOCK, if lock { 0.0 } else { 1.0 });
    }
    hint(host, &r, "Position Lock to DAW", "ON: the pattern runs with Musicton's transport and stays in sync with the song position. OFF: Run starts the pattern immediately, even when Musicton is stopped.");
    // Run / stop
    let run = host.param(p::RUN) > 0.5;
    let blink = running && play_step % 4 < 2;
    led(&c, 68.0, 336.0, run && (blink || !running));
    let r = cap(&c, ui, "run", 36.0, 356.0, 64.0, 58.0, run);
    c.text(68.0, 432.0, Align2::CENTER_TOP, "RUN / STOP", 10.0, INK);
    if r.clicked() {
        host.set_param(p::RUN, if run { 0.0 } else { 1.0 });
    }
    hint(host, &r, "Run / Stop", "Starts or stops the internal sequencer. With Position Lock it plays while Musicton plays.");
    // Keyboard / edit modes
    c.text(176.0, 252.0, Align2::CENTER_TOP, "KEYBOARD", 10.0, Color32::WHITE);
    led(&c, 176.0, 274.0, st.mode == KeyMode::Keyboard);
    let r = cap(&c, ui, "kbd", 154.0, 290.0, 44.0, 20.0, false);
    if r.clicked() {
        st.mode = if st.mode == KeyMode::Keyboard { KeyMode::Pattern } else { KeyMode::Keyboard };
    }
    hint(host, &r, "Keyboard", "Keyboard mode: the keys play notes (ACCENT, DOWN/UP apply). Off: the white keys select patterns 1-8.");
    c.text(160.0, 336.0, Align2::CENTER_CENTER, "EDIT", 10.0, INK);
    led(&c, 200.0, 336.0, st.mode == KeyMode::Edit);
    let r = cap(&c, ui, "edit", 148.0, 356.0, 32.0, 58.0, st.mode == KeyMode::Edit);
    if r.clicked() {
        st.mode = if st.mode == KeyMode::Edit { KeyMode::Pattern } else { KeyMode::Edit };
    }
    hint(host, &r, "Edit", "Step edit: select a step in the step panel, then press a key to enter its pitch (the cursor advances). ACCENT / SLIDE / DOWN / UP change the selected step.");
    // Keyboard
    c.fill(232.0, 247.0, 492.0, 18.0, 0.0, BAND);
    c.fill(232.0, 412.0, 492.0, 16.0, 0.0, BAND);
    c.text(250.0, 420.0, Align2::LEFT_CENTER, "PATTERN", 9.0, ORANGE);
    let whites = [(0u8, 262.0), (2, 321.0), (4, 380.0), (5, 439.0), (7, 498.0), (9, 557.0), (11, 616.0), (12, 675.0)];
    let blacks = [(1u8, 291.0), (3, 350.0), (6, 468.0), (8, 527.0), (10, 586.0)];
    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B", "C"];
    let sel = st.sel_step.min(pat.steps.len().saturating_sub(1));
    let sel_pitch = pat.steps.get(sel).map(|s| s.pitch).unwrap_or(45);
    let base = host.root();
    let lit = |semi: u8| -> bool {
        match st.mode {
            KeyMode::Edit => pat.steps.get(sel).is_some_and(|s| s.time != 2 && ((s.pitch as i32 - base).rem_euclid(12) as u8 == semi % 12) && (semi < 12 || s.pitch as i32 - base >= 12)),
            KeyMode::Keyboard => st.held_key == Some(semi),
            KeyMode::Pattern => false,
        }
    };
    let mut pressed: Option<u8> = None;
    let mut released = false;
    for (semi, x) in blacks {
        c.fill(x - 4.0, 266.0, 50.0, 84.0, 0.0, BAND);
        c.text(x + 21.0, 256.0, Align2::CENTER_CENTER, names[semi as usize], 9.5, Color32::WHITE);
        led(&c, x + 21.0, 276.0, lit(semi));
        let r = cap(&c, ui, ("key", semi), x + 10.0, 288.0, 22.0, 44.0, st.held_key == Some(semi));
        if r.drag_started() || r.clicked() {
            pressed = Some(semi);
        }
        if r.drag_stopped() {
            released = true;
        }
    }
    for (k, (semi, x)) in whites.iter().enumerate() {
        c.text(x + 21.0, 256.0, Align2::CENTER_CENTER, names[*semi as usize], 9.5, Color32::WHITE);
        led(&c, x + 21.0, 344.0, lit(*semi) || (st.mode == KeyMode::Pattern && cur_pat == k));
        let r = cap(&c, ui, ("key", semi), x + 8.0, 356.0, 26.0, 50.0, st.held_key == Some(*semi));
        c.text(x + 21.0, 420.0, Align2::CENTER_CENTER, k + 1, 10.0, ORANGE);
        if r.drag_started() || r.clicked() {
            pressed = Some(*semi);
        }
        if r.drag_stopped() {
            released = true;
        }
    }
    // Accent / slide / down / up
    c.fill(728.0, 247.0, 228.0, 72.0, 0.0, BAND);
    c.fill(840.0, 412.0, 116.0, 16.0, 0.0, BAND);
    c.text(898.0, 420.0, Align2::CENTER_CENTER, "OCTAVE", 9.5, ORANGE);
    let step = pat.steps.get(sel).copied();
    let flags = [
        ("ACCENT", 755.0, if st.mode == KeyMode::Keyboard { st.accent_live } else { step.is_some_and(|s| s.accent) }),
        ("SLIDE", 812.0, step.is_some_and(|s| s.slide) && st.mode == KeyMode::Edit),
        ("DOWN", 869.0, if st.mode == KeyMode::Keyboard { st.octave < 0 } else { step.is_some_and(|s| (s.pitch as i32) < base) }),
        ("UP", 926.0, if st.mode == KeyMode::Keyboard { st.octave > 0 } else { step.is_some_and(|s| s.pitch as i32 >= base + 12) }),
    ];
    for (label, x, on) in flags {
        c.text(x, 262.0, Align2::CENTER_CENTER, match label { "ACCENT" | "SLIDE" => "○ ♪", "DOWN" => "● ♪", _ => "○ ♫" }, 11.0, Color32::WHITE);
        c.text(x, 304.0, Align2::CENTER_CENTER, label, 10.0, Color32::WHITE);
        led(&c, x, 344.0, on);
        let r = cap(&c, ui, ("flag", label), x - 13.0, 356.0, 26.0, 50.0, false);
        if r.clicked() {
            match (st.mode, label) {
                (KeyMode::Keyboard, "ACCENT") => st.accent_live = !st.accent_live,
                (KeyMode::Keyboard, "DOWN") => st.octave = (st.octave - 1).max(-2),
                (KeyMode::Keyboard, "UP") => st.octave = (st.octave + 1).min(2),
                (_, _) => {
                    if let Some(s) = pat.steps.get_mut(sel) {
                        match label {
                            "ACCENT" => s.accent = !s.accent,
                            "SLIDE" => s.slide = !s.slide,
                            "DOWN" => s.pitch = (s.pitch as i32 - 12).max(12) as u8,
                            _ => s.pitch = (s.pitch as i32 + 12).min(96) as u8,
                        }
                        if s.time == 2 {
                            s.time = 0;
                        }
                        pat_changed = true;
                    }
                }
            }
        }
        let info = match label {
            "ACCENT" => "Accent for the selected step (Keyboard mode: accent the notes you play).",
            "SLIDE" => "Slide from the selected step into the next one.",
            "DOWN" => "Lowers the selected step by an octave (Keyboard mode: keyboard octave down).",
            _ => "Raises the selected step by an octave (Keyboard mode: keyboard octave up).",
        };
        hint(host, &r, label, info);
    }
    // Randomize / modify
    c.text(1022.0, 254.0, Align2::CENTER_TOP, "RANDOMIZE", 10.0, INK);
    c.p.rect_stroke(c.rect(980.0, 252.0, 84.0, 14.0), 0.0, Stroke::new(1.0_f32 * s, INK), egui::StrokeKind::Inside);
    let r = cap(&c, ui, "rand", 1000.0, 272.0, 44.0, 20.0, false);
    c.text(1022.0, 302.0, Align2::CENTER_TOP, "GENERATE / UNDO", 9.0, INK);
    if r.clicked() {
        match st.undo_random.take() {
            Some(prev) => pat = prev,
            None => {
                st.undo_random = Some(pat.clone());
                let scale = host.scale();
                let seed = host.seed();
                pat.steps = random_pattern(16, base as u8, scale, seed).into_iter().map(|s| SeqStep { pitch: s.pitch, accent: s.accent, slide: s.slide, time: s.time as u8 }).collect();
            }
        }
        pat_changed = true;
    }
    hint(host, &r, "Randomize", "Generates a new pattern in the project key and scale. Press again to undo.");
    let r = cap(&c, ui, "modify", 990.0, 350.0, 64.0, 58.0, false);
    c.text(1022.0, 432.0, Align2::CENTER_TOP, "MODIFY / UNDO", 10.0, INK);
    if r.clicked() {
        match st.undo_modify.take() {
            Some(prev) => pat = prev,
            None => {
                st.undo_modify = Some(pat.clone());
                let mut rng = Rng::new(host.seed() | 1);
                for s in &mut pat.steps {
                    let roll = rng.next() * 0.5 + 0.5;
                    if roll < 0.15 {
                        s.accent = !s.accent;
                    } else if roll < 0.27 {
                        s.slide = !s.slide;
                    } else if roll < 0.37 {
                        s.pitch = (s.pitch as i32 + [12, -12, 7, -5, 3][(rng.next().abs() * 5.0) as usize % 5]).clamp(24, 84) as u8;
                    }
                }
            }
        }
        pat_changed = true;
    }
    hint(host, &r, "Modify", "Varies the current pattern a little (accents, slides, octaves). Press again to undo.");

    // --- Step panel
    if st.panel {
        let y0 = H + 2.0;
        c.fill(12.0, y0, W - 24.0, PANEL_H - 10.0, 3.0, BAND);
        let len = pat.len.max(1) as usize;
        let cw = 56.0;
        for i in 0..16usize {
            let x = 24.0 + i as f32 * cw;
            let Some(sp) = pat.steps.get(i).copied() else { continue };
            let active = i < len;
            let r = ui.interact(c.rect(x, y0 + 8.0, cw - 6.0, 58.0), ui.id().with(("step", i)), Sense::click());
            let bg = if i == sel { Color32::from_rgb(0x4a, 0x4a, 0x50) } else if active { Color32::from_rgb(0x2c, 0x2c, 0x30) } else { Color32::from_rgb(0x22, 0x22, 0x24) };
            c.fill(x, y0 + 8.0, cw - 6.0, 58.0, 2.0, bg);
            if i == sel {
                c.p.rect_stroke(c.rect(x, y0 + 8.0, cw - 6.0, 58.0), 2.0 * s, Stroke::new(1.5_f32 * s, ORANGE), egui::StrokeKind::Inside);
            }
            led(&c, x + 8.0, y0 + 15.0, running && play_step == i);
            c.text(x + cw - 10.0, y0 + 15.0, Align2::RIGHT_CENTER, i + 1, 8.5, TEXT_DIM);
            let col = if active { Color32::WHITE } else { TEXT_DIM };
            let label = match sp.time {
                0 => format!("{}{}", NOTE_NAMES[(sp.pitch % 12) as usize], sp.pitch as i32 / 12 - 2),
                1 => "TIE".into(),
                _ => "·".into(),
            };
            c.text(x + (cw - 6.0) / 2.0, y0 + 36.0, Align2::CENTER_CENTER, label, 12.0, col);
            if sp.accent && sp.time == 0 {
                c.fill(x + 4.0, y0 + 52.0, 18.0, 8.0, 1.0, ORANGE);
            }
            if sp.slide && sp.time == 0 {
                c.fill(x + cw - 28.0, y0 + 52.0, 18.0, 8.0, 1.0, BLUE);
            }
            if r.clicked() {
                st.sel_step = i;
            }
            if r.secondary_clicked() {
                if let Some(s) = pat.steps.get_mut(i) {
                    s.time = (s.time + 1) % 3;
                    pat_changed = true;
                }
            }
            hint(host, &r, "Step", "Click to select the step (for Edit mode). Right-click cycles note / tie / rest. Orange = accent, blue = slide.");
        }
        let bx = 24.0 + 16.0 * cw + 4.0;
        let _ = bx;
        let ctl_y = y0 + 70.0;
        for (k, (label, val)) in [("NOTE", 0u8), ("TIE", 1), ("REST", 2)].iter().enumerate() {
            let x = 24.0 + k as f32 * 60.0;
            let r = cap(&c, ui, ("time", label), x, ctl_y, 52.0, 12.0, false);
            c.text(x + 26.0, ctl_y + 6.0, Align2::CENTER_CENTER, label, 8.5, INK);
            if r.clicked() {
                if let Some(s) = pat.steps.get_mut(sel) {
                    s.time = *val;
                    pat_changed = true;
                }
            }
        }
        c.text(220.0, ctl_y + 6.0, Align2::LEFT_CENTER, format!("LENGTH {len}"), 9.0, Color32::WHITE);
        for (k, (lbl, d)) in [("−", -1i32), ("+", 1)].iter().enumerate() {
            let x = 290.0 + k as f32 * 20.0;
            let r = cap(&c, ui, ("len", k), x, ctl_y, 16.0, 12.0, false);
            c.text(x + 8.0, ctl_y + 6.0, Align2::CENTER_CENTER, lbl, 9.0, INK);
            if r.clicked() {
                pat.len = (pat.len as i32 + d).clamp(1, 16) as u8;
                pat_changed = true;
            }
        }
    }

    // Key presses.
    if let Some(semi) = pressed {
        match st.mode {
            KeyMode::Pattern => {
                if let Some(k) = whites.iter().position(|w| w.0 == semi) {
                    host.set_param(p::PATTERN, k as f32);
                }
            }
            KeyMode::Keyboard => {
                let pitch = (base + semi as i32 + st.octave * 12).clamp(0, 127) as u8;
                host.note_on(pitch, if st.accent_live { 120 } else { 90 });
                st.held_key = Some(semi);
            }
            KeyMode::Edit => {
                if let Some(s) = pat.steps.get_mut(sel) {
                    let oct = if (s.pitch as i32) < base { -12 } else if s.pitch as i32 >= base + 12 && semi < 12 { 12 } else { 0 };
                    s.pitch = (base + semi as i32 + oct).clamp(12, 96) as u8;
                    s.time = 0;
                    pat_changed = true;
                    // Audition and advance the cursor.
                    host.audition(s.pitch, if s.accent { 120 } else { 90 });
                    st.sel_step = (sel + 1) % (pat.len.max(1) as usize);
                }
            }
        }
    }
    if released || (st.held_key.is_some() && !ui.input(|i| i.pointer.primary_down())) {
        if let (Some(semi), KeyMode::Keyboard) = (st.held_key, st.mode) {
            let pitch = (base + semi as i32 + st.octave * 12).clamp(0, 127) as u8;
            host.note_off(pitch);
        }
        st.held_key = None;
    }
    if st.about {
        let r = c.rect(300.0, 120.0, 500.0, 110.0);
        c.p.rect_filled(r, 6.0 * s, Color32::from_rgba_unmultiplied(20, 20, 22, 235));
        c.text(550.0, 140.0, Align2::CENTER_CENTER, "B-303 Bass Line", 18.0, Color32::WHITE);
        c.text(550.0, 170.0, Align2::CENTER_CENTER, "Musicton's TB-303 style bassline with internal sequencer.", 11.0, TEXT);
        c.text(550.0, 190.0, Align2::CENTER_CENTER, "Not affiliated with Roland. Click ABOUT to close.", 10.0, TEXT_DIM);
    }
    let _ = sel_pitch;
    if pat_changed {
        host.set_pattern(pat);
    }
}


fn knob(c: &Canvas, ui: &mut egui::Ui, host: &mut dyn Host, idx: usize, x: f32, y: f32, r: f32) {
    let sp = &SPECS[idx];
    let v = host.param(idx);
    let mut norm = to_norm(sp, v);
    let resp = chrome_knob(c, ui, ("b303k", idx), x, y, r, &mut norm, true);
    let mut nv = from_norm(sp, norm);
    if resp.double_clicked() {
        nv = sp.def;
    }
    if sp.is_choice() && resp.clicked() && !resp.dragged() {
        nv = ((v.round() as usize + 1) % sp.options.len()) as f32;
    }
    if nv != v {
        host.set_param(idx, nv);
    }
    resp.context_menu(|ui| host.knob_menu(ui, idx));
    if resp.hovered() || resp.dragged() {
        host.hint(sp.name, &format!("{}\n\nDrag up/down (Shift: fine), mouse wheel, double-click to reset.", sp.format(nv)));
        c.p.text(c.pt(x, y + r + 14.0), Align2::CENTER_TOP, sp.format(nv), FontId::proportional(9.5 * c.s), ORANGE);
    }
}
