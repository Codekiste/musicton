//! Binary message protocol between Musicton and `musicton-plughost`, the sandbox process that
//! loads one VST3 or CLAP plug-in. Messages are framed as `[tag u8][len u32 LE][payload]`.

use std::io::{self, Read, Write};

#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
    /// Number of discrete steps (0 = continuous).
    pub steps: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEvent {
    pub offset: u32,
    pub on: bool,
    pub key: u8,
    pub velocity: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Load { path: String, id: String, sample_rate: f64, max_block: u32 },
    /// Planar input: `frames` left samples, then `frames` right samples.
    Process { frames: u32, tempo: f64, playing: bool, events: Vec<NoteEvent>, input: Vec<f32> },
    SetParam { id: u32, value: f64 },
    GetState,
    SetState(Vec<u8>),
    ShowGui(bool),
    Quit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Loaded { name: String, vendor: String, instrument: bool, gui: bool, params: Vec<ParamInfo> },
    /// Planar output, like the input of `Process`.
    Audio { frames: u32, output: Vec<f32> },
    /// The plug-in changed a parameter (its own GUI or automation).
    Param { id: u32, value: f64 },
    State(Vec<u8>),
    Ok,
    Error(String),
}

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    fn f32s(&mut self, v: &[f32]) {
        self.u32(v.len() as u32);
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }
}

struct R<'a>(&'a [u8], usize);

impl R<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.1 + n > self.0.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "short message"));
        }
        let s = &self.0[self.1..self.1 + n];
        self.1 += n;
        Ok(s)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn str(&mut self) -> io::Result<String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn bytes(&mut self) -> io::Result<Vec<u8>> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn f32s(&mut self) -> io::Result<Vec<f32>> {
        let n = self.u32()? as usize;
        let b = self.take(n * 4)?;
        Ok(b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
    }
}

fn frame(out: &mut impl Write, tag: u8, payload: &[u8]) -> io::Result<()> {
    let mut buf = Vec::with_capacity(payload.len() + 5);
    buf.push(tag);
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    out.write_all(&buf)?;
    out.flush()
}

fn read_frame(inp: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    inp.read_exact(&mut head)?;
    let len = u32::from_le_bytes(head[1..5].try_into().unwrap()) as usize;
    if len > 64 << 20 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut payload = vec![0u8; len];
    inp.read_exact(&mut payload)?;
    Ok((head[0], payload))
}

impl Request {
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let mut w = W(vec![]);
        let tag = match self {
            Request::Load { path, id, sample_rate, max_block } => {
                w.str(path);
                w.str(id);
                w.f64(*sample_rate);
                w.u32(*max_block);
                1
            }
            Request::Process { frames, tempo, playing, events, input } => {
                w.u32(*frames);
                w.f64(*tempo);
                w.u8(*playing as u8);
                w.u32(events.len() as u32);
                for e in events {
                    w.u32(e.offset);
                    w.u8(e.on as u8);
                    w.u8(e.key);
                    w.u8(e.velocity);
                }
                w.f32s(input);
                2
            }
            Request::SetParam { id, value } => {
                w.u32(*id);
                w.f64(*value);
                3
            }
            Request::GetState => 4,
            Request::SetState(b) => {
                w.bytes(b);
                5
            }
            Request::ShowGui(on) => {
                w.u8(*on as u8);
                6
            }
            Request::Quit => 7,
        };
        frame(out, tag, &w.0)
    }

    pub fn read(inp: &mut impl Read) -> io::Result<Self> {
        let (tag, p) = read_frame(inp)?;
        let mut r = R(&p, 0);
        Ok(match tag {
            1 => Request::Load { path: r.str()?, id: r.str()?, sample_rate: r.f64()?, max_block: r.u32()? },
            2 => {
                let frames = r.u32()?;
                let tempo = r.f64()?;
                let playing = r.u8()? != 0;
                let n = r.u32()? as usize;
                let mut events = Vec::with_capacity(n);
                for _ in 0..n {
                    events.push(NoteEvent { offset: r.u32()?, on: r.u8()? != 0, key: r.u8()?, velocity: r.u8()? });
                }
                Request::Process { frames, tempo, playing, events, input: r.f32s()? }
            }
            3 => Request::SetParam { id: r.u32()?, value: r.f64()? },
            4 => Request::GetState,
            5 => Request::SetState(r.bytes()?),
            6 => Request::ShowGui(r.u8()? != 0),
            7 => Request::Quit,
            t => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unknown request {t}"))),
        })
    }
}

impl Reply {
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let mut w = W(vec![]);
        let tag = match self {
            Reply::Loaded { name, vendor, instrument, gui, params } => {
                w.str(name);
                w.str(vendor);
                w.u8(*instrument as u8);
                w.u8(*gui as u8);
                w.u32(params.len() as u32);
                for p in params {
                    w.u32(p.id);
                    w.str(&p.name);
                    w.f64(p.min);
                    w.f64(p.max);
                    w.f64(p.default);
                    w.f64(p.value);
                    w.u32(p.steps);
                }
                1
            }
            Reply::Audio { frames, output } => {
                w.u32(*frames);
                w.f32s(output);
                2
            }
            Reply::Param { id, value } => {
                w.u32(*id);
                w.f64(*value);
                3
            }
            Reply::State(b) => {
                w.bytes(b);
                4
            }
            Reply::Ok => 5,
            Reply::Error(e) => {
                w.str(e);
                6
            }
        };
        frame(out, tag, &w.0)
    }

    pub fn read(inp: &mut impl Read) -> io::Result<Self> {
        let (tag, p) = read_frame(inp)?;
        let mut r = R(&p, 0);
        Ok(match tag {
            1 => {
                let name = r.str()?;
                let vendor = r.str()?;
                let instrument = r.u8()? != 0;
                let gui = r.u8()? != 0;
                let n = r.u32()? as usize;
                let mut params = Vec::with_capacity(n);
                for _ in 0..n {
                    params.push(ParamInfo { id: r.u32()?, name: r.str()?, min: r.f64()?, max: r.f64()?, default: r.f64()?, value: r.f64()?, steps: r.u32()? });
                }
                Reply::Loaded { name, vendor, instrument, gui, params }
            }
            2 => Reply::Audio { frames: r.u32()?, output: r.f32s()? },
            3 => Reply::Param { id: r.u32()?, value: r.f64()? },
            4 => Reply::State(r.bytes()?),
            5 => Reply::Ok,
            6 => Reply::Error(r.str()?),
            t => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unknown reply {t}"))),
        })
    }
}

/// One plug-in found by a scan (`musicton-plughost --scan <path>` prints these as lines).
#[derive(Clone, Debug, PartialEq)]
pub struct PluginDesc {
    pub format: String,
    pub path: String,
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub instrument: bool,
}

impl PluginDesc {
    /// Tab-separated line (no external serializer needed).
    pub fn to_line(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n'], " ");
        format!("{}\t{}\t{}\t{}\t{}\t{}", self.format, clean(&self.path), clean(&self.id), clean(&self.name), clean(&self.vendor), self.instrument as u8)
    }
    pub fn from_line(l: &str) -> Option<Self> {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() != 6 {
            return None;
        }
        Some(Self { format: f[0].into(), path: f[1].into(), id: f[2].into(), name: f[3].into(), vendor: f[4].into(), instrument: f[5] == "1" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let reqs = vec![
            Request::Load { path: "/x.clap".into(), id: "a.b".into(), sample_rate: 48000.0, max_block: 64 },
            Request::Process { frames: 3, tempo: 128.0, playing: true, events: vec![NoteEvent { offset: 1, on: true, key: 60, velocity: 100 }], input: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6] },
            Request::SetParam { id: 7, value: 0.25 },
            Request::SetState(vec![1, 2, 3]),
            Request::ShowGui(true),
            Request::Quit,
        ];
        let mut buf = vec![];
        for r in &reqs {
            r.write(&mut buf).unwrap();
        }
        let mut c = std::io::Cursor::new(buf);
        for r in &reqs {
            assert_eq!(&Request::read(&mut c).unwrap(), r);
        }
        let reps = vec![
            Reply::Loaded { name: "N".into(), vendor: "V".into(), instrument: true, gui: false, params: vec![ParamInfo { id: 1, name: "Gain".into(), min: 0.0, max: 1.0, default: 0.5, value: 0.5, steps: 0 }] },
            Reply::Audio { frames: 1, output: vec![0.5, -0.5] },
            Reply::Param { id: 1, value: 0.75 },
            Reply::Error("boom".into()),
        ];
        let mut buf = vec![];
        for r in &reps {
            r.write(&mut buf).unwrap();
        }
        let mut c = std::io::Cursor::new(buf);
        for r in &reps {
            assert_eq!(&Reply::read(&mut c).unwrap(), r);
        }
        let d = PluginDesc { format: "clap".into(), path: "/p".into(), id: "i".into(), name: "n".into(), vendor: "v".into(), instrument: true };
        assert_eq!(PluginDesc::from_line(&d.to_line()), Some(d));
    }
}
