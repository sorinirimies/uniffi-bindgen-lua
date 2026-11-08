# uniffi-bindgen-lua

[![CI](https://github.com/sorinirimies/uniffi-bindgen-lua/actions/workflows/ci.yml/badge.svg)](https://github.com/sorinirimies/uniffi-bindgen-lua/actions/workflows/ci.yml)
[![Security](https://github.com/sorinirimies/uniffi-bindgen-lua/actions/workflows/security.yml/badge.svg)](https://github.com/sorinirimies/uniffi-bindgen-lua/actions/workflows/security.yml)
[![Fuzz](https://github.com/sorinirimies/uniffi-bindgen-lua/actions/workflows/fuzz.yml/badge.svg)](https://github.com/sorinirimies/uniffi-bindgen-lua/actions/workflows/fuzz.yml)
[![crates.io](https://img.shields.io/crates/v/uniffi-bindgen-lua.svg)](https://crates.io/crates/uniffi-bindgen-lua)

Lua bindings generator for [UniFFI](https://mozilla.github.io/uniffi-rs/) 0.32.

Point it at a library built with UniFFI and it writes, per component, the C source of a
Lua module on the Lua C API. Build it into a shared library with the Rust library linked
in and `require` it: from Lua 5.1 to 5.5 and LuaJIT, so from Neovim, OpenResty, Redis,
game engines or OpenWrt.

* **Plain Lua values:** tables for records, strings for enums, `{tag = "Circle", ...}`
  for enums with data, userdata with methods for objects, errors you catch with `pcall`.
* **Complete:** every fixture of uniffi's own test suite builds (warnings are errors) and
  loads with Lua 5.4/5.5 and LuaJIT; the tour test covers objects, traits, callbacks in
  both directions, async, errors, recursive types, sets and lent byte strings.
* **Safe with threads:** Lua states are single-threaded and Rust is not; callbacks Rust
  makes from its own threads are run on the Lua thread.
* **Hardened conversions:** what comes from Rust is bounds-checked; the generated
  converters are fuzzed (libFuzzer, ASan, UBSan) to round-trip byte for byte.

## Install

```sh
cargo install uniffi-bindgen-lua
```

| uniffi-bindgen-lua | UniFFI | Lua |
|---|---|---|
| 0.1 | 0.32 | 5.1 (LuaJIT) – 5.5 |

Use the release that matches the `uniffi` version of the Rust library (a library built
with another version fails at generation).

## Build a module

```sh
cargo build --release
uniffi-bindgen-lua generate target/release/libmylib.so --out-dir lua/

# Linux
cc -std=c11 -shared -fPIC -Ilua -I/usr/include/lua5.4 lua/mylib.c -Ltarget/release -lmylib -o mylib.so
# macOS
cc -std=c11 -shared -undefined dynamic_lookup -Ilua -I$(brew --prefix lua)/include/lua lua/mylib.c -Ltarget/release -lmylib -o mylib.so
```

```lua
local mylib = require("mylib")   -- finds mylib.so through package.cpath
```

`generate` also takes a `.udl` file (`--library` names the compiled library, `--crate` the
crate). `uniffi-bindgen-lua runtime <dir>` writes just the two runtime headers.

## The generated API

Names are kept as written in Rust: `tour.add_u8`, `tour.Counter.new`, `counter:get()`.

| Rust | Lua |
|---|---|
| integers | integers (range-checked; `u64` above 2^63 wraps as `math.ult` expects; doubles on 5.1) |
| `f32`, `f64`, `bool` | numbers, booleans |
| `String`, `Vec<u8>` | strings (`&[u8]` arguments are lent to Rust without a copy) |
| `SystemTime`, `Duration` | seconds since 1970 / seconds, as numbers |
| `Option<T>` | the value or `nil` |
| `Vec<T>`, `HashSet<T>` | array tables (a sequence of optionals carries `n`, as `table.pack` does) |
| `HashMap<K, V>` | tables keyed by `K` |
| record | table; missing fields take Rust's defaults |
| enum without data | the variant's name: `"Red"` |
| enum with data | `{tag = "Circle", center = {...}, radius = 2}` (unnamed fields at `[1]`, `[2]`) |
| object | userdata: `Type.new(...)`, `obj:method(...)`, released by the GC |
| `Display`, `Eq`, `Ord`, `Hash`, `Debug` | `tostring`, `==`, `<`/`<=`, `:hash()`, `:debug_string()` |
| callback interface | any table with the methods |
| trait Lua may implement | `Type.from_impl(table)` |

```lua
local tour = require("tour")

local counter = tour.Counter.new(10)
counter:add(5)
print(tostring(counter))                      -- Counter(15), Rust's Display

local ok, err = pcall(tour.fail_data, 1)
print(err.tag, err.code)                      -- BadCode 42

print(tour.notify({                           -- a callback interface
    on_event = function(self, name, value) return value * 10 end,
    on_done = function(self) end,
    fallible = function(self, n) return "ok" end,
}, 4))
```

### Errors

A Rust error is raised as a table: `tag` is the variant, its fields are alongside, and
flat errors carry `message`; `tostring` gives `"Variant"` or `"Variant: message"`. Panics
raise `"rust panic: ..."`. Arguments of the wrong type raise before Rust is called. A Lua
implementation fails with `error({tag = "Variant", ...})` for the declared error type;
any other error reaches Rust as an unexpected callback error with its message.

### Threads and callbacks

The module serves the Lua state and thread that loaded it. Rust may call a callback on
another thread: such calls run on the Lua thread at the next call into the module, in
`<module>.poll()`, or while a call waits for an async result. A callback that returns
nothing does not make Rust wait; one that returns something blocks Rust's thread until the
Lua thread runs it, so call `poll()` regularly when Rust calls back from its own threads.

### Async

Async functions block until Rust's future completes, running queued callbacks meanwhile.
Async methods implemented in Lua are plain functions.

## Limits

* A `HashMap` with optional values drops the `None` entries (a Lua table cannot hold nil).
* One Lua state per loaded module.
* Function arguments have no defaults: pass every argument.
* Windows is not tested in CI.

## Development

```sh
just check-all    # fmt, clippy, tests, docs, Nushell script tests
just e2e          # build the tour fixture's module for every Lua found and run its test
just fixtures     # uniffi's fixture suite: generate, build, require (--only <lib>)
just fuzz-lua     # fuzz the generated converters
just vendor-fixtures ~/uniffi-rs v0.32.2
just release-all 0.1.0
```

## License

MIT
