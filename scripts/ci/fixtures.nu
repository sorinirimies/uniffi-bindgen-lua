#!/usr/bin/env nu
# ── uniffi-bindgen-lua · uniffi fixture suite ────────────────────────────────
#
# Runs uniffi's own test fixtures (fixtures/uniffi, vendored from uniffi-rs)
# against the Lua bindings:
#   1. builds every fixture crate,
#   2. generates the Lua module of every cdylib, builds it for every Lua found
#      (warnings are errors) and loads it with `require`,
#   3. runs each fixture's lua/test_*.lua.
#
# Usage: nu scripts/ci/fixtures.nu [--only <cdylib name>]

use lua.nu

const FIXTURES = "fixtures/uniffi"

# Libraries we only build: upstream does not generate bindings for them either.
export const SKIP = [
    # Its UDL lives in another crate, so library mode cannot find the UDL types.
    "uniffi_proc_macro_nip"
]

# The cdylib targets in `cargo metadata` output: name and package directory.
export def cdylibs [metadata: record] {
    $metadata.packages | each { |p|
        $p.targets | where { |t| "cdylib" in $t.crate_types } | each { |t|
            {name: $t.name, dir: ($p.manifest_path | path dirname)}
        }
    } | flatten | sort-by name
}

# The libraries to run: all but SKIP, or just `only`.
export def selected [libs: list, only: string] {
    $libs | where { |l| $l.name not-in $SKIP and ($only == "" or $l.name == $only) }
}

def run_library [lib: record, bin: string, lib_dir: string, out_root: string, flavour: record, compiler: string] {
    let out = ($out_root | path join $"($flavour.name)-($lib.name)")
    mkdir $out
    let library = ($lib_dir | path join (lua library_file $lib.name $nu.os-info.name))
    # Library mode finds UDL files through `cargo metadata`, so run inside the fixture workspace.
    let gen = (do { cd $lib.dir; ^$bin generate $library --out-dir $out --no-format } | complete)
    if $gen.exit_code != 0 { return [{library: $lib.name, step: "generate", detail: $gen.stderr}] }
    mut failures = []
    let modules = (glob $"($out)/*.c" | path parse | get stem)
    for m in $modules {
        let built = (do { ^$compiler ...(lua module_args ($out | path join $"($m).c") $flavour.include $out $lib_dir $lib.name ($out | path join (lua module_file $m)) $nu.os-info.name) } | complete)
        if $built.exit_code != 0 {
            $failures = ($failures | append {library: $lib.name, step: $"($flavour.name): build ($m)", detail: $built.stderr})
        }
    }
    if ($failures | is-not-empty) { return $failures }
    # Modules that use another's types need it loaded first: load in dependency-free order by retrying.
    let load = $"local pending = {($modules | each { |m| $'"($m)"' } | str join ', ')}
for _ = 1, #pending do for i = #pending, 1, -1 do if pcall\(require, pending[i]\) then table.remove\(pending, i\) end end end
if #pending > 0 then local ok, err = pcall\(require, pending[1]\) error\(err\) end"
    let loaded = (do { cd $out; ^$flavour.bin -e $load } | complete)
    if $loaded.exit_code != 0 {
        return [{library: $lib.name, step: $"($flavour.name): require", detail: $"($loaded.stdout)($loaded.stderr)"}]
    }
    for test in (glob $"($lib.dir)/lua/test_*.lua") {
        cp $test $out
        let name = ($test | path basename)
        let r = (do { cd $out; ^$flavour.bin $name } | complete)
        if $r.exit_code != 0 {
            $failures = ($failures | append {library: $lib.name, step: $"($flavour.name): run ($name)", detail: $"($r.stdout)($r.stderr)"})
        } else {
            print -e $"  ($flavour.name) ($lib.name)/($name): ($r.stdout | str trim | lines | last)"
        }
    }
    $failures
}

def main [--only: string = ""] {
    let root = ($env.FILE_PWD | path dirname | path dirname)
    cd $root
    let manifest = ($FIXTURES | path join Cargo.toml)
    print -e "▶ building the fixtures"
    ^cargo build --quiet --manifest-path $manifest
    ^cargo build --quiet -p uniffi-bindgen-lua
    let bin = ($root | path join target debug uniffi-bindgen-lua)
    let lib_dir = ($root | path join $FIXTURES target debug)
    let libs = (selected (cdylibs (^cargo metadata --format-version 1 --no-deps --manifest-path $manifest | from json)) $only)
    let compiler = ($env.CC? | default "cc")
    let flavours = (lua flavours)
    if ($flavours | is-empty) { error make {msg: "no Lua with headers found (set LUA_INCLUDE / LUAJIT_INCLUDE)"} }
    let out_root = (mktemp --directory)

    print -e $"▶ generating, building and loading ($libs | length) libraries for ($flavours | get name | str join ', ')"
    let failures = ($flavours | each { |f| $libs | each { |lib| run_library $lib $bin $lib_dir $out_root $f $compiler } | flatten } | flatten)
    if ($failures | is-not-empty) {
        for f in $failures {
            print -e $"✗ ($f.library): ($f.step)\n($f.detail | lines | first 30 | str join "\n")"
        }
        error make {msg: $"($failures | length) fixture steps failed"}
    }
    print -e "✔ all fixtures passed"
}
