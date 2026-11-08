#!/usr/bin/env nu
# ── fresh install: build the published crates the way `cargo install` does ─────
# CI builds with Cargo.lock; `cargo install` and library users resolve the
# dependencies afresh, from the packaged crate. A requirement that only works
# with the lockfile (v0.1.0 of uniffi-bindgen-lua asked for toml 0.9 while
# uniffi_bindgen took toml 1.x: two `toml::Value`s, and the crate did not
# compile) passes CI and breaks every install. So: package each crate in
# PUBLISH, unpack it, drop its lockfile, and build it with the newest
# dependencies its requirements allow.
#
# Usage: nu scripts/ci/fresh_install.nu

use ../project.nu [PUBLISH]

# The `.crate` file `cargo package` writes for `crate` at `version`.
export def crate_file [target_dir: string, crate: string, version: string]: nothing -> string {
    $target_dir | path join "package" $"($crate)-($version).crate"
}

# The version of `crate` in the workspace, from `cargo metadata`.
export def crate_version [metadata: record, crate: string]: nothing -> string {
    let found = ($metadata.packages | where name == $crate)
    if ($found | is-empty) { error make --unspanned { msg: $"($crate) is not in this workspace" } }
    $found | first | get version
}

def main [] {
    let root = ($env.FILE_PWD | path dirname | path dirname)
    cd $root
    let metadata = (^cargo metadata --no-deps --format-version 1 | from json)
    for crate in $PUBLISH {
        let version = (crate_version $metadata $crate)
        print -e $"▶ ($crate) ($version): package, drop Cargo.lock, resolve afresh, build"
        ^cargo package --quiet --no-verify --allow-dirty -p $crate
        let work = (mktemp --directory)
        ^tar -xzf (crate_file $metadata.target_directory $crate $version) -C $work
        let unpacked = ($work | path join $"($crate)-($version)")
        rm -f ($unpacked | path join "Cargo.lock")
        do { cd $unpacked; ^cargo build --quiet --target-dir ($work | path join "target") }
        rm -rf $work
    }
    print -e "✔ the published crates build with fresh dependencies"
}
