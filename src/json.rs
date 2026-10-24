//! A deliberately small JSON reader for the extension state file.
//!
//! The extension is the only producer, but a hand-written parser keeps the
//! executable free of a general-purpose serialization stack. It accepts the
//! full JSON value grammar and is strict about malformed input.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Number(v) => Some(*v),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Option<Json> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        pos: 0,
        depth: 0,
    };
    let value = parser.value()?;
    parser.skip_ws();
    if parser.pos == parser.bytes.len() {
        Some(value)
    } else {
        None
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Option<Json> {
        self.skip_ws();
        match self.peek()? {
            b'n' => {
                self.literal(b"null")?;
                Some(Json::Null)
            }
            b't' => {
                self.literal(b"true")?;
                Some(Json::Bool(true))
            }
            b'f' => {
                self.literal(b"false")?;
                Some(Json::Bool(false))
            }
            b'"' => Some(Json::String(self.string()?)),
            b'[' => self.array(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn literal(&mut self, expected: &[u8]) -> Option<()> {
        if self.bytes.get(self.pos..self.pos + expected.len())? == expected {
            self.pos += expected.len();
            Some(())
        } else {
            None
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.bump()? != b'"' {
            return None;
        }
        let mut out = String::new();
        loop {
            let b = self.bump()?;
            match b {
                b'"' => return Some(out),
                b'\\' => {
                    let esc = self.bump()?;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(self.bytes.get(self.pos..self.pos + 4)?)
                                .ok()?;
                            let code = u16::from_str_radix(hex, 16).ok()?;
                            self.pos += 4;
                            if (0xD800..0xDC00).contains(&code) {
                                // A high surrogate must be followed by a low one.
                                if self.bump()? != b'\\' || self.bump()? != b'u' {
                                    return None;
                                }
                                let low_text =
                                    std::str::from_utf8(self.bytes.get(self.pos..self.pos + 4)?)
                                        .ok()?;
                                let low = u16::from_str_radix(low_text, 16).ok()?;
                                self.pos += 4;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return None;
                                }
                                let scalar = 0x10000
                                    + (((code as u32) - 0xD800) << 10)
                                    + ((low as u32) - 0xDC00);
                                out.push(char::from_u32(scalar)?);
                            } else {
                                out.push(char::from_u32(code as u32)?);
                            }
                        }
                        _ => return None,
                    }
                }
                0x00..=0x1f => return None,
                _ => {
                    // Copy the raw UTF-8 sequence for this byte.
                    let start = self.pos - 1;
                    let width = utf8_width(b);
                    let end = start + width;
                    let text = std::str::from_utf8(self.bytes.get(start..end)?).ok()?;
                    out.push_str(text);
                    self.pos = end;
                }
            }
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        // The state file only uses integers; reject fractions/exponents rather
        // than silently truncating them.
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return None;
        }
        let text = std::str::from_utf8(self.bytes.get(start..self.pos)?).ok()?;
        text.parse::<i64>().ok().map(Json::Number)
    }

    fn array(&mut self) -> Option<Json> {
        self.enter()?;
        self.bump()?; // [
        let mut values = Vec::new();
        self.skip_ws();
        if self.peek()? == b']' {
            self.bump()?;
            self.leave();
            return Some(Json::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.skip_ws();
            match self.bump()? {
                b',' => continue,
                b']' => break,
                _ => return None,
            }
        }
        self.leave();
        Some(Json::Array(values))
    }

    fn object(&mut self) -> Option<Json> {
        self.enter()?;
        self.bump()?; // {
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek()? == b'}' {
            self.bump()?;
            self.leave();
            return Some(Json::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.bump()? != b':' {
                return None;
            }
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.bump()? {
                b',' => continue,
                b'}' => break,
                _ => return None,
            }
        }
        self.leave();
        Some(Json::Object(entries))
    }

    fn enter(&mut self) -> Option<()> {
        self.depth += 1;
        if self.depth > 32 { None } else { Some(()) }
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

fn utf8_width(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first < 0xE0 {
        2
    } else if first < 0xF0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_state_file() {
        let text = r#"{"v":1,"seq":4,"focus":{"id":"org.gnome.Nautilus.desktop","name":"Files \"quoted\"","wm_class":"org.gnome.Nautilus","pid":12},"away":false,"away_since":0,"idle":false,"locked":false}"#;
        let value = parse(text).unwrap();
        assert_eq!(value.get("v").and_then(Json::as_i64), Some(1));
        let focus = value.get("focus").unwrap();
        assert_eq!(
            focus.get("id").and_then(Json::as_str),
            Some("org.gnome.Nautilus.desktop")
        );
        assert_eq!(
            focus.get("name").and_then(Json::as_str),
            Some("Files \"quoted\"")
        );
        assert_eq!(value.get("away").and_then(Json::as_bool), Some(false));
    }

    #[test]
    fn handles_unicode_escapes() {
        let value = parse(r#"{"name":"caf\u00e9 \ud83c\udf1e"}"#).unwrap();
        assert_eq!(value.get("name").and_then(Json::as_str), Some("café 🌞"));
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(parse("not json").is_none());
        assert!(parse(r#"{"a":}"#).is_none());
        assert!(parse(r#"{"a":1"#).is_none());
        assert!(parse(r#"{"a":1}x"#).is_none());
        assert!(parse(r#"{"a":1.5}"#).is_none());
    }

    #[test]
    fn handles_null_and_large_integers() {
        let value = parse(r#"{"focus":null,"stamp":1710000000123}"#).unwrap();
        assert!(matches!(value.get("focus"), Some(Json::Null)));
        assert_eq!(
            value.get("stamp").and_then(Json::as_i64),
            Some(1710000000123)
        );
    }
}
