//! Valve's text KeyValues format (`.vdf`), enough to edit Steam's
//! `localconfig.vdf`. Strings are kept exactly as written (still escaped), so a
//! file Steam wrote serializes back byte for byte; callers check that before
//! changing anything.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Raw contents between the quotes, escapes untouched.
    Str(String),
    Obj(Vec<(String, Value)>),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub type Pairs = Vec<(String, Value)>;

pub fn parse(text: &str) -> Result<Pairs, ParseError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let pairs = parser.pairs(false)?;
    parser.skip_space();
    if parser.pos != parser.bytes.len() {
        return Err(parser.error("unexpected text after the last block"));
    }
    Ok(pairs)
}

/// Steam's layout: one tab per level, two tabs between a key and its value.
pub fn serialize(pairs: &[(String, Value)]) -> String {
    let mut out = String::new();
    write_pairs(&mut out, pairs, 0);
    out
}

fn write_pairs(out: &mut String, pairs: &[(String, Value)], depth: usize) {
    let indent = "\t".repeat(depth);
    for (key, value) in pairs {
        match value {
            Value::Str(s) => out.push_str(&format!("{indent}\"{key}\"\t\t\"{s}\"\n")),
            Value::Obj(children) => {
                out.push_str(&format!("{indent}\"{key}\"\n{indent}{{\n"));
                write_pairs(out, children, depth + 1);
                out.push_str(&format!("{indent}}}\n"));
            }
        }
    }
}

/// The value as Steam means it: `\\`, `\"`, `\n`, `\t` decoded.
pub fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

pub fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Case-insensitive lookup, as Steam treats keys.
pub fn get<'a>(pairs: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    pairs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// The child object `key`, created at the end if missing. `None` if `key` holds a string.
pub fn obj_mut<'a>(pairs: &'a mut Pairs, key: &str) -> Option<&'a mut Pairs> {
    let index = match pairs.iter().position(|(k, _)| k.eq_ignore_ascii_case(key)) {
        Some(i) => i,
        None => {
            pairs.push((key.to_owned(), Value::Obj(Vec::new())));
            pairs.len() - 1
        }
    };
    match &mut pairs[index].1 {
        Value::Obj(children) => Some(children),
        Value::Str(_) => None,
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, message: &str) -> ParseError {
        let line = self.bytes[..self.pos.min(self.bytes.len())]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1;
        ParseError(format!("{message} (line {line})"))
    }

    fn skip_space(&mut self) {
        loop {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
            if self.bytes[self.pos..].starts_with(b"//") {
                while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                return;
            }
        }
    }

    fn pairs(&mut self, nested: bool) -> Result<Pairs, ParseError> {
        let mut pairs = Vec::new();
        loop {
            self.skip_space();
            match self.bytes.get(self.pos) {
                None if nested => return Err(self.error("missing '}'")),
                None => return Ok(pairs),
                Some(b'}') if nested => {
                    self.pos += 1;
                    return Ok(pairs);
                }
                Some(b'}') => return Err(self.error("unmatched '}'")),
                Some(_) => {
                    let key = self.string()?;
                    self.skip_space();
                    let value = match self.bytes.get(self.pos) {
                        Some(b'{') => {
                            self.pos += 1;
                            Value::Obj(self.pairs(true)?)
                        }
                        Some(b'"') => Value::Str(self.string()?),
                        _ => return Err(self.error("expected a value")),
                    };
                    pairs.push((key, value));
                }
            }
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err(self.error("expected a quoted string"));
        }
        let start = self.pos + 1;
        let mut i = start;
        while i < self.bytes.len() {
            match self.bytes[i] {
                b'\\' => i += 2,
                b'"' => {
                    self.pos = i + 1;
                    return std::str::from_utf8(&self.bytes[start..i])
                        .map(str::to_owned)
                        .map_err(|_| self.error("invalid UTF-8"));
                }
                _ => i += 1,
            }
        }
        Err(self.error("unterminated string"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\"UserLocalConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"427520\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LastPlayed\"\t\t\"1790000000\"\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"\\\"C:\\\\a b\\\\x.exe\\\" %command%\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";

    #[test]
    fn steam_layout_round_trips_byte_for_byte() {
        let parsed = parse(SAMPLE).unwrap();
        assert_eq!(serialize(&parsed), SAMPLE);
    }

    #[test]
    fn escaped_values_decode() {
        let parsed = parse(SAMPLE).unwrap();
        let app = [
            "UserLocalConfigStore",
            "Software",
            "Valve",
            "Steam",
            "apps",
            "427520",
        ]
        .iter()
        .try_fold(&parsed, |pairs, key| match get(pairs, key) {
            Some(Value::Obj(children)) => Some(children),
            _ => None,
        })
        .unwrap();
        let Some(Value::Str(raw)) = get(app, "launchoptions") else {
            panic!()
        };
        assert_eq!(unescape(raw), r#""C:\a b\x.exe" %command%"#);
        assert_eq!(escape(&unescape(raw)), *raw);
    }

    #[test]
    fn comments_and_loose_spacing_parse() {
        let parsed = parse("// header\n\"a\" { \"b\" \"c\" }").unwrap();
        assert_eq!(
            parsed,
            vec![(
                "a".into(),
                Value::Obj(vec![("b".into(), Value::Str("c".into()))])
            )]
        );
    }

    #[test]
    fn broken_input_is_an_error() {
        assert!(parse("\"a\"\n{\n\t\"b\"\t\t\"c\"\n").is_err());
        assert!(parse("\"a\" \"unterminated").is_err());
        assert!(parse("}").is_err());
    }

    #[test]
    fn obj_mut_creates_missing_children() {
        let mut pairs = parse("\"root\"\n{\n}\n").unwrap();
        let root = obj_mut(&mut pairs, "ROOT").unwrap();
        obj_mut(root, "apps")
            .unwrap()
            .push(("x".into(), Value::Str("1".into())));
        assert_eq!(
            serialize(&pairs),
            "\"root\"\n{\n\t\"apps\"\n\t{\n\t\t\"x\"\t\t\"1\"\n\t}\n}\n"
        );
    }
}
