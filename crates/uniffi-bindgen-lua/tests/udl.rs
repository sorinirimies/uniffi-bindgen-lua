//! Generate from UDL and check the shape of the output. The compile check builds
//! the module against Lua's headers ($LUA_INCLUDE, else the usual locations) and
//! is skipped when there are none.

use std::path::{Path, PathBuf};
use std::process::Command;

use uniffi_bindgen_lua::{
    Config, GeneratedFiles, LUA_RUNTIME_HEADER, LUA_RUNTIME_HEADER_NAME, RUNTIME_HEADER,
    RUNTIME_HEADER_NAME, generate_from_udl,
};

const UDL: &str = r#"
namespace sample {
    u32 add(u32 a, u32 b);
    [Throws=Oops] string risky(string s);
    Shape shape_of(Point p);
    [Async] string fetch(string key);
    sequence<string?> names();
    record<string, u32> counts();
    bytes reverse(bytes data);
    u32 lend([ByRef] bytes data);
    void use_sink(Sink sink);
};

[Error]
enum Oops { "Bad", "Worse" };

enum Color { "Red", "Green" };

dictionary Point {
    double x;
    double y = 1.5;
    string? label = null;
    Color color = "Green";
};

[Enum]
interface Shape {
    Circle(Point center, double radius);
    Empty();
};

interface Thing {
    constructor(string name);
    [Name=other] constructor(u32 n);
    string name();
    [Throws=Oops] void check();
    [Async] u32 later();
};

callback interface Sink {
    void push(string s);
    [Throws=Oops] u32 count();
};
"#;

fn generate() -> GeneratedFiles {
    generate_from_udl(UDL, "sample", &Config::default()).expect("generates")
}

#[test]
fn module_registers_functions_types_and_poll() {
    let c = generate().source;
    for expected in [
        "#include \"uniffi_lua.h\"",
        "int luaopen_sample(lua_State *L) {",
        "lua_setfield(L, -2, \"add\");",
        "lua_setfield(L, -2, \"poll\");",
        "luaL_newmetatable(L, \"sample.Thing\");",
        "lua_setfield(L, -2, \"Thing\");",
        "lua_setfield(L, -2, \"other\");",
        "luaL_newmetatable(L, \"sample.Oops\");",
        "lua_pushcfunction(L, uniffi_lua_error_tostring);",
    ] {
        assert!(c.contains(expected), "lacks `{expected}`");
    }
}

// Regression: each module had its own callback queue, so a Lua program waiting in one
// component (a client's connect) never ran the callbacks Rust made into another (the
// broker's auth provider), and both waited (fixtures/runtime/dispatcher.c).
#[test]
fn modules_of_one_lua_state_share_the_dispatcher() {
    let c = generate().source;
    assert!(c.contains("static uniffi_lua_dispatcher *sample_shared_dispatcher;"));
    assert!(c.contains("sample_shared_dispatcher = uniffi_lua_shared_dispatcher(L);"));
    assert!(!c.contains("uniffi_lua_dispatcher_init"));
}

#[test]
fn arguments_are_checked_before_any_rust_resource_exists() {
    let c = generate().source;
    let f = &c[c.find("static int sample_fn_risky(").expect("risky")..];
    let check = f.find("uniffi_lua_check_lstring").expect("check");
    let lower = f.find("uniffi_buffer_from").expect("lower");
    assert!(
        check < lower,
        "a Lua error after allocating a RustBuffer would leak it"
    );
}

#[test]
fn record_defaults_fill_missing_fields() {
    let c = generate().source;
    assert!(c.contains("uniffi_write_f64(w, 1.5);"));
    assert!(
        c.contains("uniffi_write_i32(w, 2);"),
        "Color default Green (index 2)"
    );
}

// Regression: Lua arrays cannot hold nil; a sequence of optionals carries `n`.
#[test]
fn sequence_of_optionals_carries_its_length() {
    let c = generate().source;
    assert!(c.contains("lua_setfield(L, -2, \"n\");"));
}

#[test]
fn borrowed_bytes_are_lent_without_a_copy() {
    let c = generate().source;
    assert!(c.contains("f0.data = (const uint8_t *)s0;"));
}

#[test]
fn config_omit_checksums_drops_the_check() {
    let with = generate().source;
    let config = Config {
        omit_checksums: true,
        ..Config::default()
    };
    let without = generate_from_udl(UDL, "sample", &config)
        .expect("generates")
        .source;
    assert!(with.contains("API checksum mismatch"));
    assert!(!without.contains("API checksum mismatch"));
}

fn lua_include() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("LUA_INCLUDE") {
        return Some(PathBuf::from(dir));
    }
    [
        "/usr/include/lua5.4",
        "/usr/include/lua5.5",
        "/opt/homebrew/opt/lua/include/lua",
        "/usr/local/opt/lua/include/lua",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|d| d.join("lua.h").exists())
}

/// Compiles the module (C11, warnings are errors). Passes vacuously without Lua headers.
fn compiles(files: &GeneratedFiles) -> bool {
    let Some(include) = lua_include() else {
        eprintln!("no Lua headers; skipping the compile check");
        return true;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let write = |p: &Path, s: &str| std::fs::write(p, s).expect("write");
    write(&dir.path().join(RUNTIME_HEADER_NAME), RUNTIME_HEADER);
    write(
        &dir.path().join(LUA_RUNTIME_HEADER_NAME),
        LUA_RUNTIME_HEADER,
    );
    write(&dir.path().join(&files.name), &files.source);
    let out = Command::new("cc")
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Wpedantic",
            "-Werror",
            "-c",
        ])
        .arg(dir.path().join(&files.name))
        .arg("-I")
        .arg(dir.path())
        .arg("-I")
        .arg(include)
        .arg("-o")
        .arg(dir.path().join("out.o"))
        .output()
        .expect("run cc");
    if !out.status.success() {
        eprintln!("{}", String::from_utf8_lossy(&out.stderr));
    }
    out.status.success()
}

#[test]
fn generated_code_compiles() {
    assert!(compiles(&generate()), "generated code does not compile");
}

#[test]
fn empty_namespace_generates_compilable_code() {
    assert!(compiles(
        &generate_from_udl("namespace empty {};", "empty", &Config::default()).expect("generates")
    ));
}

// Regression: an empty record's converters did not use their writer/reader,
// which -Werror rejects.
#[test]
fn empty_record_compiles() {
    let udl = "namespace empty_rec { Nothing id(Nothing n); }; dictionary Nothing {};";
    assert!(compiles(
        &generate_from_udl(udl, "empty_rec", &Config::default()).expect("generate")
    ));
}

#[test]
fn invalid_udl_is_an_error() {
    assert!(generate_from_udl("namespace broken { void f(", "broken", &Config::default()).is_err());
}

#[test]
fn generated_snapshot() {
    insta::assert_snapshot!("sample", generate().source);
}
