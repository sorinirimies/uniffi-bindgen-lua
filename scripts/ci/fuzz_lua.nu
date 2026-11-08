#!/usr/bin/env nu
# ── uniffi-bindgen-lua · fuzz the generated converters ─────────────────────
#
# Generates the tour fixture's Lua module and runs fuzz/lua/fuzz_codecs.c (which
# embeds Lua) under ASan and UBSan: with libFuzzer for --secs seconds when clang
# has it, else with the pseudo-random driver.
#
# Usage: nu scripts/ci/fuzz_lua.nu [--secs 60]

use lua.nu

const FIXTURE = "fixtures/tour"

# The linker arguments of the Lua library next to `include` (…/include/lua → …/lib).
export def lua_lib_args [include: string, os: string] {
    let prefix = ($include | path dirname | path dirname)
    let lib = ($prefix | path join lib)
    let names = if $os == "macos" { ["lua"] } else { ["lua5.4" "lua5.5" "lua"] }
    let candidates = ([$lib "/usr/lib/x86_64-linux-gnu" "/usr/lib/aarch64-linux-gnu"] | each { |d| $names | each { |n| {dir: $d, name: $n} } } | flatten)
    let found = ($candidates | where { |c| (glob $"($c.dir)/lib($c.name).*" | is-not-empty) } | get 0?)
    if $found == null { error make {msg: $"no Lua library near ($include)"} }
    [$"-L($found.dir)" $"-l($found.name)" "-lm"]
}

def main [--secs: int = 60] {
    let root = ($env.FILE_PWD | path dirname | path dirname)
    cd $root
    let flavour = (lua flavours | where name == "lua" | get 0?)
    if $flavour == null { error make {msg: "no Lua 5.x with headers found"} }
    ^cargo build --quiet --manifest-path $"($FIXTURE)/Cargo.toml"
    let lib_dir = ($root | path join $FIXTURE target debug)
    let gen = (mktemp --directory)
    ^cargo run --quiet -p uniffi-bindgen-lua -- generate ($lib_dir | path join (lua library_file "tour" $nu.os-info.name)) --out-dir $gen --no-format --metadata-no-deps
    let compiler = ($env.CC? | default "clang")
    let exe = ($gen | path join fuzz_codecs)
    let common = ["-std=c11" "-O1" "-g" "-fno-sanitize-recover=all" $"-I($gen)" $"-I($flavour.include)" "fuzz/lua/fuzz_codecs.c" $"-L($lib_dir)" "-ltour" $"-Wl,-rpath,($lib_dir)" ...(lua_lib_args $flavour.include $nu.os-info.name) "-o" $exe]
    let libfuzzer = (do { ^$compiler "-fsanitize=fuzzer,address,undefined" ...$common } | complete).exit_code == 0
    if $libfuzzer {
        let corpus = ($gen | path join corpus)
        mkdir $corpus
        ^$exe $corpus $"-max_total_time=($secs)" "-max_len=4096"
    } else {
        print -e "▶ no libFuzzer: using the pseudo-random driver"
        ^$compiler "-fsanitize=address,undefined" "-DUNIFFI_FUZZ_SMOKE" ...$common
        ^$exe "300000"
    }
    print -e "✔ fuzzing the generated converters found nothing"
}
