// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Identity values as PostScript literal text — `(Fictional Press)`,
//! `47.0`, `true`, `/name`, `[612 792]`, `<< /a 1 >>` — scanned with the
//! language's own scanner into a value the interpreter seeds, so a host
//! configures the device in one syntax whatever the language binding.

use efterscript_vm::{MarkValue, Memory, Object, Type, scan_all};

/// The value one literal denotes; the text must hold exactly one.
pub(crate) fn literal(text: &str) -> Result<MarkValue, String> {
    let mut memory = Memory::new();
    let tokens = scan_all(text.as_bytes(), &mut memory, &mut ())
        .map_err(|e| format!("{} at byte {}", e.kind.name(), e.span.start))?;
    let mut cursor = Cursor {
        objects: tokens.iter().map(|(object, _)| *object).collect(),
        at: 0,
    };
    let value = parse(&memory, &mut cursor, 0)?;
    if cursor.next().is_some() {
        return Err("more than one value".to_string());
    }
    Ok(value)
}

const MAX_DEPTH: usize = 32;

struct Cursor {
    objects: Vec<Object>,
    at: usize,
}

impl Cursor {
    fn next(&mut self) -> Option<Object> {
        let object = self.objects.get(self.at).copied();
        self.at += 1;
        object
    }

    /// Consumes the next object if it is the executable name `close`.
    fn closes(&mut self, memory: &Memory, close: &str) -> Result<bool, String> {
        let object = self
            .objects
            .get(self.at)
            .copied()
            .ok_or_else(|| format!("missing `{close}`"))?;
        let closes = object.ty() == Type::Name
            && object.is_executable()
            && object
                .as_name()
                .is_some_and(|atom| memory.name_text(atom) == close.as_bytes());
        if closes {
            self.at += 1;
        }
        Ok(closes)
    }
}

fn name_text(memory: &Memory, object: Object) -> String {
    let atom = object.as_name().expect("a name");
    String::from_utf8_lossy(memory.name_text(atom)).into_owned()
}

fn parse(memory: &Memory, cursor: &mut Cursor, depth: usize) -> Result<MarkValue, String> {
    if depth > MAX_DEPTH {
        return Err("nested too deeply".to_string());
    }
    let object = cursor.next().ok_or_else(|| "no value".to_string())?;
    match object.ty() {
        Type::Integer => Ok(MarkValue::Int(object.as_i32().expect("integer"))),
        Type::Real => Ok(MarkValue::Real(object.as_f32().expect("real"))),
        Type::String => Ok(MarkValue::String(
            memory.string(object).unwrap_or_default().to_vec(),
        )),
        Type::Name if !object.is_executable() => {
            Ok(MarkValue::Name(name_text(memory, object).into_bytes()))
        }
        // `true`, `false`, and `null` scan as the names the interpreter
        // resolves to the constants.
        Type::Name => match name_text(memory, object).as_str() {
            "true" => Ok(MarkValue::Bool(true)),
            "false" => Ok(MarkValue::Bool(false)),
            "null" => Ok(MarkValue::Null),
            "[" => {
                let mut items = Vec::new();
                while !cursor.closes(memory, "]")? {
                    items.push(parse(memory, cursor, depth + 1)?);
                }
                Ok(MarkValue::Array(items))
            }
            "<<" => {
                let mut entries = Vec::new();
                while !cursor.closes(memory, ">>")? {
                    let key = cursor.next().ok_or_else(|| "missing `>>`".to_string())?;
                    if key.ty() != Type::Name || key.is_executable() {
                        return Err("a dictionary key must be a name".to_string());
                    }
                    let value = parse(memory, cursor, depth + 1)?;
                    entries.push((name_text(memory, key).into_bytes(), value));
                }
                Ok(MarkValue::Dict(entries))
            }
            other => Err(format!("`{other}` is not a literal")),
        },
        _ => Err(format!("a {} is not an identity value", object.ty().name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> MarkValue {
        MarkValue::Name(text.as_bytes().to_vec())
    }

    #[test]
    fn scalars() {
        assert_eq!(
            literal("(Fictional Press)").unwrap(),
            MarkValue::String(b"Fictional Press".to_vec())
        );
        assert_eq!(
            literal("(a\\)b)").unwrap(),
            MarkValue::String(b"a)b".to_vec())
        );
        assert_eq!(
            literal("<4142>").unwrap(),
            MarkValue::String(b"AB".to_vec())
        );
        assert_eq!(literal("47.0").unwrap(), MarkValue::Real(47.0));
        assert_eq!(literal(" 300 ").unwrap(), MarkValue::Int(300));
        assert_eq!(literal("true").unwrap(), MarkValue::Bool(true));
        assert_eq!(literal("false").unwrap(), MarkValue::Bool(false));
        assert_eq!(literal("null").unwrap(), MarkValue::Null);
        assert_eq!(literal("/Upper").unwrap(), name("Upper"));
    }

    #[test]
    fn composites() {
        assert_eq!(
            literal("[612 792]").unwrap(),
            MarkValue::Array(vec![MarkValue::Int(612), MarkValue::Int(792)])
        );
        assert_eq!(literal("[]").unwrap(), MarkValue::Array(Vec::new()));
        assert_eq!(
            literal("[ /a [1] ]").unwrap(),
            MarkValue::Array(vec![name("a"), MarkValue::Array(vec![MarkValue::Int(1)])])
        );
        assert_eq!(
            literal("<< /name /Upper /sizes [1] >>").unwrap(),
            MarkValue::Dict(vec![
                (b"name".to_vec(), name("Upper")),
                (b"sizes".to_vec(), MarkValue::Array(vec![MarkValue::Int(1)])),
            ])
        );
    }

    #[test]
    fn rejections() {
        assert!(literal("").is_err());
        assert!(literal("1 2").is_err());
        assert!(literal("add").is_err());
        assert!(literal("[1 2").is_err());
        assert!(literal("<< 1 2 >>").is_err());
        assert!(literal("<< /a >>").is_err());
        assert!(literal("{ 1 }").is_err());
        assert!(literal("(open").is_err());
        assert!(literal("]").is_err());
    }
}
