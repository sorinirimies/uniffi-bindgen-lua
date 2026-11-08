//! The C ABI declarations of a component: structs, callback types and the
//! scaffolding functions. They go in the `.c` file only, never in the header.

use std::fmt::Write;

use uniffi_bindgen::interface::FfiDefinition;

use super::names::escape;
use super::types::Gen;

impl Gen<'_> {
    pub(crate) fn emit_ffi_declarations(&self) -> String {
        let mut out = String::from("/* ---- The scaffolding's C ABI ---- */\n\n");
        // Structs may be named before they are defined (callback types take pointers).
        for def in self.ci.ffi_definitions() {
            if let FfiDefinition::Struct(st) = def {
                let _ = writeln!(out, "typedef struct {0} {0};", st.name());
            }
        }
        for def in self.ci.ffi_definitions() {
            match def {
                FfiDefinition::CallbackFunction(cb) => {
                    let mut args: Vec<String> = cb
                        .arguments()
                        .iter()
                        .map(|a| self.ffi_type(&a.type_()))
                        .collect();
                    if cb.has_rust_call_status_arg() {
                        args.push("RustCallStatus *".into());
                    }
                    if args.is_empty() {
                        args.push("void".into());
                    }
                    let ret = cb
                        .return_type()
                        .map_or_else(|| "void".to_owned(), |t| self.ffi_type(t));
                    let _ = writeln!(out, "typedef {ret} (*{})({});", cb.name(), args.join(", "));
                }
                FfiDefinition::Struct(st) => {
                    let _ = writeln!(out, "struct {} {{", st.name());
                    if st.fields().is_empty() {
                        let _ = writeln!(out, "    char uniffi_empty;");
                    }
                    for field in st.fields() {
                        let _ = writeln!(
                            out,
                            "    {} {};",
                            self.ffi_type(&field.type_()),
                            escape(field.name())
                        );
                    }
                    let _ = writeln!(out, "}};");
                }
                FfiDefinition::Function(f) => {
                    let mut args: Vec<String> = f
                        .arguments()
                        .iter()
                        .map(|a| format!("{} {}", self.ffi_type(&a.type_()), escape(a.name())))
                        .collect();
                    if f.has_rust_call_status_arg() {
                        args.push("RustCallStatus *out_status".into());
                    }
                    if args.is_empty() {
                        args.push("void".into());
                    }
                    let ret = f
                        .return_type()
                        .map_or_else(|| "void".to_owned(), |t| self.ffi_type(t));
                    let _ = writeln!(out, "extern {ret} {}({});", f.name(), args.join(", "));
                }
            }
        }
        out.push('\n');
        out
    }
}
