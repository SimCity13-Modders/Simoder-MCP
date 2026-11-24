#![deny(unsafe_code)]

//! Optional native DevTool MCP mod (`simoder_mcp.dll`, `ABI v2`) for Simoder (`U4`/`U5`, `[Rust-6]`).
//!
//! Interacts with `Simoder` (`d3d9.dll`) across the Native Module Host Boundary (`ABI v2`:
//! [`SimoderModule_QueryV2`], [`SimoderModule_PrepareV2`], [`SimoderModule_AdmitV2`],
//! [`SimoderModule_QuiesceV2`], [`SimoderModule_DrainV2`]) without embedding or duplicating
//! `Simoder`'s internal runtime crates (`ADR-001`..`ADR-005`).

pub mod abi;
#[cfg(not(test))]
pub(crate) mod abi_bridges;
pub mod auth;
pub mod catalog;
pub mod config;
pub mod host;
pub mod http_server;
pub mod packaging;
pub mod protocol;
pub mod types;

pub use {abi::*, auth::*, catalog::*, config::*, host::*, http_server::*, packaging::*, protocol::*, types::*};
