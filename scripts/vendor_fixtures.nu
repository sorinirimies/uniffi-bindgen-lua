#!/usr/bin/env nu
# ── uniffi-bindgen-lua · vendor uniffi's fixtures ────────────────────────────
#
# Replaces fixtures/uniffi with the fixtures and examples of a uniffi-rs tag,
# trimmed to what the Lua tests need: their tests in other languages,
# dev-dependencies, binaries, benches and the ffi-trace feature go; path
# dependencies follow the flatter layout (fixtures/x → x). Kept across updates:
# every `lua/` dir, the `[bindings.lua…]` sections of uniffi.toml files, and
# crates that upstream removed (they are marked in the workspace manifest).
#
# Usage: nu scripts/vendor_fixtures.nu <uniffi-rs checkout> <tag>

const TARGET = "fixtures/uniffi"

# Upstream crates that are not binding tests, or test only another language
# or target.
const SKIP = [
    "examples/app/uniffi-bindgen-cli"
    "fixtures/metadata"
    "fixtures/multi-arch-extract"
    "fixtures/swift-bridging-header-compile"
    "fixtures/swift-codable"
    "fixtures/swift-link-frameworks"
    "fixtures/swift-mock-objects"
    "fixtures/swift-omit-labels"
    "fixtures/uitests"
    "fixtures/version-mismatch"
    "fixtures/wasm-unstable-single-threaded"
]

# Files of other languages' tests.
const FOREIGN_TESTS = ["py" "kts" "kt" "swift" "rb" "sh" "gradle"]

# Where an upstream crate dir lives here.
export def local_dir [upstream: string] {
    $upstream | str replace --regex '^fixtures/' ''
}

# Joins a relative path onto a dir, resolving `..` without the file system.
export def join_relative [dir: string, relative: string] {
    $relative | path split | reduce --fold ($dir | path split) {|part, acc|
        if $part == ".." { $acc | drop 1 } else if $part == "." { $acc } else { $acc | append $part }
    } | path join
}

# Rewrites a path dependency of the crate at `upstream` for the local layout.
export def local_path [upstream: string, dep: string] {
    let from = (local_dir $upstream | path split)
    let to = (local_dir (join_relative $upstream $dep) | path split)
    let common = ($from | zip $to | take while {|pair| $pair.0 == $pair.1 } | length)
    let up = ($from | skip $common | each { ".." })
    [...$up ...($to | skip $common)] | path join
}

# The manifest of a vendored crate.
export def trim_manifest [manifest: record, upstream: string] {
    let fix_paths = {|deps|
        mut out = {}
        for dep in ($deps | transpose name spec) {
            let spec = if ($dep.spec | describe | str starts-with "record") and ("path" in $dep.spec) {
                let target = (join_relative $upstream $dep.spec.path)
                if ($target | str starts-with "fixtures/") or ($target | str starts-with "examples/") {
                    $dep.spec | upsert path (local_path $upstream $dep.spec.path)
                } else if $dep.name == "uniffi" {
                    # uniffi itself, from the repo: the released version instead.
                    $dep.spec | reject path | insert workspace true
                } else {
                    error make {msg: $"($upstream): path dependency ($dep.name) outside the fixtures"}
                }
            } else { $dep.spec }
            $out = ($out | upsert $dep.name $spec)
        }
        $out
    }
    mut m = ($manifest | reject --optional dev-dependencies bin bench)
    for table in [dependencies build-dependencies] {
        if $table in $m {
            $m = ($m | upsert $table (do $fix_paths ($m | get $table)))
        }
    }
    if "features" in $m {
        let features = ($m.features | reject --optional ffi-trace)
        $m = if ($features | is-empty) { $m | reject features } else { $m | upsert features $features }
    }
    $m
}

# The `[bindings.lua]` / `[bindings.lua.…]` sections of a uniffi.toml, with
# comments just above them.
export def lua_sections [text: string] {
    let lines = ($text | lines)
    mut out = []
    mut keep = false
    mut pending = []
    for line in $lines {
        if ($line | str starts-with "[") {
            $keep = ($line =~ '^\[bindings\.lua[\].]')
            if $keep { $out = ($out | append $pending) }
            $pending = []
        }
        if $keep {
            $out = ($out | append $line)
        } else if ($line | str starts-with "#") or ($line | str trim | is-empty) {
            $pending = ($pending | append $line)
        } else {
            $pending = []
        }
    }
    $out | str join "\n" | str trim
}

# Crate dirs (relative to the repo) at `tag`.
def upstream_crates [repo: string, tag: string] {
    ^git -C $repo ls-tree -r --name-only $tag fixtures examples
    | lines
    | where ($it | str ends-with "/Cargo.toml")
    | each { path dirname }
    | where $it not-in $SKIP
}

def vendor_crate [repo: string, tag: string, upstream: string] {
    let dir = ($TARGET | path join (local_dir $upstream))
    let lua_dir = ($dir | path join lua)
    let saved = (mktemp --directory)
    if ($lua_dir | path exists) { cp -r $lua_dir $saved }
    let config = ($dir | path join uniffi.toml)
    let sections = if ($config | path exists) { lua_sections (open --raw $config) } else { "" }

    rm -rf $dir
    mkdir $dir
    let depth = ($upstream | path split | length)
    ^git -C $repo archive $tag $upstream | ^tar -x -C $dir $"--strip-components=($depth)"
    for junk in ["tests" "benches" "examples" "android" "bindings" "src/bin.rs"] {
        rm -rf ($dir | path join $junk)
    }
    for ext in $FOREIGN_TESTS {
        glob $"($dir)/**/*.($ext)" | each { rm $in }
    }
    let manifest = ($dir | path join Cargo.toml)
    trim_manifest (open $manifest) $upstream | to toml | save --force $manifest
    if ($saved | path join lua | path exists) { cp -r ($saved | path join lua) $dir }
    if ($sections | is-not-empty) {
        let base = if ($config | path exists) { open --raw $config | str trim } else { "" }
        $"($base)\n\n($sections)\n" | str trim --left | save --force $config
    }
    rm -rf $saved
}

def main [repo: string, tag: string] {
    let root = ($env.FILE_PWD | path dirname)
    cd $root
    let version = ($tag | str replace --regex '^v' '')
    let upstream = (upstream_crates $repo $tag)
    let current = (open ($TARGET | path join Cargo.toml) | get workspace.members)
    for crate in $upstream {
        print -e $"▶ ($crate)"
        vendor_crate $repo $tag $crate
    }
    let fresh = ($upstream | each { local_dir $in })
    let removed = ($current | where $it not-in $fresh)
    let members = ($fresh | append $removed | sort)
    let header = [
        $"# uniffi-rs ($tag) test fixtures and examples \(MPL-2.0,"
        $"# https://github.com/mozilla/uniffi-rs/tree/($tag)\),"
        "# vendored by scripts/vendor_fixtures.nu to run their tests against the Lua"
        "# bindings. Each fixture's `lua/test_*.lua` ports the upstream test named at"
        "# its top. Changes from upstream: the test harness and the per-language"
        "# tests, dev-dependencies, the ffi-trace feature, benches and binaries are"
        "# removed."
        ...(if ($removed | is-empty) { [] } else {
            [$"# Kept from an earlier uniffi-rs version \(no longer upstream\): ($removed | str join ', ')."]
        })
    ]
    let workspace = {
        workspace: {resolver: "2", members: $members, dependencies: {uniffi: $"=($version)"}}
    }
    $"($header | str join "\n")\n($workspace | to toml)" | save --force ($TARGET | path join Cargo.toml)
    print -e $"✔ vendored ($upstream | length) crates from uniffi-rs ($tag)"
}
