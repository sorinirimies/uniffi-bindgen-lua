//! A Lua bindings generator for [UniFFI](https://mozilla.github.io/uniffi-rs/).
//!
//! Point it at a library built with UniFFI and it writes, per component, the C
//! source of a Lua module (on the Lua C API, Lua 5.1/LuaJIT to 5.5), plus the
//! runtime headers it includes:
//!
//! ```text
//! uniffi-bindgen-lua generate target/release/libmycrate.so --out-dir lua/
//! ```
//!
//! Build `<namespace>.c` into a shared library named after the namespace, linking
//! the Rust library, and `require` it from Lua.

mod cli;
mod codegen;
mod config;

pub use cli::run;
pub use codegen::{
    GeneratedFiles, LUA_RUNTIME_HEADER, LUA_RUNTIME_HEADER_NAME, LuaBindingGenerator,
    RUNTIME_HEADER, RUNTIME_HEADER_NAME, generate_component, generate_from_udl, write_runtime,
};
pub use config::Config;
