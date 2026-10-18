//! Remote commands for the track, clip and time operations of the right-click menus.

use super::{App, ClipRef};
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
/// 1-based bar (fractions allowed) -> steps.
fn bar(a: &Value, k: &str) -> Option<u32> {
    f(a, k).map(|x| ((x - 1.0).max(0.0) * 16.0).round() as u32)
}

impl App {
    pub(super) fn cmd_track_action(&mut self, a: &Value) -> R {
        let action = s(a, "action").ok_or("missing 'action'")?;
        // Project-wide actions need no track.
        match action {
            "delete_unused" => return Ok(json!({ "deleted": self.delete_unused_tracks() })),
            "auto_color" => {
                self.auto_color_tracks();
                return Ok(json!("ok"));
            }
            "show_hidden" => {
                self.show_hidden = b(a, "on").unwrap_or(true);
                return Ok(json!({ "show_hidden": self.show_hidden }));
            }
            "unhide_all" => {
                self.unhide_all_tracks();
                return Ok(json!("ok"));
            }
            "add_return" => return self.add_return_track().map(|k| json!({ "return": return_letter(k).to_string(), "index": k })),
            "delete_return" => {
                let k = u(a, "return_index").ok_or("missing 'return_index' (0 = A)")? as usize;
                self.delete_return_track(k);
                return Ok(json!({ "returns": self.project.returns.len() }));
            }
            "insert_default" => {
                let id = self.insert_default_track(b(a, "audio").unwrap_or(false), None);
                return Ok(json!({ "track_index": self.project.track_index(id) }));
            }
            "paste" if a.get("track").is_none() => {
                let id = self.paste_track(None).ok_or("nothing copied")?;
                return Ok(json!({ "track_index": self.project.track_index(id) }));
            }
            _ => {}
        }
        let id = self.resolve_track(a.get("track"))?;
        let idx = |app: &App, t: u64| json!({ "track_index": app.project.track_index(t) });
        let on = b(a, "on");
        let cur = self.project.track(id).cloned().ok_or("no such track")?;
        match action {
            "copy" => {
                self.copy_track(id);
                Ok(json!("copied"))
            }
            "cut" => {
                if cur.locked {
                    return Err("track is protected".into());
                }
                self.cut_track(id);
                Ok(json!("cut"))
            }
            "paste" => self.paste_track(Some(id)).map(|t| idx(self, t)).ok_or_else(|| "nothing copied".to_string()),
            "duplicate" => {
                self.duplicate_track(id);
                Ok(json!("duplicated"))
            }
            "same_settings" => self.new_track_same_settings(id).map(|t| idx(self, t)).ok_or_else(|| "failed".to_string()),
            "delete" => {
                if cur.locked {
                    return Err("track is protected".into());
                }
                self.delete_track(id);
                Ok(json!("deleted"))
            }
            "rename" => {
                let name = s(a, "name").ok_or("missing 'name'")?.to_string();
                if let Some(t) = self.project.track_mut(id) {
                    t.name = name;
                }
                Ok(json!("ok"))
            }
            "info" => {
                self.set_track_info(id, s(a, "text").unwrap_or("").to_string());
                Ok(json!("ok"))
            }
            "group" => {
                self.group_track(id);
                Ok(json!("grouped"))
            }
            "folder" => {
                self.make_folder(id);
                Ok(json!("folder created"))
            }
            "set_folder" => {
                self.set_folder(id, on.unwrap_or(true));
                Ok(json!("ok"))
            }
            "ungroup" => {
                self.ungroup(id);
                Ok(json!("ok"))
            }
            "color" => {
                let c = match a.get("color") {
                    Some(Value::Array(v)) if v.len() == 3 => [v[0].as_u64().unwrap_or(0) as u8, v[1].as_u64().unwrap_or(0) as u8, v[2].as_u64().unwrap_or(0) as u8],
                    Some(v) => CLIP_COLORS[(v.as_u64().unwrap_or(0) as usize) % CLIP_COLORS.len()],
                    None => return Err("missing 'color' ([r,g,b] or palette index)".into()),
                };
                self.set_track_color(id, c);
                Ok(json!("ok"))
            }
            "icon" => {
                self.set_track_icon(id, s(a, "icon").unwrap_or(""));
                Ok(json!("ok"))
            }
            "hide" => {
                self.set_track_hidden(id, on.unwrap_or(true));
                Ok(json!("ok"))
            }
            "protect" => {
                self.set_track_locked(id, on.unwrap_or(true));
                Ok(json!("ok"))
            }
            "select_all_clips" => {
                self.select_all_clips(id);
                Ok(json!({ "selected": self.arr.multi.len() }))
            }
            "freeze" => self.freeze_track(id).map(|m| json!(m)),
            "unfreeze" => self.unfreeze_track(id).map(|m| json!(m)),
            "flatten" => self.flatten_track(id).map(|m| json!(m)),
            "bounce" => self.bounce_in_place(id, b(a, "with_fx").unwrap_or(true)).map(|m| json!(m)),
            "bounce_new" => self.bounce_to_new_track(id).map(|t| idx(self, t)),
            "add_take_lane" => self.add_take_lane(id).map(|l| json!({ "lane_id": l })).ok_or_else(|| "failed".to_string()),
            "show_takes" => {
                if let Some(t) = self.project.track_mut(id) {
                    t.show_takes = on.unwrap_or(true);
                }
                Ok(json!("ok"))
            }
            "show_automation" => {
                if on.unwrap_or(true) {
                    self.auto_open.insert(id);
                } else {
                    self.auto_open.remove(&id);
                }
                Ok(json!("ok"))
            }
            "save_default" => self.save_default_track(id).map(|m| json!(m)),
            "replace_instrument" => {
                let p = s(a, "preset").ok_or("missing 'preset'")?;
                if !crate::presets::INSTRUMENT_PRESETS.iter().any(|x| x.name.eq_ignore_ascii_case(p)) {
                    return Err(format!("unknown preset '{p}'"));
                }
                let name = crate::presets::INSTRUMENT_PRESETS.iter().find(|x| x.name.eq_ignore_ascii_case(p)).unwrap().name;
                self.replace_instrument(id, name);
                Ok(json!("ok"))
            }
            other => Err(format!(
                "unknown action '{other}' (copy, cut, paste, duplicate, same_settings, delete, rename, info, group, folder, set_folder, ungroup, color, icon, hide, show_hidden, unhide_all, protect, select_all_clips, freeze, unfreeze, flatten, bounce, bounce_new, add_take_lane, show_takes, show_automation, save_default, insert_default, replace_instrument, delete_unused, auto_color, add_return, delete_return)"
            )),
        }
    }

    pub(super) fn cmd_clip_action(&mut self, a: &Value) -> R {
        let action = s(a, "action").ok_or("missing 'action'")?;
        let r = self.clip_ref(a)?;
        let track = r.track();
        if self.clip(r).is_none() && !matches!(action, "paste") {
            return Err("no clip there".into());
        }
        let arr = match r {
            ClipRef::Arr { id, .. } => Some(id),
            _ => None,
        };
        let need_arr = || arr.ok_or_else(|| format!("'{action}' needs a timeline clip ('arrangement_clip_id')"));
        let res: R = match action {
            "split" => {
                let id = need_arr()?;
                let at = bar(a, "at_bar").or(u(a, "at_step").map(|x| x as u32)).ok_or("missing 'at_bar' (1-based, fractions allowed) or 'at_step'")?;
                self.split_clip(track, id, at).map(|n| json!({ "new_clip_id": n }))
            }
            "join" => {
                let id = need_arr()?;
                let mut ids: Vec<u64> = a.get("with").and_then(|v| v.as_array()).map(|v| v.iter().filter_map(|x| x.as_u64()).collect()).unwrap_or_default();
                ids.push(id);
                self.join_clips(track, &ids).map(|n| json!({ "new_clip_id": n }))
            }
            "crop" => self.crop_clip_to_loop(track, need_arr()?).map(|_| json!("cropped to the loop region")),
            "link_duplicate" => self.duplicate_linked(track, need_arr()?).map(|n| json!({ "new_clip_id": n })),
            "unlink" => {
                self.unlink_clip(r);
                Ok(json!("ok"))
            }
            "mute" => {
                let want = b(a, "on").unwrap_or(true);
                if self.clip(r).is_some_and(|c| c.muted != want) {
                    self.toggle_clip_mute(r);
                }
                Ok(json!({ "muted": self.clip(r).map(|c| c.muted) }))
            }
            "loop" => {
                self.loop_clip(track, need_arr()?);
                Ok(json!("ok"))
            }
            "unloop" => {
                self.unloop_clip(track, need_arr()?);
                Ok(json!("ok"))
            }
            "reverse" => self.reverse_audio_clip(r).map(|_| json!("reversed")),
            "warp" => {
                let w = match s(a, "mode").unwrap_or("beats") {
                    "off" => WarpMode::Off,
                    "repitch" => WarpMode::Repitch,
                    _ => WarpMode::Beats,
                };
                self.set_warp(r, w);
                Ok(json!("ok"))
            }
            "fit_bars" => self.fit_audio_to_bars(r).map(|m| json!(m)),
            "to_sampler" => {
                let kind = match s(a, "kind").unwrap_or("slicer") {
                    "pads" | "beat_pads" => InstrumentKind::BeatPads,
                    "multi" | "multi_sampler" => InstrumentKind::MultiSampler,
                    _ => InstrumentKind::Slicer,
                };
                self.convert_to_sampler(r, kind).map(|t| json!({ "track_index": self.project.track_index(t) }))
            }
            "pitch_correction" => {
                self.add_pitch_correction(track);
                Ok(json!("ok"))
            }
            "quantize" => {
                self.quantize_clip(r, u(a, "grid_steps").unwrap_or(2) as u32);
                Ok(json!("ok"))
            }
            "legato" => {
                self.legato_clip(r);
                Ok(json!("ok"))
            }
            "note_lengths" => {
                self.scale_note_lengths(r, f(a, "factor").unwrap_or(2.0));
                Ok(json!("ok"))
            }
            "velocity" => {
                match s(a, "mode").unwrap_or("set") {
                    "even" => self.compress_velocities(r),
                    "humanize" => self.humanize_velocities(r),
                    _ => self.set_velocities(r, u(a, "value").unwrap_or(100) as u8),
                }
                Ok(json!("ok"))
            }
            "export_audio" => {
                let p = s(a, "path").ok_or("missing 'path'")?;
                self.export_clip_audio(r, std::path::Path::new(p), b(a, "as_loop").unwrap_or(false)).map(|p| json!({ "path": p }))
            }
            "export_midi" => {
                let p = s(a, "path").ok_or("missing 'path'")?;
                self.export_clip_midi(r, std::path::Path::new(p)).map(|p| json!({ "path": p }))
            }
            "move_to_take" => {
                self.move_clip_to_take(track, need_arr()?);
                Ok(json!({ "take_lanes": self.project.track(track).map(|t| t.takes.len()) }))
            }
            "copy" => {
                let c = self.clip(r).cloned().unwrap();
                let len = match r {
                    ClipRef::Arr { track, id } => self.project.track(track).and_then(|t| t.arrangement.iter().find(|x| x.id == id)).map(|x| x.len).unwrap_or(c.len),
                    _ => c.len,
                };
                self.clip_clipboard = Some((c, len));
                Ok(json!("copied"))
            }
            other => Err(format!(
                "unknown action '{other}' (split, join, crop, link_duplicate, unlink, mute, loop, unloop, reverse, warp, fit_bars, to_sampler, pitch_correction, quantize, legato, note_lengths, velocity, export_audio, export_midi, move_to_take, copy)"
            )),
        };
        res
    }

    pub(super) fn cmd_time_action(&mut self, a: &Value) -> R {
        let action = s(a, "action").ok_or("missing 'action' (insert_silence, delete_time, duplicate_time, use_take, paste)")?;
        if action == "use_take" {
            let track = self.resolve_track(a.get("track"))?;
            let lane = u(a, "lane_id").ok_or("missing 'lane_id'")?;
            let clip = u(a, "take_clip_id").ok_or("missing 'take_clip_id'")?;
            self.use_take_clip(track, lane, clip);
            return Ok(json!("comped"));
        }
        if action == "paste" {
            let track = self.resolve_track(a.get("track"))?;
            let at = bar(a, "at_bar").ok_or("missing 'at_bar'")?;
            if self.clip_clipboard.is_none() {
                return Err("nothing copied".into());
            }
            self.paste_clip_at(track, at);
            return Ok(json!("pasted"));
        }
        let (start, len) = match (bar(a, "start_bar"), f(a, "bars")) {
            (Some(s), Some(l)) => (s, (l * 16.0).round().max(1.0) as u32),
            _ => self.loop_range().ok_or("give 'start_bar' and 'bars' or set a loop region")?,
        };
        match action {
            "insert_silence" => self.insert_time(start, len),
            "delete_time" => self.delete_time(start, len),
            "duplicate_time" => self.duplicate_time(start, len),
            other => return Err(format!("unknown action '{other}'")),
        }
        Ok(json!({ "start_bar": start / 16 + 1, "bars": len as f32 / 16.0 }))
    }
}
