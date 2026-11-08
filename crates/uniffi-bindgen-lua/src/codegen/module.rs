//! The Lua-callable functions (functions, constructors, methods, metamethods)
//! and `luaopen_<namespace>`.

use std::fmt::Write;

use uniffi_bindgen::interface::{
    AsType, Callable, Constructor, Method, Object, Type, UniffiTraitMethods,
};

use super::names::{ident, string_literal};
use super::types::{Gen, int_info, type_name};

/// How a call passes its receiver.
pub(crate) enum Receiver {
    None,
    /// An object, and the method's name (for Lua-implemented objects; empty for
    /// trait metamethods, which those do not have).
    Object(Type, String),
    Value(Type),
}

/// One entry of a `luaL_Reg` table: the Lua name and the C function.
type Reg = (String, String);

impl Gen<'_> {
    /// A Lua-callable C function calling `c`.
    pub(crate) fn lua_function(
        &self,
        c: &dyn Callable,
        cname: &str,
        receiver: &Receiver,
        label: &str,
    ) -> String {
        let p = &self.prefix;
        let mut check = String::new();
        let mut lower = String::new();
        let mut args: Vec<String> = Vec::new();
        let offset = match receiver {
            Receiver::None => 0,
            _ => 1,
        };
        match receiver {
            Receiver::None => {}
            Receiver::Object(t, method) => {
                let m = self.mangle(t);
                let _ = writeln!(
                    check,
                    "    uint64_t self_handle = {p}_handle_{m}(L, 1, {});",
                    string_literal(&format!("{label}: self"))
                );
                if !method.is_empty()
                    && self
                        .object_def(t)
                        .is_some_and(Object::has_callback_interface)
                    && !self.ci.is_external(t)
                {
                    let _ = writeln!(
                        check,
                        "    if (self_handle & 1) {{\n        return {p}_{m}_call_impl(L, {});\n    }}",
                        string_literal(method)
                    );
                }
                let _ = writeln!(
                    lower,
                    "    uint64_t self_lowered = {p}_clone_{m}(self_handle);"
                );
                args.push("self_lowered".into());
            }
            Receiver::Value(t) => {
                let _ = writeln!(
                    check,
                    "    uniffi_writer *self_w = uniffi_lua_writer(L);\n    {{\n        uniffi_writer *w = self_w;\n        {}\n    }}",
                    self.encode(t, "1", &string_literal(&format!("{label}: self")))
                );
                let _ = writeln!(
                    lower,
                    "    RustBuffer self_lowered = uniffi_buffer_from(self_w->data, self_w->len);\n    uniffi_writer_free(self_w);"
                );
                args.push("self_lowered".into());
            }
        }
        for (i, a) in c.arguments().iter().enumerate() {
            let t = a.as_type();
            let idx = i + 1 + offset;
            let what = string_literal(&format!("{label}: argument {} ({})", i + 1, a.name()));
            let (chk, low, arg) = self.argument(&t, idx, &what, a.is_borrowed_bytes(), i);
            check.push_str(&chk);
            lower.push_str(&low);
            args.push(arg);
        }
        let mut call = String::new();
        let has_ret = c.return_type().is_some();
        let ret_ffi = c.return_type().map(|t| self.ffi_of(t)).unwrap_or_default();
        if c.is_async() {
            let _ = writeln!(
                call,
                "    uint64_t future = {}({});",
                c.ffi_func().name(),
                args.join(", ")
            );
            let _ = writeln!(
                call,
                "    uniffi_lua_block_on(&{p}_dispatcher, future, (uniffi_poll_fn){});",
                c.ffi_rust_future_poll(self.ci)
            );
            let _ = writeln!(call, "    RustCallStatus uniffi_call = {{0}};");
            let complete = c.ffi_rust_future_complete(self.ci);
            if has_ret {
                let _ = writeln!(
                    call,
                    "    {ret_ffi} result = {complete}(future, &uniffi_call);"
                );
            } else {
                let _ = writeln!(call, "    {complete}(future, &uniffi_call);");
            }
            let _ = writeln!(call, "    {}(future);", c.ffi_rust_future_free(self.ci));
        } else {
            let _ = writeln!(call, "    RustCallStatus uniffi_call = {{0}};");
            let mut all = args.clone();
            all.push("&uniffi_call".into());
            if has_ret {
                let _ = writeln!(
                    call,
                    "    {ret_ffi} result = {}({});",
                    c.ffi_func().name(),
                    all.join(", ")
                );
            } else {
                let _ = writeln!(call, "    {}({});", c.ffi_func().name(), all.join(", "));
            }
        }
        let raise_error = c.throws_type().map_or_else(
            || "NULL".to_owned(),
            |t| format!("{p}_push_error_{}", self.mangle(t)),
        );
        let _ = writeln!(
            call,
            "    if (uniffi_call.code != UNIFFI_CALL_SUCCESS) {{\n        return {p}_raise(L, &uniffi_call, {raise_error});\n    }}"
        );
        let ret = match c.return_type() {
            None => "    return 0;\n".to_owned(),
            Some(t) => format!("{}    return 1;\n", self.push_result(t, "result", label)),
        };
        format!(
            "static int {cname}(lua_State *L) {{\n    {p}_enter(L);\n{check}{lower}{call}{ret}}}\n\n"
        )
    }

    /// Checking (may raise), lowering (cannot) and the FFI argument of argument `i`.
    fn argument(
        &self,
        t: &Type,
        idx: usize,
        what: &str,
        borrowed: bool,
        i: usize,
    ) -> (String, String, String) {
        let p = &self.prefix;
        match t {
            Type::String if borrowed => unreachable_string(),
            Type::Bytes if borrowed => (
                format!(
                    "    size_t n{i};\n    const char *s{i} = uniffi_lua_check_lstring(L, {idx}, &n{i}, {what});\n"
                ),
                format!(
                    "    ForeignBytes f{i};\n    f{i}.len = (int32_t)n{i};\n    f{i}.data = (const uint8_t *)s{i};\n"
                ),
                format!("f{i}"),
            ),
            Type::String => (
                format!(
                    "    size_t n{i};\n    const char *s{i} = uniffi_lua_check_lstring(L, {idx}, &n{i}, {what});\n"
                ),
                format!("    RustBuffer b{i} = uniffi_buffer_from(s{i}, n{i});\n"),
                format!("b{i}"),
            ),
            Type::Custom { builtin, .. } => self.argument(builtin, idx, what, borrowed, i),
            Type::Boolean => (
                format!("    bool v{i} = uniffi_lua_check_bool(L, {idx}, {what});\n"),
                String::new(),
                format!("(int8_t)(v{i} ? 1 : 0)"),
            ),
            Type::Float32 => (
                format!("    float v{i} = (float)uniffi_lua_check_number(L, {idx}, {what});\n"),
                String::new(),
                format!("v{i}"),
            ),
            Type::Float64 => (
                format!("    double v{i} = uniffi_lua_check_number(L, {idx}, {what});\n"),
                String::new(),
                format!("v{i}"),
            ),
            t if int_info(t).is_some() => {
                let (bits, signed) = int_info(t).unwrap_or((64, true));
                let c = format!("{}int{bits}_t", if signed { "" } else { "u" });
                (
                    format!(
                        "    {c} v{i} = ({c})uniffi_lua_check_int(L, {idx}, {bits}, {signed}, {what});\n"
                    ),
                    String::new(),
                    format!("v{i}"),
                )
            }
            Type::Object { .. } => {
                let m = self.mangle(t);
                (
                    format!("    uint64_t h{i} = {p}_handle_{m}(L, {idx}, {what});\n"),
                    format!("    uint64_t o{i} = {p}_clone_{m}(h{i});\n"),
                    format!("o{i}"),
                )
            }
            Type::CallbackInterface { .. } => {
                let m = self.mangle(t);
                (
                    format!(
                        "    if (!lua_istable(L, {idx})) {{\n        luaL_error(L, \"%s: a table expected, got %s\", {what}, luaL_typename(L, {idx}));\n    }}\n"
                    ),
                    format!(
                        "    lua_pushvalue(L, {idx});\n    uint64_t c{i} = {p}_insert_{m}(luaL_ref(L, LUA_REGISTRYINDEX));\n"
                    ),
                    format!("c{i}"),
                )
            }
            _ => (
                format!(
                    "    uniffi_writer *w{i} = uniffi_lua_writer(L);\n    {{\n        uniffi_writer *w = w{i};\n        {}\n    }}\n",
                    self.encode(t, &idx.to_string(), what)
                ),
                format!(
                    "    RustBuffer b{i} = uniffi_buffer_from(w{i}->data, w{i}->len);\n    uniffi_writer_free(w{i});\n"
                ),
                format!("b{i}"),
            ),
        }
    }

    /// Pushes the FFI result `expr` of type `t`.
    fn push_result(&self, t: &Type, expr: &str, label: &str) -> String {
        let p = &self.prefix;
        match t {
            Type::Boolean => format!("    lua_pushboolean(L, {expr} != 0);\n"),
            Type::Float32 | Type::Float64 => {
                format!("    lua_pushnumber(L, (lua_Number){expr});\n")
            }
            Type::UInt64 => format!("    uniffi_lua_push_u64(L, {expr});\n"),
            t if int_info(t).is_some() => format!("    uniffi_lua_push_i64(L, (int64_t){expr});\n"),
            Type::Object { .. } | Type::CallbackInterface { .. } => {
                format!("    {p}_push_{}(L, {expr});\n", self.mangle(t))
            }
            Type::String => format!(
                "    lua_pushlstring(L, {expr}.data ? (const char *){expr}.data : \"\", (size_t){expr}.len);\n    uniffi_buffer_free({expr});\n"
            ),
            Type::Custom { builtin, .. } => self.push_result(builtin, expr, label),
            _ => format!(
                "    {{\n        uniffi_reader reader = uniffi_reader_new({expr}.data, (size_t){expr}.len);\n        uniffi_reader *r = &reader;\n        {}\n        bool bad = reader.failed || uniffi_reader_remaining(&reader) != 0;\n        uniffi_buffer_free({expr});\n        if (bad) {{\n            return luaL_error(L, \"%s: Rust sent a malformed value\", {});\n        }}\n    }}\n",
                self.decode(t),
                string_literal(label)
            ),
        }
    }

    /// The handle helpers of an object: push, check, clone, the `__gc`.
    pub(crate) fn emit_object_helpers(&self, t: &Type) -> String {
        let p = &self.prefix;
        let m = self.mangle(t);
        let mt = string_literal(&self.metatable_name(t));
        let owner = self.owner(t);
        let Some(obj) = owner.get_object_definition(type_name(t)) else {
            return String::new();
        };
        let foreign = obj.has_callback_interface() && !self.ci.is_external(t);
        let (odd_clone, odd_free) = if foreign {
            (
                format!(
                    "    if (handle & 1) {{\n        return uniffi_handle_map_clone(&{p}_{m}_handles, handle);\n    }}\n"
                ),
                format!(
                    "    if (handle & 1) {{\n        {p}_{m}_drop(handle);\n        return;\n    }}\n"
                ),
            )
        } else {
            (String::new(), String::new())
        };
        format!(
            "static UNIFFI_MAYBE_UNUSED void {p}_push_{m}(lua_State *L, uint64_t handle) {{\n    {p}_object *o = ({p}_object *)uniffi_lua_newuserdata(L, sizeof *o);\n    o->handle = handle;\n    luaL_getmetatable(L, {mt});\n    if (lua_isnil(L, -1)) {{\n        luaL_error(L, \"require the module of %s first\", {mt});\n    }}\n    lua_setmetatable(L, -2);\n}}\n\nstatic UNIFFI_MAYBE_UNUSED uint64_t {p}_handle_{m}(lua_State *L, int idx, const char *what) {{\n    {p}_object *o = ({p}_object *)uniffi_lua_testudata(L, idx, {mt});\n    if (!o) {{\n        luaL_error(L, \"%s: a %s expected, got %s\", what, {mt}, luaL_typename(L, idx));\n    }}\n    return o->handle;\n}}\n\nstatic UNIFFI_MAYBE_UNUSED uint64_t {p}_clone_{m}(uint64_t handle) {{\n{odd_clone}    RustCallStatus uniffi_call = {{0}};\n    return {clone}(handle, &uniffi_call);\n}}\n\nstatic UNIFFI_MAYBE_UNUSED uint64_t {p}_lower_{m}(lua_State *L, int idx, const char *what) {{\n    return {p}_clone_{m}({p}_handle_{m}(L, idx, what));\n}}\n\nstatic UNIFFI_MAYBE_UNUSED void {p}_release_{m}(uint64_t handle) {{\n{odd_free}    RustCallStatus uniffi_call = {{0}};\n    {free}(handle, &uniffi_call);\n}}\n\nstatic UNIFFI_MAYBE_UNUSED int {p}_gc_{m}(lua_State *L) {{\n    {p}_object *o = ({p}_object *)lua_touserdata(L, 1);\n    if (o && o->handle) {{\n        {p}_release_{m}(o->handle);\n        o->handle = 0;\n    }}\n    return 0;\n}}\n\n",
            clone = obj.ffi_object_clone().name(),
            free = obj.ffi_object_free().name(),
        )
    }

    /// Methods, constructors and metamethods of an object, plus its registration.
    pub(crate) fn emit_object(&self, obj: &Object) -> (String, String) {
        let t = obj.as_type();
        let p = &self.prefix;
        let m = self.mangle(&t);
        let name = obj.name();
        let mut out = String::new();
        let mut class: Vec<Reg> = Vec::new();
        let mut methods: Vec<Reg> = Vec::new();
        for c in obj.constructors() {
            let cname = format!("{p}_new_{m}_{}", ident(c.name()));
            out.push_str(&self.lua_function(
                c,
                &cname,
                &Receiver::None,
                &format!("{name}.{}", c.name()),
            ));
            class.push((c.name().to_owned(), cname));
        }
        for meth in obj.methods() {
            let cname = format!("{p}_method_{m}_{}", ident(meth.name()));
            out.push_str(&self.lua_function(
                meth,
                &cname,
                &Receiver::Object(t.clone(), meth.name().to_owned()),
                &format!("{name}:{}", meth.name()),
            ));
            methods.push((meth.name().to_owned(), cname));
        }
        let mut meta: Vec<Reg> = vec![("__gc".into(), format!("{p}_gc_{m}"))];
        self.trait_functions(
            &t,
            &obj.uniffi_trait_methods(),
            &Receiver::Object(t.clone(), String::new()),
            &mut out,
            &mut meta,
            &mut methods,
        );
        if obj.has_callback_interface() {
            let cname = format!("{p}_from_impl_{m}");
            let _ = write!(
                out,
                "static int {cname}(lua_State *L) {{\n    {p}_enter(L);\n    if (!lua_istable(L, 1)) {{\n        return luaL_error(L, \"{name}.from_impl: a table expected, got %s\", luaL_typename(L, 1));\n    }}\n    lua_pushvalue(L, 1);\n    {p}_push_{m}(L, {p}_insert_{m}(luaL_ref(L, LUA_REGISTRYINDEX)));\n    return 1;\n}}\n\n"
            );
            class.push(("from_impl".into(), cname));
        }
        let register = self.register_type(&t, &class, &methods, &meta);
        (out, register)
    }

    /// Constructors, methods and metamethods of a record or enum.
    pub(crate) fn emit_value_type(
        &self,
        t: &Type,
        constructors: &[Constructor],
        methods: &[Method],
        traits: &UniffiTraitMethods,
    ) -> (String, String) {
        let p = &self.prefix;
        let m = self.mangle(t);
        let name = type_name(t);
        let mut out = String::new();
        let mut class: Vec<Reg> = Vec::new();
        let mut regs: Vec<Reg> = Vec::new();
        for c in constructors {
            let cname = format!("{p}_new_{m}_{}", ident(c.name()));
            out.push_str(&self.lua_function(
                c,
                &cname,
                &Receiver::None,
                &format!("{name}.{}", c.name()),
            ));
            class.push((c.name().to_owned(), cname));
        }
        for meth in methods {
            let cname = format!("{p}_method_{m}_{}", ident(meth.name()));
            out.push_str(&self.lua_function(
                meth,
                &cname,
                &Receiver::Value(t.clone()),
                &format!("{name}:{}", meth.name()),
            ));
            regs.push((meth.name().to_owned(), cname.clone()));
            class.push((meth.name().to_owned(), cname));
        }
        let mut meta: Vec<Reg> = Vec::new();
        self.trait_functions(
            t,
            traits,
            &Receiver::Value(t.clone()),
            &mut out,
            &mut meta,
            &mut regs,
        );
        (out, self.register_type(t, &class, &regs, &meta))
    }

    /// `__tostring`, `__eq`, `__lt`, `__le` and `hash`/`debug_string` from Rust's traits.
    fn trait_functions(
        &self,
        t: &Type,
        traits: &UniffiTraitMethods,
        receiver: &Receiver,
        out: &mut String,
        meta: &mut Vec<Reg>,
        methods: &mut Vec<Reg>,
    ) {
        let p = &self.prefix;
        let m = self.mangle(t);
        let name = type_name(t);
        if let Some(meth) = &traits.display_fmt {
            let cname = format!("{p}_tostring_{m}");
            out.push_str(&self.lua_function(meth, &cname, receiver, &format!("tostring({name})")));
            meta.push(("__tostring".into(), cname));
        }
        if let Some(meth) = &traits.debug_fmt {
            let cname = format!("{p}_debug_{m}");
            out.push_str(&self.lua_function(
                meth,
                &cname,
                receiver,
                &format!("{name}:debug_string"),
            ));
            methods.push(("debug_string".into(), cname));
        }
        if let Some(meth) = &traits.eq_eq {
            let cname = format!("{p}_eq_{m}");
            out.push_str(&self.lua_function(meth, &cname, receiver, &format!("{name} ==")));
            meta.push(("__eq".into(), cname));
        }
        if let Some(meth) = &traits.hash_hash {
            let cname = format!("{p}_hash_{m}");
            out.push_str(&self.lua_function(meth, &cname, receiver, &format!("{name}:hash")));
            methods.push(("hash".into(), cname));
        }
        if let Some(meth) = &traits.ord_cmp {
            let cmp = format!("{p}_cmp_{m}");
            out.push_str(&self.lua_function(meth, &cmp, receiver, &format!("{name} <")));
            let _ = write!(
                out,
                "static int {p}_lt_{m}(lua_State *L) {{\n    {cmp}(L);\n    lua_pushboolean(L, lua_tonumber(L, -1) < 0);\n    return 1;\n}}\n\nstatic int {p}_le_{m}(lua_State *L) {{\n    {cmp}(L);\n    lua_pushboolean(L, lua_tonumber(L, -1) <= 0);\n    return 1;\n}}\n\n"
            );
            meta.push(("__lt".into(), format!("{p}_lt_{m}")));
            meta.push(("__le".into(), format!("{p}_le_{m}")));
            methods.push(("cmp".into(), cmp));
        }
    }

    /// The `luaopen` code registering a type: its metatable and its class table.
    fn register_type(&self, t: &Type, class: &[Reg], methods: &[Reg], meta: &[Reg]) -> String {
        let mt = string_literal(&self.metatable_name(t));
        let mut out = format!("    luaL_newmetatable(L, {mt});\n");
        if !methods.is_empty() {
            out.push_str("    lua_newtable(L);\n");
            for (n, f) in methods {
                let _ = writeln!(
                    out,
                    "    lua_pushcfunction(L, {f});\n    lua_setfield(L, -2, {});",
                    string_literal(n)
                );
            }
            out.push_str("    lua_setfield(L, -2, \"__index\");\n");
        }
        for (n, f) in meta {
            let _ = writeln!(
                out,
                "    lua_pushcfunction(L, {f});\n    lua_setfield(L, -2, {});",
                string_literal(n)
            );
        }
        let _ = writeln!(
            out,
            "    lua_pushstring(L, {mt});\n    lua_setfield(L, -2, \"__name\");\n    lua_pop(L, 1);"
        );
        if !class.is_empty() {
            out.push_str("    lua_newtable(L);\n");
            for (n, f) in class {
                let _ = writeln!(
                    out,
                    "    lua_pushcfunction(L, {f});\n    lua_setfield(L, -2, {});",
                    string_literal(n)
                );
            }
            let _ = writeln!(
                out,
                "    lua_setfield(L, -2, {});",
                string_literal(type_name(t))
            );
        }
        out
    }

    /// The error pushers (`<p>_push_error_<E>`) of every thrown type, and `__tostring` of errors.
    pub(crate) fn emit_error_helpers(&self) -> (String, String) {
        let p = &self.prefix;
        let mut out = String::new();
        let mut register = String::new();
        let mut done = std::collections::BTreeSet::new();
        for c in self.all_callables() {
            let Some(t) = c.throws_type() else { continue };
            let m = self.mangle(t);
            if !done.insert(m.clone()) {
                continue;
            }
            let push = match t {
                Type::Object { .. } => format!(
                    "    uniffi_reader reader = uniffi_reader_new(buffer.data, (size_t)buffer.len);\n    uint64_t handle = uniffi_read_u64(&reader);\n    uniffi_buffer_free(buffer);\n    {p}_push_{m}(L, handle);\n"
                ),
                _ => format!(
                    "    uniffi_reader reader = uniffi_reader_new(buffer.data, (size_t)buffer.len);\n    uniffi_reader *r = &reader;\n    {}\n    bool bad = reader.failed;\n    uniffi_buffer_free(buffer);\n    if (bad) {{\n        lua_pop(L, 1);\n        lua_pushliteral(L, \"Rust sent a malformed error\");\n    }}\n",
                    self.decode(t)
                ),
            };
            let _ = write!(
                out,
                "static void {p}_push_error_{m}(lua_State *L, RustBuffer buffer) {{\n{push}}}\n\n"
            );
            if matches!(t, Type::Enum { .. }) && !self.has_value_methods(t) {
                let mt = string_literal(&self.metatable_name(t));
                let _ = write!(
                    register,
                    "    luaL_newmetatable(L, {mt});\n    lua_pushcfunction(L, uniffi_lua_error_tostring);\n    lua_setfield(L, -2, \"__tostring\");\n    lua_pushstring(L, {mt});\n    lua_setfield(L, -2, \"__name\");\n    lua_pop(L, 1);\n"
                );
            }
        }
        (out, register)
    }
}

/// `&str` arguments are not part of UniFFI: only `&[u8]` is lent.
fn unreachable_string() -> (String, String, String) {
    (String::new(), String::new(), String::new())
}
