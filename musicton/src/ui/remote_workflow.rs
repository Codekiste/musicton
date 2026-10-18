//! Remote commands for arranger, playlist, routing, sidechain, racks and modulators.

use super::App;
use crate::model::*;
use serde_json::{json, Value};

type R = Result<Value, String>;

fn f(a: &Value, k: &str) -> Option<f32> {
    a.get(k).and_then(|v| v.as_f64()).map(|v| v as f32)
}
fn u(a: &Value, k: &str) -> Option<u64> {
    a.get(k).and_then(|v| v.as_u64())
}
fn s<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.as_str())
}
fn b(a: &Value, k: &str) -> Option<bool> {
    a.get(k).and_then(|v| v.as_bool())
}
/// 1-based bar -> steps.
fn bar(a: &Value, k: &str) -> Option<u32> {
    f(a, k).map(|x| ((x - 1.0).max(0.0) * 16.0).round() as u32)
}
fn bars(a: &Value, k: &str) -> Option<u32> {
    f(a, k).map(|x| (x * 16.0).round().max(1.0) as u32)
}

impl App {
    fn section_arg(&self, a: &Value) -> Result<u64, String> {
        if let Some(id) = u(a, "id") {
            if self.project.sections.iter().any(|x| x.id == id) {
                return Ok(id);
            }
        }
        let name = s(a, "section").or(s(a, "name")).ok_or("missing section 'id' or 'section' name")?;
        self.project.sections.iter().find(|x| x.name.eq_ignore_ascii_case(name)).map(|x| x.id).ok_or(format!("no section '{name}'"))
    }

    pub(super) fn exec_workflow(&mut self, cmd: &str, a: &Value) -> Option<R> {
        let r = match cmd {
            "arranger" => self.cmd_arranger(a),
            "playlist" => self.cmd_playlist(a),
            "set_routing" => self.cmd_routing(a),
            "set_sidechain" => self.cmd_sidechain(a),
            "rack" => self.cmd_rack(a),
            "modulator" => self.cmd_modulator(a),
            _ => return None,
        };
        if r.is_ok() {
            self.touch();
        }
        Some(r)
    }

    fn cmd_arranger(&mut self, a: &Value) -> R {
        let p = &mut self.project;
        match s(a, "action").ok_or("missing 'action'")? {
            "add_section" => {
                let start = bar(a, "start_bar").unwrap_or_else(|| p.sections.iter().map(|x| x.start + x.len).max().unwrap_or(0));
                let id = p.add_section(s(a, "name"), start, bars(a, "bars").unwrap_or(128));
                Ok(json!({ "section_id": id }))
            }
            "move_section" => {
                let id = self.section_arg(a)?;
                let to = bar(a, "start_bar").ok_or("missing 'start_bar'")?;
                self.project.move_section(id, to);
                Ok(json!("moved"))
            }
            "resize_section" => {
                let id = self.section_arg(a)?;
                let len = bars(a, "bars").ok_or("missing 'bars'")?;
                if let Some(x) = self.project.sections.iter_mut().find(|x| x.id == id) {
                    x.len = len;
                }
                Ok(json!("ok"))
            }
            "rename_section" => {
                let id = self.section_arg(a)?;
                let n = s(a, "new_name").ok_or("missing 'new_name'")?;
                if let Some(x) = self.project.sections.iter_mut().find(|x| x.id == id) {
                    x.name = n.to_string();
                }
                Ok(json!("ok"))
            }
            "duplicate_section" => {
                let id = self.section_arg(a)?;
                Ok(json!({ "section_id": self.project.duplicate_section(id) }))
            }
            "delete_section" => {
                let id = self.section_arg(a)?;
                if b(a, "keep_content") == Some(true) {
                    self.project.sections.retain(|x| x.id != id);
                    self.project.chain.retain(|c| *c != id);
                } else {
                    self.project.delete_section(id);
                }
                Ok(json!("deleted"))
            }
            "add_marker" => {
                let pos = bar(a, "bar").ok_or("missing 'bar'")?;
                Ok(json!({ "marker_id": self.add_marker(s(a, "name"), pos) }))
            }
            "delete_marker" => {
                let id = u(a, "id").ok_or("missing marker 'id'")?;
                p.markers.retain(|m| m.id != id);
                Ok(json!("deleted"))
            }
            "set_chain" => {
                let list = a.get("sections").and_then(|v| v.as_array()).ok_or("missing 'sections': [name or id, ...]")?;
                let mut chain = vec![];
                for v in list {
                    let id = match v {
                        Value::Number(n) => n.as_u64(),
                        Value::String(n) => p.sections.iter().find(|x| x.name.eq_ignore_ascii_case(n)).map(|x| x.id),
                        _ => None,
                    }
                    .ok_or(format!("unknown section {v}"))?;
                    chain.push(id);
                }
                p.chain = chain;
                if let Some(on) = b(a, "on") {
                    p.chain_on = on;
                }
                Ok(json!({ "chain_len": p.chain.len(), "on": p.chain_on }))
            }
            "chain_on" => {
                p.chain_on = b(a, "on").unwrap_or(true);
                Ok(json!({ "on": p.chain_on }))
            }
            other => Err(format!("unknown action '{other}' (add_section, move_section, resize_section, rename_section, duplicate_section, delete_section, add_marker, delete_marker, set_chain, chain_on)")),
        }
    }

    fn cmd_playlist(&mut self, a: &Value) -> R {
        match s(a, "action").ok_or("missing 'action'")? {
            "add" => {
                let scene = u(a, "scene").ok_or("missing 'scene'")? as usize;
                if scene >= self.project.scenes.len() {
                    return Err(format!("no scene {scene}"));
                }
                let start = bar(a, "start_bar").unwrap_or_else(|| self.project.playlist_end());
                let id = self.add_block(scene, start, u(a, "lane").unwrap_or(0) as u32, bars(a, "bars"));
                Ok(json!({ "block_id": id }))
            }
            "move" => {
                let id = u(a, "id").ok_or("missing block 'id'")?;
                let bl = self.project.playlist.iter_mut().find(|x| x.id == id).ok_or("no such block")?;
                if let Some(st) = bar(a, "start_bar") {
                    bl.start = st;
                }
                if let Some(l) = u(a, "lane") {
                    bl.lane = (l as u32).min(super::arranger::PL_LANES - 1);
                }
                if let Some(l) = bars(a, "bars") {
                    bl.len = l;
                }
                if let Some(sc) = u(a, "scene") {
                    bl.scene = sc as usize;
                }
                Ok(json!("ok"))
            }
            "delete" => {
                let id = u(a, "id").ok_or("missing block 'id'")?;
                self.project.playlist.retain(|x| x.id != id);
                Ok(json!("deleted"))
            }
            "clear" => {
                self.project.playlist.clear();
                Ok(json!("cleared"))
            }
            "show" => {
                self.arr.ext.playlist = b(a, "on").unwrap_or(true);
                self.view = super::View::Arrangement;
                Ok(json!({ "playlist_visible": self.arr.ext.playlist }))
            }
            other => Err(format!("unknown action '{other}' (add, move, delete, clear, show)")),
        }
    }

    fn cmd_routing(&mut self, a: &Value) -> R {
        let id = self.resolve_track(a.get("track"))?;
        let out = match a.get("output").ok_or("missing 'output': master | return_a | return_b | track")? {
            Value::String(x) if x.eq_ignore_ascii_case("master") => Output::Master,
            Value::String(x) if x.eq_ignore_ascii_case("return_a") => Output::Return(0),
            Value::String(x) if x.eq_ignore_ascii_case("return_b") => Output::Return(1),
            v => {
                let t = self.resolve_track(Some(v))?;
                if t == id {
                    return Err("a track cannot feed itself".into());
                }
                Output::Track(t)
            }
        };
        self.project.track_mut(id).unwrap().output = out;
        Ok(json!({ "output": super::remote::output_name(out, &self.project) }))
    }

    fn cmd_sidechain(&mut self, a: &Value) -> R {
        let dev = u(a, "device_id").ok_or("missing 'device_id'")?;
        let src = match a.get("source") {
            None | Some(Value::Null) => None,
            Some(v) => Some(self.resolve_track(Some(v))?),
        };
        let d = self.project.device_mut(dev).ok_or("no such device")?;
        if !d.kind.has_sidechain() {
            return Err(format!("{} has no sidechain input (use Compressor, Bus Compressor, Gate or Sidechain)", d.kind.name()));
        }
        d.sc_source = src;
        Ok(json!({ "sidechain_from_track_id": src }))
    }

    fn cmd_rack(&mut self, a: &Value) -> R {
        let rid = u(a, "device_id").ok_or("missing 'device_id' (the Rack)")?;
        let action = s(a, "action").ok_or("missing 'action'")?.to_string();
        let new_id = self.project.new_id();
        let d = self.project.device_mut(rid).ok_or("no such device")?;
        if d.kind != DeviceKind::Rack {
            return Err("device is not a Rack".into());
        }
        let ci = u(a, "chain").unwrap_or(0) as usize;
        match action.as_str() {
            "add_chain" => {
                let n = d.chains.len() + 1;
                d.chains.push(RackChain::new(s(a, "name").unwrap_or(&format!("Chain {n}"))));
                Ok(json!({ "chain": d.chains.len() - 1 }))
            }
            "remove_chain" => {
                if ci >= d.chains.len() {
                    return Err("no such chain".into());
                }
                d.chains.remove(ci);
                Ok(json!("removed"))
            }
            "set_chain" => {
                let c = d.chains.get_mut(ci).ok_or("no such chain")?;
                if let Some(v) = f(a, "volume") {
                    c.volume = v.clamp(0.0, 1.0);
                }
                if let Some(v) = f(a, "pan") {
                    c.pan = v.clamp(-1.0, 1.0);
                }
                if let Some(v) = b(a, "mute") {
                    c.mute = v;
                }
                if let Some(v) = s(a, "name") {
                    c.name = v.to_string();
                }
                Ok(json!("ok"))
            }
            "add_device" => {
                let name = s(a, "device").ok_or("missing 'device'")?;
                let kind = DeviceKind::all().find(|k| k.name().eq_ignore_ascii_case(name) && *k != DeviceKind::Rack).ok_or(format!("unknown effect '{name}'"))?;
                let c = d.chains.get_mut(ci).ok_or("no such chain")?;
                c.devices.push(Device::new(new_id, kind));
                Ok(json!({ "device_id": new_id }))
            }
            other => Err(format!("unknown action '{other}' (add_chain, remove_chain, set_chain, add_device)")),
        }
    }

    fn cmd_modulator(&mut self, a: &Value) -> R {
        let tid = self.resolve_track(a.get("track"))?;
        let action = s(a, "action").ok_or("missing 'action' (add, set, remove)")?.to_string();
        // Resolve the target before borrowing the track mutably.
        let target = match s(a, "target") {
            None => None,
            Some(tg) => Some(match tg {
                "none" => None,
                "volume" => Some(AutoTarget::Volume),
                "pan" => Some(AutoTarget::Pan),
                "send_a" => Some(AutoTarget::Send(0)),
                "send_b" => Some(AutoTarget::Send(1)),
                "instrument" => {
                    let t = self.project.track(tid).unwrap();
                    let defs = t.instrument.kind.defs();
                    let key = a.get("param").ok_or("missing 'param'")?;
                    Some(AutoTarget::Instrument(param_index(defs, key)?))
                }
                "device" => {
                    let dev = u(a, "device_id").ok_or("missing 'device_id'")?;
                    let d = self.project.device_mut(dev).ok_or("no such device")?;
                    let key = a.get("param").ok_or("missing 'param'")?;
                    Some(AutoTarget::Device { id: dev, param: param_index(d.kind.defs(), key)? })
                }
                other => return Err(format!("unknown target '{other}'")),
            }),
        };
        let new_id = self.project.new_id();
        let t = self.project.track_mut(tid).unwrap();
        let m = match action.as_str() {
            "add" => {
                t.modulators.push(Modulator::new(new_id));
                t.modulators.last_mut().unwrap()
            }
            "set" => {
                let id = u(a, "id").ok_or("missing modulator 'id'")?;
                t.modulators.iter_mut().find(|m| m.id == id).ok_or("no such modulator")?
            }
            "remove" => {
                let id = u(a, "id").ok_or("missing modulator 'id'")?;
                t.modulators.retain(|m| m.id != id);
                return Ok(json!("removed"));
            }
            other => return Err(format!("unknown action '{other}'")),
        };
        if let Some(sh) = s(a, "shape") {
            m.shape = ModShape::ALL.iter().copied().find(|x| x.name().eq_ignore_ascii_case(sh)).ok_or(format!("unknown shape '{sh}'"))?;
        }
        if let Some(div) = s(a, "rate") {
            m.synced = true;
            m.rate = crate::params::div_index(div) as f32;
        } else if let Some(hz) = f(a, "rate_hz") {
            m.synced = false;
            m.rate = hz.clamp(0.01, 40.0);
        }
        if let Some(v) = f(a, "depth") {
            m.depth = v.clamp(-1.0, 1.0);
        }
        if let Some(v) = f(a, "attack") {
            m.attack = v.max(0.001);
        }
        if let Some(v) = f(a, "decay") {
            m.decay = v.max(0.01);
        }
        if let Some(v) = b(a, "on") {
            m.on = v;
        }
        if let Some(tg) = target {
            m.target = tg;
        }
        Ok(json!({ "modulator_id": m.id }))
    }
}

fn param_index(defs: &[crate::params::ParamDef], key: &Value) -> Result<usize, String> {
    if let Some(i) = key.as_u64() {
        return if (i as usize) < defs.len() { Ok(i as usize) } else { Err(format!("no param {i}")) };
    }
    let n = key.as_str().ok_or("param must be a name or index")?;
    defs.iter().position(|d| d.name.eq_ignore_ascii_case(n)).ok_or(format!("unknown param '{n}'"))
}
