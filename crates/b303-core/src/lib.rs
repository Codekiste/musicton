//! The B-303 bassline, independent of any host: Musicton uses it as a built-in instrument and
//! `b303-plugin` wraps it as a VST3/CLAP plug-in.

pub mod engine;
pub mod seq;
pub mod spec;

pub use engine::B303;
pub use seq::{random_pattern, SeqBank, SeqPattern, SeqStep, Step, StepTime};
pub use spec::{p, Spec, Unit, SPECS};
