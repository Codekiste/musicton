//! Project versions: named snapshots of a project, listed, compared and restored.
//! Stored as `<id>.mton` plus `<id>.json` (metadata) in a per-project folder.

use crate::model::Project;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VersionInfo {
    pub id: String,
    /// Unix seconds.
    pub time: u64,
    pub message: String,
    pub summary: String,
}

pub fn versions_dir(project_name: &str) -> PathBuf {
    let safe: String = project_name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("musicton").join("versions").join(if safe.is_empty() { "untitled".into() } else { safe })
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "2026-10-18 14:05" (UTC) from Unix seconds.
pub fn format_time(t: u64) -> String {
    let days = (t / 86400) as i64;
    let secs = t % 86400;
    // Civil-from-days (Gregorian calendar).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600, (secs % 3600) / 60)
}

pub fn summary(p: &Project) -> String {
    let clips: usize = p.tracks.iter().map(|t| t.clips.iter().flatten().count() + t.arrangement.len()).sum();
    format!("{} tracks, {} clips, {:.0} BPM", p.tracks.len(), clips, p.bpm)
}

pub fn save_version(dir: &Path, p: &Project, message: &str) -> Result<VersionInfo, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let time = now();
    let mut id = format!("{time}");
    let mut k = 1;
    while dir.join(format!("{id}.mton")).exists() {
        id = format!("{time}-{k}");
        k += 1;
    }
    let info = VersionInfo { id: id.clone(), time, message: message.to_string(), summary: summary(p) };
    std::fs::write(dir.join(format!("{id}.mton")), serde_json::to_string(p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string_pretty(&info).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(info)
}

/// Newest first.
pub fn list_versions(dir: &Path) -> Vec<VersionInfo> {
    let mut v: Vec<VersionInfo> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .filter_map(|p| std::fs::read_to_string(p).ok())
                .filter_map(|s| serde_json::from_str(&s).ok())
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a: &VersionInfo, b| b.time.cmp(&a.time).then(b.id.cmp(&a.id)));
    v
}

pub fn load_version(dir: &Path, id: &str) -> Result<Project, String> {
    let s = std::fs::read_to_string(dir.join(format!("{id}.mton"))).map_err(|e| e.to_string())?;
    let mut p: Project = serde_json::from_str(&s).map_err(|e| e.to_string())?;
    p.normalize();
    Ok(p)
}

/// Keeps the newest `keep` versions.
pub fn prune(dir: &Path, keep: usize) {
    for v in list_versions(dir).into_iter().skip(keep) {
        let _ = std::fs::remove_file(dir.join(format!("{}.mton", v.id)));
        let _ = std::fs::remove_file(dir.join(format!("{}.json", v.id)));
    }
}

/// Human-readable differences from `old` to `new`.
pub fn diff(old: &Project, new: &Project) -> Vec<String> {
    let mut out = vec![];
    if (old.bpm - new.bpm).abs() > 0.01 {
        out.push(format!("Tempo {:.1} -> {:.1} BPM", old.bpm, new.bpm));
    }
    if old.key != new.key || old.scale != new.scale {
        out.push("Key or scale changed".into());
    }
    for t in &new.tracks {
        match old.tracks.iter().find(|o| o.id == t.id) {
            None => out.push(format!("+ Track \"{}\" ({})", t.name, t.instrument.kind.name())),
            Some(o) => {
                let mut ch = vec![];
                if o.name != t.name {
                    ch.push(format!("renamed from \"{}\"", o.name));
                }
                if o.instrument.kind != t.instrument.kind {
                    ch.push(format!("instrument {} -> {}", o.instrument.kind.name(), t.instrument.kind.name()));
                } else if o.instrument.params != t.instrument.params {
                    ch.push("sound edited".into());
                }
                let (oc, nc) = (o.clips.iter().flatten().count(), t.clips.iter().flatten().count());
                if oc != nc {
                    ch.push(format!("launcher clips {oc} -> {nc}"));
                } else if o.clips.iter().zip(&t.clips).any(|(a, b)| a.as_ref().map(|c| &c.notes) != b.as_ref().map(|c| &c.notes)) {
                    ch.push("notes edited".into());
                }
                if o.arrangement.len() != t.arrangement.len() {
                    ch.push(format!("timeline clips {} -> {}", o.arrangement.len(), t.arrangement.len()));
                }
                let names = |d: &[crate::model::Device]| d.iter().map(|x| x.kind.name()).collect::<Vec<_>>().join(", ");
                if names(&o.channel.devices) != names(&t.channel.devices) {
                    ch.push(format!("effects [{}] -> [{}]", names(&o.channel.devices), names(&t.channel.devices)));
                } else if o.channel.devices != t.channel.devices {
                    ch.push("effect settings".into());
                }
                if (o.channel.volume - t.channel.volume).abs() > 0.001 || (o.channel.pan - t.channel.pan).abs() > 0.001 {
                    ch.push("mix changed".into());
                }
                if !ch.is_empty() {
                    out.push(format!("~ Track \"{}\": {}", t.name, ch.join(", ")));
                }
            }
        }
    }
    for o in old.tracks.iter().filter(|o| !new.tracks.iter().any(|t| t.id == o.id)) {
        out.push(format!("- Track \"{}\"", o.name));
    }
    if old.sections.len() != new.sections.len() || old.chords.len() != new.chords.len() || old.markers.len() != new.markers.len() {
        out.push(format!(
            "Arranger: {} -> {} sections, {} -> {} chords, {} -> {} markers",
            old.sections.len(),
            new.sections.len(),
            old.chords.len(),
            new.chords.len(),
            old.markers.len(),
            new.markers.len()
        ));
    }
    if out.is_empty() {
        out.push("No differences".into());
    }
    out
}
