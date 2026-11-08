//! Converting Lua values to UniFFI's wire format and back, with no C structs in
//! between: `to_wire_<T>(L, idx, w, what)` reads the Lua value at absolute stack
//! index `idx` (raising a Lua error when it does not fit `T`), and
//! `from_wire_<T>(L, r)` pushes one Lua value.

use std::fmt::Write;

use uniffi_bindgen::interface::{AsType, Type};

use super::names::string_literal;
use super::types::{Gen, int_info, prim, type_name};

impl Gen<'_> {
    pub(crate) fn encoder_name(&self, t: &Type) -> String {
        format!("{}_to_wire_{}", self.prefix, self.mangle(t))
    }

    pub(crate) fn decoder_name(&self, t: &Type) -> String {
        format!("{}_from_wire_{}", self.prefix, self.mangle(t))
    }

    /// C statements converting the Lua value at `idx` into `w`.
    pub(crate) fn encode(&self, t: &Type, idx: &str, what: &str) -> String {
        match t {
            Type::Boolean => {
                format!("uniffi_write_bool(w, uniffi_lua_check_bool(L, {idx}, {what}));")
            }
            Type::Float32 => {
                format!("uniffi_write_f32(w, (float)uniffi_lua_check_number(L, {idx}, {what}));")
            }
            Type::Float64 => {
                format!("uniffi_write_f64(w, uniffi_lua_check_number(L, {idx}, {what}));")
            }
            t if int_info(t).is_some() => {
                let (bits, signed) = int_info(t).unwrap_or((64, true));
                let c = format!("{}int{bits}_t", if signed { "" } else { "u" });
                format!(
                    "uniffi_write_{p}(w, ({c})uniffi_lua_check_int(L, {idx}, {bits}, {signed}, {what}));",
                    p = prim(t)
                )
            }
            Type::String | Type::Bytes => format!(
                "{{\n        size_t n;\n        const char *s = uniffi_lua_check_lstring(L, {idx}, &n, {what});\n        uniffi_write_len(w, n);\n        uniffi_write_raw(w, s, n);\n    }}"
            ),
            Type::Timestamp => format!(
                "uniffi_write_timestamp(w, uniffi_lua_seconds_to_timestamp(uniffi_lua_check_number(L, {idx}, {what})));"
            ),
            Type::Duration => format!(
                "uniffi_write_duration(w, uniffi_lua_seconds_to_duration(L, uniffi_lua_check_number(L, {idx}, {what}), {what}));"
            ),
            Type::Box { inner_type } => self.encode(inner_type, idx, what),
            Type::Custom { builtin, .. } => self.encode(builtin, idx, what),
            _ => format!("{}(L, {idx}, w, {what});", self.encoder_name(t)),
        }
    }

    /// C statements reading a `t` from `r` and pushing it.
    pub(crate) fn decode(&self, t: &Type) -> String {
        match t {
            Type::Boolean => "lua_pushboolean(L, uniffi_read_bool(r));".into(),
            Type::Float32 => "lua_pushnumber(L, (lua_Number)uniffi_read_f32(r));".into(),
            Type::Float64 => "lua_pushnumber(L, (lua_Number)uniffi_read_f64(r));".into(),
            Type::UInt64 => "uniffi_lua_push_u64(L, uniffi_read_u64(r));".into(),
            t if int_info(t).is_some() => format!(
                "uniffi_lua_push_i64(L, (int64_t)uniffi_read_{}(r));",
                prim(t)
            ),
            Type::String | Type::Bytes => "uniffi_lua_push_wire_string(L, r);".into(),
            Type::Timestamp => "uniffi_lua_push_timestamp(L, uniffi_read_timestamp(r));".into(),
            Type::Duration => "uniffi_lua_push_duration(L, uniffi_read_duration(r));".into(),
            Type::Box { inner_type } => self.decode(inner_type),
            Type::Custom { builtin, .. } => self.decode(builtin),
            _ => format!("{}(L, r);", self.decoder_name(t)),
        }
    }

    /// Prototypes and definitions of every converter the component needs.
    pub(crate) fn emit_converters(&self) -> String {
        let types: Vec<Type> = self
            .all_types()
            .into_iter()
            .filter(|t| {
                matches!(
                    t,
                    Type::Record { .. }
                        | Type::Enum { .. }
                        | Type::Object { .. }
                        | Type::CallbackInterface { .. }
                        | Type::Optional { .. }
                        | Type::Sequence { .. }
                        | Type::Set { .. }
                        | Type::Map { .. }
                )
            })
            .collect();
        let mut protos = String::from("/* ---- Lua values <-> the wire format ---- */\n\n");
        let mut defs = String::new();
        for t in &types {
            let _ = writeln!(
                protos,
                "static UNIFFI_MAYBE_UNUSED void {}(lua_State *L, int idx, uniffi_writer *w, const char *what);\nstatic UNIFFI_MAYBE_UNUSED void {}(lua_State *L, uniffi_reader *r);",
                self.encoder_name(t),
                self.decoder_name(t)
            );
            defs.push_str(&self.converter(t));
        }
        protos.push('\n');
        protos + &defs
    }

    fn converter(&self, t: &Type) -> String {
        let (to, from) = match t {
            Type::Record { .. } => self.record_converter(t),
            Type::Enum { .. } => self.enum_converter(t),
            Type::Object { .. } => (
                format!(
                    "    uniffi_write_u64(w, {p}_lower_{m}(L, idx, what));\n",
                    p = self.prefix,
                    m = self.mangle(t)
                ),
                format!(
                    "    {p}_push_{m}(L, uniffi_read_u64(r));\n",
                    p = self.prefix,
                    m = self.mangle(t)
                ),
            ),
            Type::CallbackInterface { .. } => (
                format!(
                    "    uniffi_write_u64(w, {p}_lower_{m}(L, idx, what));\n",
                    p = self.prefix,
                    m = self.mangle(t)
                ),
                format!(
                    "    {p}_push_{m}(L, uniffi_read_u64(r));\n",
                    p = self.prefix,
                    m = self.mangle(t)
                ),
            ),
            Type::Optional { inner_type } => (
                format!(
                    "    if (lua_isnoneornil(L, idx)) {{\n        uniffi_write_i8(w, 0);\n        return;\n    }}\n    uniffi_write_i8(w, 1);\n    {}\n",
                    self.encode(inner_type, "idx", "what")
                ),
                format!(
                    "    switch (uniffi_read_i8(r)) {{\n        case 1:\n            {}\n            break;\n        case 0:\n            lua_pushnil(L);\n            break;\n        default:\n            r->failed = true;\n            lua_pushnil(L);\n    }}\n",
                    self.decode(inner_type)
                ),
            ),
            Type::Sequence { inner_type } | Type::Set { inner_type } => (
                format!(
                    "    if (!lua_istable(L, idx)) {{\n        luaL_error(L, \"%s: a table expected, got %s\", what, luaL_typename(L, idx));\n    }}\n    /* `n` (as table.pack sets it) counts trailing and inner nils, which `#` cannot. */\n    size_t n = (size_t)lua_rawlen(L, idx);\n    lua_getfield(L, idx, \"n\");\n    if (uniffi_lua_isinteger(L, -1) && lua_tonumber(L, -1) >= 0) {{\n        n = (size_t)lua_tonumber(L, -1);\n    }}\n    lua_pop(L, 1);\n    uniffi_write_len(w, n);\n    for (size_t i = 1; i <= n; ++i) {{\n        lua_rawgeti(L, idx, (int)i);\n        int item = lua_gettop(L);\n        {}\n        lua_pop(L, 1);\n    }}\n",
                    self.encode(inner_type, "item", "what")
                ),
                format!(
                    "    size_t n = uniffi_read_len(r, {min});\n    lua_createtable(L, (int)n, 1);\n    for (size_t i = 0; i < n && !r->failed; ++i) {{\n        {}\n        lua_rawseti(L, -2, (int)(i + 1));\n    }}\n{set_n}",
                    self.decode(inner_type),
                    min = self.min_wire_size(inner_type),
                    // Lua arrays cannot hold nil: keep the length of a sequence of optionals.
                    set_n = if matches!(inner_type.as_ref(), Type::Optional { .. }) {
                        "    uniffi_lua_push_i64(L, (int64_t)n);\n    lua_setfield(L, -2, \"n\");\n"
                    } else {
                        ""
                    }
                ),
            ),
            Type::Map {
                key_type,
                value_type,
            } => (
                format!(
                    "    if (!lua_istable(L, idx)) {{\n        luaL_error(L, \"%s: a table expected, got %s\", what, luaL_typename(L, idx));\n    }}\n    size_t n = 0;\n    lua_pushnil(L);\n    while (lua_next(L, idx)) {{\n        n++;\n        lua_pop(L, 1);\n    }}\n    uniffi_write_len(w, n);\n    lua_pushnil(L);\n    while (lua_next(L, idx)) {{\n        int key = lua_gettop(L) - 1;\n        int value = lua_gettop(L);\n        {}\n        {}\n        lua_pop(L, 1);\n    }}\n",
                    self.encode(key_type, "key", "what"),
                    self.encode(value_type, "value", "what")
                ),
                format!(
                    "    size_t n = uniffi_read_len(r, {min});\n    lua_createtable(L, 0, (int)n);\n    for (size_t i = 0; i < n && !r->failed; ++i) {{\n        {}\n        {}\n        lua_settable(L, -3);\n    }}\n",
                    self.decode(key_type),
                    self.decode(value_type),
                    min = self.min_wire_size(key_type) + self.min_wire_size(value_type)
                ),
            ),
            _ => return String::new(),
        };
        format!(
            "static void {}(lua_State *L, int idx, uniffi_writer *w, const char *what) {{\n    (void)what;\n    (void)w;\n    luaL_checkstack(L, 8, \"uniffi: values nested too deeply\");\n{to}}}\n\nstatic void {}(lua_State *L, uniffi_reader *r) {{\n    (void)r;\n    luaL_checkstack(L, 8, \"uniffi: values nested too deeply\");\n{from}}}\n\n",
            self.encoder_name(t),
            self.decoder_name(t)
        )
    }

    fn record_converter(&self, t: &Type) -> (String, String) {
        let Some(r) = self.record_def(t) else {
            return (String::new(), "    lua_pushnil(L);\n".into());
        };
        let name = type_name(t);
        let mut to = String::from(
            "    if (!lua_istable(L, idx)) {\n        luaL_error(L, \"%s: a table expected, got %s\", what, luaL_typename(L, idx));\n    }\n",
        );
        let mut from = format!("    lua_createtable(L, 0, {});\n", r.fields().len());
        for f in r.fields() {
            let ft = f.as_type();
            let what = string_literal(&format!("{name}.{}", f.name()));
            let missing = match f.default_value().and_then(|d| self.default_write(d, &ft)) {
                Some(stmt) => stmt,
                None if matches!(ft, Type::Optional { .. }) => "uniffi_write_i8(w, 0);".into(),
                None => format!(
                    "luaL_error(L, \"%s: field '{}' is missing\", what);",
                    f.name()
                ),
            };
            let _ = write!(
                to,
                "    lua_getfield(L, idx, {key});\n    if (lua_isnil(L, -1)) {{\n        {missing}\n    }} else {{\n        int field = lua_gettop(L);\n        {}\n    }}\n    lua_pop(L, 1);\n",
                self.encode(&ft, "field", &what),
                key = string_literal(f.name())
            );
            let _ = write!(
                from,
                "    {}\n    lua_setfield(L, -2, {});\n",
                self.decode(&ft),
                string_literal(f.name())
            );
        }
        if self.has_value_methods(t) {
            let _ = writeln!(
                from,
                "    luaL_getmetatable(L, {});\n    lua_setmetatable(L, -2);",
                string_literal(&self.metatable_name(t))
            );
        }
        (to, from)
    }

    fn enum_converter(&self, t: &Type) -> (String, String) {
        let Some(e) = self.enum_def(t) else {
            return (String::new(), "    lua_pushnil(L);\n".into());
        };
        let name = type_name(t);
        if self.is_flat_enum(t) {
            let mut to = String::from(
                "    const char *tag = uniffi_lua_check_lstring(L, idx, NULL, what);
",
            );
            let mut from = String::from("    switch (uniffi_read_i32(r)) {\n");
            for (i, v) in e.variants().iter().enumerate() {
                let _ = write!(
                    to,
                    "    if (strcmp(tag, {}) == 0) {{\n        uniffi_write_i32(w, {});\n        return;\n    }}\n",
                    string_literal(v.name()),
                    i + 1
                );
                let _ = writeln!(
                    from,
                    "        case {}: lua_pushstring(L, {}); break;",
                    i + 1,
                    string_literal(v.name())
                );
            }
            let _ = writeln!(
                to,
                "    luaL_error(L, \"%s: '%s' is not a variant of {name}\", what, tag);"
            );
            from.push_str("        default: r->failed = true; lua_pushnil(L);\n    }\n");
            return (to, from);
        }
        let flat_error = e.is_flat();
        let mut to = String::from(
            "    if (!lua_istable(L, idx)) {\n        luaL_error(L, \"%s: a table expected, got %s\", what, luaL_typename(L, idx));\n    }\n    lua_getfield(L, idx, \"tag\");\n    const char *tag = lua_tostring(L, -1);\n    if (!tag) {\n        luaL_error(L, \"%s: the variant's name expected in field 'tag'\", what);\n    }\n",
        );
        let mut from =
            String::from("    lua_createtable(L, 0, 4);\n    switch (uniffi_read_i32(r)) {\n");
        for (i, v) in e.variants().iter().enumerate() {
            let _ = write!(
                to,
                "    if (strcmp(tag, {}) == 0) {{\n        lua_pop(L, 1);\n        uniffi_write_i32(w, {});\n",
                string_literal(v.name()),
                i + 1
            );
            let _ = write!(
                from,
                "        case {}:\n            lua_pushstring(L, {});\n            lua_setfield(L, -2, \"tag\");\n",
                i + 1,
                string_literal(v.name())
            );
            if flat_error {
                from.push_str("            uniffi_lua_push_wire_string(L, r);\n            lua_setfield(L, -2, \"message\");\n");
            } else {
                for (j, f) in v.fields().iter().enumerate() {
                    let ft = f.as_type();
                    let what = string_literal(&format!("{name}.{}.{}", v.name(), f.name()));
                    let (get, set) = if f.name().is_empty() {
                        (
                            format!("lua_rawgeti(L, idx, {})", j + 1),
                            format!("lua_rawseti(L, -2, {})", j + 1),
                        )
                    } else {
                        (
                            format!("lua_getfield(L, idx, {})", string_literal(f.name())),
                            format!("lua_setfield(L, -2, {})", string_literal(f.name())),
                        )
                    };
                    let missing = if matches!(ft, Type::Optional { .. }) {
                        "uniffi_write_i8(w, 0);".to_owned()
                    } else {
                        format!(
                            "luaL_error(L, \"%s: field '{}' is missing\", what);",
                            if f.name().is_empty() {
                                format!("[{}]", j + 1)
                            } else {
                                f.name().to_owned()
                            }
                        )
                    };
                    let _ = write!(
                        to,
                        "        {get};\n        if (lua_isnil(L, -1)) {{\n            {missing}\n        }} else {{\n            int field = lua_gettop(L);\n            {}\n        }}\n        lua_pop(L, 1);\n",
                        self.encode(&ft, "field", &what)
                    );
                    let _ = write!(
                        from,
                        "            {}\n            {set};\n",
                        self.decode(&ft)
                    );
                }
            }
            to.push_str("        return;\n    }\n");
            from.push_str("            break;\n");
        }
        let _ = writeln!(
            to,
            "    luaL_error(L, \"%s: '%s' is not a variant of {name}\", what, tag);"
        );
        from.push_str("        default:\n            r->failed = true;\n    }\n");
        if self.is_error_enum(t) || self.has_value_methods(t) {
            let _ = writeln!(
                from,
                "    luaL_getmetatable(L, {});\n    lua_setmetatable(L, -2);",
                string_literal(&self.metatable_name(t))
            );
        }
        (to, from)
    }

    /// Whether values of this record or enum get a metatable (methods, traits).
    pub(crate) fn has_value_methods(&self, t: &Type) -> bool {
        if self.ci.is_external(t) {
            return false;
        }
        let traits_or_methods =
            |methods: usize,
             ctors: usize,
             traits: &uniffi_bindgen::interface::UniffiTraitMethods| {
                methods > 0
                    || ctors > 0
                    || traits.display_fmt.is_some()
                    || traits.eq_eq.is_some()
                    || traits.ord_cmp.is_some()
                    || traits.debug_fmt.is_some()
                    || traits.hash_hash.is_some()
            };
        if let Some(r) = self.ci.get_record_definition(type_name(t)) {
            return traits_or_methods(
                r.methods().len(),
                r.constructors().len(),
                &r.uniffi_trait_methods(),
            );
        }
        if let Some(e) = self.ci.get_enum_definition(type_name(t)) {
            return traits_or_methods(
                e.methods().len(),
                e.constructors().len(),
                &e.uniffi_trait_methods(),
            );
        }
        false
    }
}
