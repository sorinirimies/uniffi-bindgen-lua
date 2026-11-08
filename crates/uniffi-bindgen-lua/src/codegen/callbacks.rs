//! Lua implementations of callback interfaces and traits: a registry reference
//! per Rust handle, and the vtable functions Rust calls, which run the Lua method
//! on the Lua thread inside `lua_pcall` (a Lua error never unwinds through Rust).

use std::fmt::Write;

use uniffi_bindgen::interface::{AsType, FfiCallbackFunction, Method, Type};

use super::names::string_literal;
use super::types::{Gen, int_info};

impl Gen<'_> {
    /// The handle map of a Lua-implemented type and its helpers.
    pub(crate) fn emit_impl_handles(&self, t: &Type, object: bool) -> String {
        let p = &self.prefix;
        let m = self.mangle(t);
        let mut out = format!(
            "static uniffi_handle_map {p}_{m}_handles = UNIFFI_HANDLE_MAP_INIT;\n\nstatic UNIFFI_MAYBE_UNUSED uint64_t {p}_insert_{m}(int ref) {{\n    return uniffi_handle_map_insert(&{p}_{m}_handles, (void *)(intptr_t)ref);\n}}\n\nstatic int {p}_ref_{m}(uint64_t handle) {{\n    void *ref = uniffi_handle_map_get(&{p}_{m}_handles, handle);\n    return ref ? (int)(intptr_t)ref : LUA_NOREF;\n}}\n\nstatic void {p}_{m}_unref(lua_State *L, void *ref) {{\n    luaL_unref(L, LUA_REGISTRYINDEX, (int)(intptr_t)ref);\n}}\n\n/* Rust may drop its last handle on any thread: unref on the Lua thread. */\nstatic void {p}_{m}_drop(uint64_t handle) {{\n    void *ref = uniffi_handle_map_remove(&{p}_{m}_handles, handle);\n    if (ref) {{\n        uniffi_lua_invoke(&{p}_dispatcher, {p}_{m}_unref, ref, true);\n    }}\n}}\n\nstatic void {p}_{m}_vtable_free(uint64_t handle) {{\n    {p}_{m}_drop(handle);\n}}\n\nstatic uint64_t {p}_{m}_vtable_clone(uint64_t handle) {{\n    return uniffi_handle_map_clone(&{p}_{m}_handles, handle);\n}}\n\n"
        );
        if object {
            // Lua calling a method of a Lua-implemented object: call the table.
            let _ = write!(
                out,
                "static int {p}_{m}_call_impl(lua_State *L, const char *method) {{\n    int n = lua_gettop(L);\n    {p}_object *o = ({p}_object *)lua_touserdata(L, 1);\n    lua_rawgeti(L, LUA_REGISTRYINDEX, {p}_ref_{m}(o->handle));\n    lua_getfield(L, n + 1, method);\n    lua_pushvalue(L, n + 1);\n    for (int i = 2; i <= n; ++i) {{\n        lua_pushvalue(L, i);\n    }}\n    lua_call(L, n, LUA_MULTRET);\n    return lua_gettop(L) - (n + 1);\n}}\n\n"
            );
        } else {
            let _ = write!(
                out,
                "static UNIFFI_MAYBE_UNUSED uint64_t {p}_lower_{m}(lua_State *L, int idx, const char *what) {{\n    if (!lua_istable(L, idx)) {{\n        luaL_error(L, \"%s: a table expected, got %s\", what, luaL_typename(L, idx));\n    }}\n    lua_pushvalue(L, idx);\n    return {p}_insert_{m}(luaL_ref(L, LUA_REGISTRYINDEX));\n}}\n\nstatic UNIFFI_MAYBE_UNUSED void {p}_push_{m}(lua_State *L, uint64_t handle) {{\n    lua_rawgeti(L, LUA_REGISTRYINDEX, {p}_ref_{m}(handle));\n}}\n\n"
            );
        }
        out
    }

    /// The vtable functions of a Lua-implemented type and the function registering them.
    pub(crate) fn emit_vtable(
        &self,
        t: &Type,
        methods: &[(FfiCallbackFunction, Method)],
        init_fn: &str,
        vtable_type: &str,
    ) -> String {
        let p = &self.prefix;
        let m = self.mangle(t);
        let mut out = String::new();
        let mut entries = vec![
            format!("{p}_{m}_vtable_free"),
            format!("{p}_{m}_vtable_clone"),
        ];
        for (i, (callback, meth)) in methods.iter().enumerate() {
            out.push_str(&self.vtable_method(t, i, callback, meth));
            entries.push(format!("{p}_{m}_m{i}"));
        }
        let _ = write!(
            out,
            "static void {p}_{m}_init(void) {{\n    static {vtable_type} vtable = {{{}}};\n    {init_fn}(&vtable);\n}}\n\n",
            entries.join(", ")
        );
        out
    }

    fn vtable_method(
        &self,
        t: &Type,
        i: usize,
        callback: &FfiCallbackFunction,
        meth: &Method,
    ) -> String {
        let p = &self.prefix;
        let m = self.mangle(t);
        let base = format!("{p}_{m}_m{i}");
        let n_args = meth.arguments().len();
        let mname = meth.name();
        // The FFI parameters, named.
        let mut params: Vec<(String, String)> = Vec::new();
        for (j, a) in callback.arguments().iter().enumerate() {
            let name = match j {
                0 => "uniffi_handle".to_owned(),
                j if j <= n_args => format!("arg{}", j - 1),
                _ => a.name().to_owned(),
            };
            params.push((self.ffi_type(&a.type_()), name));
        }
        if callback.has_rust_call_status_arg() {
            params.push(("RustCallStatus *".into(), "uniffi_call_status".into()));
        }
        let ret_ffi = meth.return_type().map(|rt| self.ffi_of(rt));
        // The call record.
        let mut fields = String::from("    uint64_t handle;\n");
        for (j, a) in meth.arguments().iter().enumerate() {
            let _ = writeln!(fields, "    {} arg{j};", self.ffi_of(&a.as_type()));
        }
        if let Some(r) = &ret_ffi {
            let _ = writeln!(fields, "    {r} result;");
        }
        fields.push_str("    RustCallStatus status;\n");
        if meth.is_async() {
            let complete = callback
                .arguments()
                .iter()
                .find(|a| a.name() == "uniffi_future_callback")
                .map(|a| self.ffi_type(&a.type_()))
                .unwrap_or_default();
            let _ = writeln!(fields, "    {complete} complete;\n    uint64_t data;");
        }
        let mut out = format!("typedef struct {base}_call {{\n{fields}}} {base}_call;\n\n");
        // Inside lua_pcall: call the method, convert the result.
        let mut push_args = String::new();
        for (j, a) in meth.arguments().iter().enumerate() {
            push_args.push_str(&self.push_ffi_arg(&a.as_type(), &format!("c->arg{j}"), j));
        }
        let nret = if ret_ffi.is_some() { 1 } else { 0 };
        let mut convert = String::new();
        if let Some(rt) = meth.return_type() {
            let what = string_literal(&format!("{mname}: result"));
            convert = self.result_from_lua(rt, &what);
        }
        let _ = write!(
            out,
            "static int {base}_protected(lua_State *L) {{\n    {base}_call *c = ({base}_call *)lua_touserdata(L, 1);\n    int ref = {p}_ref_{m}(c->handle);\n    if (ref == LUA_NOREF) {{\n        return luaL_error(L, \"the object was released already\");\n    }}\n    lua_rawgeti(L, LUA_REGISTRYINDEX, ref);\n    lua_getfield(L, -1, {name});\n    if (!lua_isfunction(L, -1)) {{\n        return luaL_error(L, \"the implementation has no method '%s'\", {name});\n    }}\n    lua_pushvalue(L, -2);\n{push_args}    lua_call(L, {nargs}, {nret});\n{convert}    return 0;\n}}\n\n",
            name = string_literal(mname),
            nargs = n_args + 1
        );
        // A raised `{tag = ...}` table is the declared error.
        let mut typed = String::new();
        if let Some(et) = meth.throws_type()
            && matches!(et, Type::Enum { .. } | Type::Object { .. })
        {
            let to_wire = self.encode(et, "1", "\"the error\"");
            let _ = write!(
                out,
                "static int {base}_error(lua_State *L) {{\n    {base}_call *c = ({base}_call *)lua_touserdata(L, 2);\n    uniffi_writer *w = uniffi_lua_writer(L);\n    {to_wire}\n    c->status.error_buf = uniffi_buffer_from(w->data, w->len);\n    uniffi_writer_free(w);\n    c->status.code = UNIFFI_CALL_ERROR;\n    return 0;\n}}\n\n"
            );
            typed = format!(
                "        if (lua_istable(L, -1) || lua_isuserdata(L, -1)) {{\n            lua_pushcfunction(L, {base}_error);\n            lua_pushvalue(L, -2);\n            lua_pushlightuserdata(L, c);\n            handled = lua_pcall(L, 2, 0, 0) == LUA_OK;\n            if (!handled) {{\n                lua_pop(L, 1);\n            }}\n        }}\n"
            );
        }
        let mut free_args = String::new();
        for (j, a) in meth.arguments().iter().enumerate() {
            if Self::is_buffer_type(&a.as_type()) {
                let _ = writeln!(free_args, "    uniffi_buffer_free(c->arg{j});");
            }
        }
        let _ = write!(
            out,
            "static void {base}_body(lua_State *L, {base}_call *c) {{\n    int top = lua_gettop(L);\n    lua_pushcfunction(L, {base}_protected);\n    lua_pushlightuserdata(L, c);\n    if (lua_pcall(L, 1, 0, 0) != LUA_OK) {{\n        int handled = 0;\n{typed}        if (!handled) {{\n            size_t n = 0;\n            const char *message = uniffi_lua_error_message(L, -1, &n);\n            c->status.code = UNIFFI_CALL_UNEXPECTED_ERROR;\n            c->status.error_buf = uniffi_buffer_from(message, n);\n        }}\n    }}\n    lua_settop(L, top);\n{free_args}}}\n\n"
        );
        // Called by Rust.
        let param_list: Vec<String> = params.iter().map(|(ty, n)| format!("{ty} {n}")).collect();
        let mut init = String::from("    c->handle = uniffi_handle;\n");
        for j in 0..n_args {
            let _ = writeln!(init, "    c->arg{j} = arg{j};");
        }
        if meth.is_async() {
            let result_set = if ret_ffi.is_some() {
                "    result.return_value = c->result;\n"
            } else {
                ""
            };
            let result_struct = meth.foreign_future_ffi_result_struct().name().to_owned();
            let _ = write!(
                out,
                "static void {base}_async_run(lua_State *L, void *raw) {{\n    {base}_call *c = ({base}_call *)raw;\n    {base}_body(L, c);\n    {result_struct} result;\n    memset(&result, 0, sizeof result);\n{result_set}    result.call_status = c->status;\n    c->complete(c->data, result);\n    free(c);\n}}\n\nstatic void {base}({}) {{\n    uniffi_out_dropped_callback->handle = 0;\n    uniffi_out_dropped_callback->free = uniffi_foreign_future_dropped;\n    {base}_call *c = ({base}_call *)uniffi_alloc(sizeof *c);\n{init}    c->complete = uniffi_future_callback;\n    c->data = uniffi_callback_data;\n    uniffi_lua_invoke(&{p}_dispatcher, {base}_async_run, c, true);\n}}\n\n",
                param_list.join(", ")
            );
        } else if ret_ffi.is_none() && meth.throws_type().is_none() {
            // Nothing to return: a call from another thread does not wait.
            let _ = write!(
                out,
                "static void {base}_detached(lua_State *L, void *raw) {{\n    {base}_call *c = ({base}_call *)raw;\n    {base}_body(L, c);\n    if (c->status.code != UNIFFI_CALL_SUCCESS) {{\n        fprintf(stderr, \"uniffi: a {mname} callback failed: %.*s\\n\", (int)c->status.error_buf.len, c->status.error_buf.data ? (const char *)c->status.error_buf.data : \"\");\n        uniffi_buffer_free(c->status.error_buf);\n    }}\n    free(c);\n}}\n\nstatic void {base}({}) {{\n    (void)uniffi_out_return;\n    if (!uniffi_lua_on_owner(&{p}_dispatcher)) {{\n        {base}_call *c = ({base}_call *)uniffi_alloc(sizeof *c);\n{init_heap}        uniffi_lua_invoke(&{p}_dispatcher, {base}_detached, c, true);\n        return;\n    }}\n    {base}_call call;\n    memset(&call, 0, sizeof call);\n    {base}_call *c = &call;\n{init}    {base}_body({p}_dispatcher.L, c);\n    *uniffi_call_status = c->status;\n}}\n\n",
                param_list.join(", "),
                init_heap = init
                    .replace("\n    ", "\n        ")
                    .replacen("    ", "        ", 1),
            );
        } else {
            let store = if ret_ffi.is_some() {
                "    *uniffi_out_return = c->result;\n"
            } else {
                "    (void)uniffi_out_return;\n"
            };
            let _ = write!(
                out,
                "static void {base}_run(lua_State *L, void *raw) {{\n    {base}_body(L, ({base}_call *)raw);\n}}\n\nstatic void {base}({}) {{\n    {base}_call call;\n    memset(&call, 0, sizeof call);\n    {base}_call *c = &call;\n{init}    uniffi_lua_invoke(&{p}_dispatcher, {base}_run, c, false);\n    *uniffi_call_status = c->status;\n{store}}}\n\n",
                param_list.join(", ")
            );
        }
        out
    }

    /// Pushes the FFI argument `expr` of a callback (the bindings free buffers after).
    fn push_ffi_arg(&self, t: &Type, expr: &str, j: usize) -> String {
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
                "    lua_pushlstring(L, {expr}.data ? (const char *){expr}.data : \"\", (size_t){expr}.len);\n"
            ),
            Type::Custom { builtin, .. } => self.push_ffi_arg(builtin, expr, j),
            _ => format!(
                "    {{\n        uniffi_reader reader{j} = uniffi_reader_new({expr}.data, (size_t){expr}.len);\n        uniffi_reader *r = &reader{j};\n        {}\n        if (reader{j}.failed) {{\n            return luaL_error(L, \"Rust sent a malformed argument\");\n        }}\n    }}\n",
                self.decode(t)
            ),
        }
    }

    /// Converts the Lua result at the top into `c->result` (may raise).
    fn result_from_lua(&self, t: &Type, what: &str) -> String {
        let p = &self.prefix;
        match t {
            Type::Boolean => {
                format!("    c->result = uniffi_lua_check_bool(L, -1, {what}) ? 1 : 0;\n")
            }
            Type::Float32 => {
                format!("    c->result = (float)uniffi_lua_check_number(L, -1, {what});\n")
            }
            Type::Float64 => format!("    c->result = uniffi_lua_check_number(L, -1, {what});\n"),
            t if int_info(t).is_some() => {
                let (bits, signed) = int_info(t).unwrap_or((64, true));
                format!(
                    "    c->result = ({}int{bits}_t)uniffi_lua_check_int(L, -1, {bits}, {signed}, {what});\n",
                    if signed { "" } else { "u" }
                )
            }
            Type::Object { .. } | Type::CallbackInterface { .. } => format!(
                "    c->result = {p}_lower_{}(L, lua_gettop(L), {what});\n",
                self.mangle(t)
            ),
            Type::String => format!(
                "    {{\n        size_t n;\n        const char *s = uniffi_lua_check_lstring(L, -1, &n, {what});\n        c->result = uniffi_buffer_from(s, n);\n    }}\n"
            ),
            Type::Custom { builtin, .. } => self.result_from_lua(builtin, what),
            _ => format!(
                "    {{\n        int top = lua_gettop(L);\n        uniffi_writer *w = uniffi_lua_writer(L);\n        {}\n        c->result = uniffi_buffer_from(w->data, w->len);\n        uniffi_writer_free(w);\n    }}\n",
                self.encode(t, "top", what)
            ),
        }
    }

    pub(crate) fn has_async_callbacks(&self) -> bool {
        self.ci
            .callback_interface_definitions()
            .iter()
            .any(|cb| cb.methods().iter().any(|m| m.is_async()))
            || self
                .ci
                .object_definitions()
                .iter()
                .any(|o| o.has_callback_interface() && o.methods().iter().any(|m| m.is_async()))
    }
}
