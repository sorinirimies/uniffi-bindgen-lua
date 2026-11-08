//! Type facts for the Lua generator: mangled names, definitions (also of other
//! components' types), defaults, and the FFI spelling of types.

use std::collections::BTreeSet;

use uniffi_bindgen::ComponentInterface;
use uniffi_bindgen::interface::{
    AsType, Callable, DefaultValue, Enum, FfiType, Literal, Object, Record, Type,
};

use super::names::{ident, string_literal};
use crate::config::Config;

#[derive(Debug)]
pub(crate) struct Gen<'a> {
    pub(crate) ci: &'a ComponentInterface,
    pub(crate) config: &'a Config,
    /// Prefix of every C identifier: the namespace.
    pub(crate) prefix: String,
    /// The other components generated alongside: definitions of external types.
    others: &'a [ComponentInterface],
    external_errors: &'a BTreeSet<String>,
}

impl<'a> Gen<'a> {
    pub(crate) fn new(
        ci: &'a ComponentInterface,
        config: &'a Config,
        others: &'a [ComponentInterface],
        external_errors: &'a BTreeSet<String>,
    ) -> Self {
        Self {
            ci,
            config,
            prefix: ident(ci.namespace()),
            others,
            external_errors,
        }
    }

    /// The component that defines `t`: this one, or the other one that has it.
    pub(crate) fn owner(&self, t: &Type) -> &ComponentInterface {
        if !self.ci.is_external(t) {
            return self.ci;
        }
        let name = type_name(t);
        self.others
            .iter()
            .find(|o| {
                o.get_record_definition(name).is_some()
                    || o.get_enum_definition(name).is_some()
                    || o.get_object_definition(name).is_some()
                    || o.get_callback_interface_definition(name).is_some()
                    || o.iter_local_types()
                        .any(|lt| matches!(lt, Type::Custom { name: n, .. } if n == name))
            })
            .unwrap_or(self.ci)
    }

    /// The Lua module (namespace) that defines `t`.
    pub(crate) fn module_of(&self, t: &Type) -> String {
        if !self.ci.is_external(t) {
            return self.ci.namespace().to_owned();
        }
        self.ci
            .namespace_for_type(t)
            .map_or_else(|_| self.owner(t).namespace().to_owned(), str::to_owned)
    }

    pub(crate) fn record_def(&self, t: &Type) -> Option<&Record> {
        self.owner(t).get_record_definition(type_name(t))
    }

    pub(crate) fn enum_def(&self, t: &Type) -> Option<&Enum> {
        self.owner(t).get_enum_definition(type_name(t))
    }

    pub(crate) fn object_def(&self, t: &Type) -> Option<&Object> {
        self.owner(t).get_object_definition(type_name(t))
    }

    pub(crate) fn is_error_enum(&self, t: &Type) -> bool {
        let name = type_name(t);
        let owner = self.owner(t);
        owner.get_enum_definition(name).is_some()
            && (owner.is_name_used_as_error(name)
                || self.external_errors.contains(name)
                || self.ci.is_name_used_as_error(name))
    }

    /// An enum without data that is not an error: a string in Lua.
    pub(crate) fn is_flat_enum(&self, t: &Type) -> bool {
        self.enum_def(t)
            .is_some_and(|e| e.is_flat() && !self.is_error_enum(t))
    }

    /// A C identifier unique among the component's types.
    pub(crate) fn mangle(&self, t: &Type) -> String {
        match t {
            Type::UInt8 => "U8".into(),
            Type::Int8 => "I8".into(),
            Type::UInt16 => "U16".into(),
            Type::Int16 => "I16".into(),
            Type::UInt32 => "U32".into(),
            Type::Int32 => "I32".into(),
            Type::UInt64 => "U64".into(),
            Type::Int64 => "I64".into(),
            Type::Float32 => "F32".into(),
            Type::Float64 => "F64".into(),
            Type::Boolean => "Bool".into(),
            Type::String => "String".into(),
            Type::Bytes => "Bytes".into(),
            Type::Timestamp => "Timestamp".into(),
            Type::Duration => "Duration".into(),
            Type::Object { name, .. }
            | Type::CallbackInterface { name, .. }
            | Type::Record { name, .. }
            | Type::Enum { name, .. }
            | Type::Custom { name, .. } => {
                if self.ci.is_external(t) {
                    format!("{}_{}", ident(&self.module_of(t)), ident(name))
                } else {
                    ident(name)
                }
            }
            Type::Box { inner_type } => self.mangle(inner_type),
            Type::Optional { inner_type } => format!("Option{}", self.mangle(inner_type)),
            Type::Sequence { inner_type } => format!("Sequence{}", self.mangle(inner_type)),
            Type::Set { inner_type } => format!("Set{}", self.mangle(inner_type)),
            Type::Map {
                key_type,
                value_type,
            } => format!("Map{}{}", self.mangle(key_type), self.mangle(value_type)),
        }
    }

    /// The Lua-facing name of a type, for messages and metatables: `ns.Name`.
    pub(crate) fn metatable_name(&self, t: &Type) -> String {
        format!("{}.{}", self.module_of(t), type_name(t))
    }

    /// Every type this component uses, nested ones included, each once.
    pub(crate) fn all_types(&self) -> Vec<Type> {
        let mut roots: Vec<Type> = self
            .ci
            .iter_local_types()
            .chain(self.ci.iter_external_types())
            .cloned()
            .collect();
        for r in self.ci.record_definitions() {
            roots.extend(r.fields().iter().map(|f| f.as_type()));
        }
        for e in self.ci.enum_definitions() {
            for v in e.variants() {
                roots.extend(v.fields().iter().map(|f| f.as_type()));
            }
        }
        for c in self.ci.iter_callables() {
            roots.extend(c.arguments().iter().map(|a| a.as_type()));
            roots.extend(c.return_type().cloned());
            roots.extend(c.throws_type().cloned());
        }
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        let mut stack: Vec<Type> = roots;
        while let Some(t) = stack.pop() {
            for inner in t.iter_types() {
                // A box converts like what it holds, under the same name.
                if matches!(inner, Type::Box { .. }) {
                    continue;
                }
                if !seen.insert(self.mangle(inner)) {
                    continue;
                }
                out.push(inner.clone());
                // External records and enums: their fields need converters too.
                if self.ci.is_external(inner) {
                    if let Some(r) = self.record_def(inner) {
                        stack.extend(r.fields().iter().map(|f| f.as_type()));
                    }
                    if let Some(e) = self.enum_def(inner) {
                        for v in e.variants() {
                            stack.extend(v.fields().iter().map(|f| f.as_type()));
                        }
                    }
                }
            }
        }
        out
    }

    /// The smallest number of bytes a `t` takes in a buffer; 0 when it can be empty.
    pub(crate) fn min_wire_size(&self, t: &Type) -> usize {
        match t {
            Type::Boolean | Type::UInt8 | Type::Int8 | Type::Optional { .. } => 1,
            Type::UInt16 | Type::Int16 => 2,
            Type::UInt32 | Type::Int32 | Type::Float32 => 4,
            Type::UInt64 | Type::Int64 | Type::Float64 => 8,
            Type::String
            | Type::Bytes
            | Type::Sequence { .. }
            | Type::Set { .. }
            | Type::Map { .. }
            | Type::Enum { .. } => 4,
            Type::Timestamp | Type::Duration => 12,
            Type::Object { .. } | Type::CallbackInterface { .. } => 8,
            Type::Box { inner_type } => self.min_wire_size(inner_type),
            Type::Custom { builtin, .. } => self.min_wire_size(builtin),
            Type::Record { .. } => self.record_def(t).map_or(0, |r| {
                r.fields()
                    .iter()
                    .map(|f| self.min_wire_size(&f.as_type()))
                    .sum()
            }),
        }
    }

    /// Whether a value of `t` crosses the FFI in a RustBuffer.
    pub(crate) fn is_buffer_type(t: &Type) -> bool {
        match t {
            Type::Boolean
            | Type::UInt8
            | Type::Int8
            | Type::UInt16
            | Type::Int16
            | Type::UInt32
            | Type::Int32
            | Type::UInt64
            | Type::Int64
            | Type::Float32
            | Type::Float64
            | Type::Object { .. }
            | Type::CallbackInterface { .. } => false,
            Type::Custom { builtin, .. } => Self::is_buffer_type(builtin),
            _ => true,
        }
    }

    /// The C spelling of an FFI-level type.
    pub(crate) fn ffi_type(&self, t: &FfiType) -> String {
        match t {
            FfiType::UInt8 => "uint8_t".into(),
            FfiType::Int8 => "int8_t".into(),
            FfiType::UInt16 => "uint16_t".into(),
            FfiType::Int16 => "int16_t".into(),
            FfiType::UInt32 => "uint32_t".into(),
            FfiType::Int32 => "int32_t".into(),
            FfiType::UInt64 | FfiType::Handle => "uint64_t".into(),
            FfiType::Int64 => "int64_t".into(),
            FfiType::Float32 => "float".into(),
            FfiType::Float64 => "double".into(),
            FfiType::RustBuffer(_) => "RustBuffer".into(),
            FfiType::ForeignBytes => "ForeignBytes".into(),
            FfiType::Callback(name) | FfiType::Struct(name) => name.clone(),
            FfiType::RustCallStatus => "RustCallStatus".into(),
            FfiType::Reference(inner) => format!("const {} *", self.ffi_type(inner)),
            FfiType::MutReference(inner) => format!("{} *", self.ffi_type(inner)),
            FfiType::VoidPointer => "void *".into(),
        }
    }

    /// The C type of the FFI value of `t`.
    pub(crate) fn ffi_of(&self, t: &Type) -> String {
        self.ffi_type(&FfiType::from(t))
    }

    /// A C statement writing the default `value` of `t` into `w`, or `None`.
    pub(crate) fn default_write(&self, value: &DefaultValue, t: &Type) -> Option<String> {
        match value {
            DefaultValue::Default => self.zero_write(t),
            DefaultValue::Literal(lit) => self.literal_write(lit, t),
        }
    }

    fn zero_write(&self, t: &Type) -> Option<String> {
        Some(match t {
            Type::Boolean => "uniffi_write_bool(w, false);".into(),
            Type::Float32 => "uniffi_write_f32(w, 0.0f);".into(),
            Type::Float64 => "uniffi_write_f64(w, 0.0);".into(),
            Type::UInt8
            | Type::Int8
            | Type::UInt16
            | Type::Int16
            | Type::UInt32
            | Type::Int32
            | Type::UInt64
            | Type::Int64 => {
                format!("uniffi_write_{}(w, 0);", prim(t))
            }
            Type::String
            | Type::Bytes
            | Type::Sequence { .. }
            | Type::Set { .. }
            | Type::Map { .. } => "uniffi_write_len(w, 0);".into(),
            Type::Optional { .. } => "uniffi_write_i8(w, 0);".into(),
            Type::Custom { builtin, .. } => return self.zero_write(builtin),
            _ => return None,
        })
    }

    fn literal_write(&self, lit: &Literal, t: &Type) -> Option<String> {
        let t = match t {
            Type::Custom { builtin, .. } => builtin.as_ref(),
            t => t,
        };
        Some(match lit {
            Literal::Boolean(b) => format!("uniffi_write_bool(w, {b});"),
            Literal::String(s) => {
                format!("uniffi_write_string(w, uniffi_str({}));", string_literal(s))
            }
            Literal::UInt(v, ..) => format!("uniffi_write_{}(w, {v}ULL);", prim(t)),
            Literal::Int(v, ..) => format!("uniffi_write_{}(w, {v}LL);", prim(t)),
            Literal::Float(s, _) => match t {
                Type::Float32 => format!("uniffi_write_f32(w, {s}f);"),
                _ => format!("uniffi_write_f64(w, {s});"),
            },
            Literal::Enum(variant, ty) => {
                let e = self.enum_def(ty)?;
                let normal = |s: &str| s.replace('_', "").to_ascii_lowercase();
                let index = e
                    .variants()
                    .iter()
                    .position(|v| normal(v.name()) == normal(variant))?;
                format!("uniffi_write_i32(w, {});", index + 1)
            }
            Literal::EmptySequence | Literal::EmptyMap | Literal::EmptySet => {
                "uniffi_write_len(w, 0);".into()
            }
            Literal::None => "uniffi_write_i8(w, 0);".into(),
            Literal::Some { inner } => {
                let inner_type = match t {
                    Type::Optional { inner_type } => inner_type.as_ref(),
                    other => other,
                };
                format!(
                    "uniffi_write_i8(w, 1); {}",
                    self.default_write(inner, inner_type)?
                )
            }
        })
    }

    /// Every function, method and constructor of the component.
    pub(crate) fn all_callables(&self) -> Vec<&dyn Callable> {
        self.ci.iter_callables().collect()
    }
}

/// The runtime codec suffix of a primitive (`u8`, `i64`, ...).
pub(crate) fn prim(t: &Type) -> &'static str {
    match t {
        Type::UInt8 => "u8",
        Type::Int8 => "i8",
        Type::UInt16 => "u16",
        Type::Int16 => "i16",
        Type::UInt32 => "u32",
        Type::Int32 => "i32",
        Type::UInt64 => "u64",
        Type::Int64 => "i64",
        Type::Float32 => "f32",
        Type::Float64 => "f64",
        Type::Boolean => "bool",
        _ => "",
    }
}

/// The bit width and signedness of an integer type.
pub(crate) fn int_info(t: &Type) -> Option<(u32, bool)> {
    Some(match t {
        Type::UInt8 => (8, false),
        Type::Int8 => (8, true),
        Type::UInt16 => (16, false),
        Type::Int16 => (16, true),
        Type::UInt32 => (32, false),
        Type::Int32 => (32, true),
        Type::UInt64 => (64, false),
        Type::Int64 => (64, true),
        _ => return None,
    })
}

/// The name of a user-defined type.
pub(crate) fn type_name(t: &Type) -> &str {
    match t {
        Type::Object { name, .. }
        | Type::CallbackInterface { name, .. }
        | Type::Record { name, .. }
        | Type::Enum { name, .. }
        | Type::Custom { name, .. } => name,
        _ => "",
    }
}
