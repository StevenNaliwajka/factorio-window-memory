//! The parts of factorio-window-memory that don't touch a live process: reading
//! factorio.pdb, matching it to factorio.exe, naming window classes, clamping
//! positions to the screen, persisting them, and parsing launcher arguments.

pub mod args;
pub mod geometry;
pub mod launch_option;
pub mod pe;
pub mod protocol;
pub mod rtti;
pub mod steam;
pub mod store;
pub mod symbols;
pub mod targets;
pub mod vdf;
