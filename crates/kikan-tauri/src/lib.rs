//! kikan-tauri — helpers specific to the Tauri desktop shell.
//!
//! Per `adr-kikan-binary-topology` + `adr-tauri-http-not-ipc`, Mokumo uses
//! real HTTP for the data plane and UDS for admin. `#[tauri::command]`
//! wrappers are not used for control or data plane logic. This crate
//! holds only shell-specific helpers and exists as its own crate so kikan
//! stays adapter-shell-agnostic (I2).

pub mod net;

pub use net::try_bind_ephemeral_loopback;
