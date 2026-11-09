#!/usr/bin/env nu
# ── uniffi-bindgen-lua · Lua end-to-end test ───────────────────────────────
#
# Builds the `tour` fixture crate, generates its Lua module, builds it for every
# Lua found (Lua 5.x and LuaJIT, see lua.nu) and runs
# fixtures/tour/lua/test_tour.lua with each; then the runtime's own regression
# tests (fixtures/runtime/), which need no Rust library.
#
# Usage: nu scripts/ci/lua_e2e.nu

const FIXTURE = "fixtures/tour"

use lua.nu

def main [] {
    let root = ($env.FILE_PWD | path dirname | path dirname)
    cd $root
    let flavours = (lua flavours)
    if ($flavours | is-empty) { error make {msg: "no Lua with headers found (set LUA_INCLUDE / LUAJIT_INCLUDE)"} }
    print -e "▶ building the tour fixture"
    ^cargo build --manifest-path $"($FIXTURE)/Cargo.toml"
    let lib_dir = ($root | path join $FIXTURE target debug)
    let lib = ($lib_dir | path join (lua library_file "tour" $nu.os-info.name))
    let compiler = ($env.CC? | default "cc")
    for f in $flavours {
        let out = (mktemp --directory)
        ^cargo run --quiet -p uniffi-bindgen-lua -- generate $lib --out-dir $out --no-format --metadata-no-deps
        cp $"($FIXTURE)/lua/test_tour.lua" $out
        print -e $"▶ ($f.name): building the module"
        ^$compiler ...(lua module_args ($out | path join tour.c) $f.include $out $lib_dir "tour" ($out | path join (lua module_file "tour")) $nu.os-info.name)
        print -e $"▶ ($f.name): running"
        do { cd $out; ^$f.bin test_tour.lua }
        print -e $"▶ ($f.name): runtime tests"
        let rt = (mktemp --directory)
        cp fixtures/runtime/test_dispatcher.lua $rt
        ^$compiler ...(lua runtime_module_args ($root | path join fixtures runtime dispatcher.c) $f.include ($root | path join crates uniffi-bindgen-lua src runtime) ($rt | path join (lua module_file "dispatcher")) $nu.os-info.name)
        do { cd $rt; ^$f.bin test_dispatcher.lua }
    }
    print -e "✔ Lua end-to-end test passed"
}
