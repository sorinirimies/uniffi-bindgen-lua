//! Identifiers. Lua reaches every name through a table key, so names stay as
//! written in Rust; C identifiers are built from the component prefix, so only
//! struct fields from the FFI definitions need escaping.

/// C (and C++) keywords a struct field of the FFI definitions might collide with.
const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "register",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    "bool",
    "true",
    "false",
    "class",
    "new",
    "delete",
    "this",
    "template",
    "namespace",
    "operator",
    "private",
    "public",
    "protected",
    "virtual",
];

/// Appends `_` to a C keyword.
pub(crate) fn escape(name: &str) -> String {
    if C_KEYWORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

/// A C identifier fragment for any UniFFI name (always valid: [A-Za-z0-9_]).
pub(crate) fn ident(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Quote `s` as a C string literal.
pub(crate) fn string_literal(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}\"\"", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_appends_underscore_to_c_keywords() {
        assert_eq!(escape("return"), "return_");
        assert_eq!(escape("handle"), "handle");
    }

    #[test]
    fn ident_keeps_only_identifier_characters() {
        assert_eq!(ident("a-b.c"), "a_b_c");
    }

    #[test]
    fn string_literal_escapes_quotes_and_newlines() {
        assert_eq!(string_literal("a\"b\n"), r#""a\"b\n""#);
    }
}
