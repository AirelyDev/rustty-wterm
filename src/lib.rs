//! rustty-wterm — wterm's `ghostty-vt.wasm` ABI on the Rustty VT engine.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod cell;
mod core;
mod scrollback;
pub mod sinks;
pub use crate::core::Core;

#[cfg(target_arch = "wasm32")]
mod exports;
