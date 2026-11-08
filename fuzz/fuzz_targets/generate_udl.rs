#![no_main]
//! Whatever text we are given, parsing it as UDL either fails cleanly or yields
//! a definition the generator renders without panicking; a rendered module
//! always has its luaopen function.

use libfuzzer_sys::fuzz_target;
use uniffi_bindgen_lua::{Config, generate_from_udl};

fuzz_target!(|data: &[u8]| {
    let Ok(udl) = std::str::from_utf8(data) else { return };
    if let Ok(files) = generate_from_udl(udl, "fuzz", &Config::default()) {
        assert!(files.source.contains("int luaopen_fuzz(lua_State *L) {"));
        assert_eq!(files.name, "fuzz.c");
    }
});
