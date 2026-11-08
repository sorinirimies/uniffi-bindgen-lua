//! End-to-end: the real binary, real arguments, real files.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_uniffi-bindgen-lua"))
}

#[test]
fn version_flag_prints_the_crate_version() {
    let out = bin().arg("--version").output().expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.trim(),
        format!("uniffi-bindgen-lua {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_argument_exits_with_usage_error() {
    let out = bin().arg("--nope").output().expect("run");
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn runtime_subcommand_writes_the_runtime_header() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin().arg("runtime").arg(dir.path()).output().expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let header = std::fs::read_to_string(dir.path().join("uniffi_lua.h")).unwrap();
    assert_eq!(header, uniffi_bindgen_lua::LUA_RUNTIME_HEADER);
}

#[test]
fn generate_with_unknown_source_kind_fails() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args(["generate", "Cargo.toml", "--out-dir"])
        .arg(dir.path())
        .output()
        .expect("run");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("neither a library nor a .udl file"));
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "nothing is written"
    );
}

#[test]
fn generate_from_udl_writes_header_source_and_runtime() {
    let dir = tempfile::tempdir().unwrap();
    // UDL mode looks for the crate around the file.
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let udl = dir.path().join("src").join("demo.udl");
    std::fs::write(&udl, "namespace demo { u32 twice(u32 n); };").unwrap();
    let out_dir = dir.path().join("out");
    let out = bin()
        .arg("generate")
        .arg(&udl)
        .arg("--out-dir")
        .arg(&out_dir)
        .args(["--crate", "demo", "--no-format"])
        .output()
        .expect("run");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let header = std::fs::read_to_string(out_dir.join("demo.c")).unwrap();
    assert!(header.contains("int luaopen_demo(lua_State *L) {"));
    assert!(out_dir.join("uniffi_lua.h").is_file());
    assert!(out_dir.join("uniffi_runtime.h").is_file());
}
