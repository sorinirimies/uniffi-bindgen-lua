# ── uniffi-bindgen-lua · Lua toolchain helpers ─────────────────────────────
# Shared by lua_e2e.nu, fixtures.nu and fuzz_lua.nu.

# File name of the shared library `name` on `os`.
export def library_file [name: string, os: string] {
    match $os {
        "macos" => $"lib($name).dylib"
        "windows" => $"($name).dll"
        _ => $"lib($name).so"
    }
}

# The Lua flavours to test: their include directory and interpreter, from
# $LUA_INCLUDE / $LUAJIT_INCLUDE, else pkg-config, else the Homebrew layout.
export def flavours [] {
    let candidates = [
        {name: "lua", env: "LUA_INCLUDE", pkg: ["lua5.4" "lua5.5" "lua"], bin: ["lua5.4" "lua5.5" "lua"], dirs: ["/usr/include/lua5.4" "/usr/include/lua5.5" "/opt/homebrew/opt/lua/include/lua" "/usr/local/opt/lua/include/lua"]}
        {name: "luajit", env: "LUAJIT_INCLUDE", pkg: ["luajit"], bin: ["luajit"], dirs: ["/usr/include/luajit-2.1" "/opt/homebrew/opt/luajit/include/luajit-2.1" "/usr/local/opt/luajit/include/luajit-2.1"]}
    ]
    $candidates | each { |c|
        let bin = ($c.bin | where { |b| (which $b | is-not-empty) } | get 0? | default "")
        let from_env = ($env | get --optional $c.env | default "")
        let include = if $from_env != "" { $from_env } else { pkg_include $c.pkg $c.dirs }
        if $bin == "" or $include == "" { null } else { {name: $c.name, include: $include, bin: $bin} }
    } | compact
}

# The include directory of the first of `names` pkg-config knows, else the first
# of `dirs` holding lua.h.
export def pkg_include [names: list<string>, dirs: list<string>] {
    if (which pkg-config | is-empty) {
        return ($dirs | where { |d| ($d | path join lua.h | path exists) } | get 0? | default "")
    }
    for n in $names {
        let r = (do { ^pkg-config --cflags-only-I $n } | complete)
        if $r.exit_code == 0 {
            let dir = ($r.stdout | str trim | split row " " | where { |f| $f starts-with "-I" } | get 0? | default "" | str replace "-I" "")
            if $dir != "" { return $dir }
        }
    }
    $dirs | where { |d| ($d | path join lua.h | path exists) } | get 0? | default ""
}

# Compiler flags: C11, all warnings, warnings are errors.
export def flags [] {
    ["-std=c11" "-Wall" "-Wextra" "-Wpedantic" "-Werror" "-g"]
}

# Arguments building `source` into the Lua module `output` (a shared library)
# linked against the Rust library `name` in `lib_dir`.
export def module_args [source: string, include: string, out_dir: string, lib_dir: string, name: string, output: string, os: string] {
    let platform = if $os == "macos" { ["-shared" "-undefined" "dynamic_lookup"] } else { ["-shared" "-fPIC" "-lpthread"] }
    [...(flags) ...$platform $"-I($out_dir)" $"-I($include)" $source $"-L($lib_dir)" $"-l($name)" $"-Wl,-rpath,($lib_dir)" "-o" $output]
}

# The module file name: `<name>.so` (Lua's default cpath uses .so on macOS too).
export def module_file [name: string] {
    $"($name).so"
}
