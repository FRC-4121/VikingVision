use std::borrow::Cow;
use std::collections::BTreeMap;
use toml_parser::Span;
use toml_parser::decoder::Encoding;

#[derive(Default)]
pub struct Edits(BTreeMap<Span, Cow<'static, str>>);
impl Edits {
    pub fn add(&mut self, span: Span, content: impl Into<Cow<'static, str>>) {
        self.0.insert(span, content.into());
    }
    pub fn apply(&self, input: &str) -> String {
        let cap = self
            .0
            .iter()
            .fold(input.len(), |len, (old, new)| len + new.len() - old.len());
        let mut out = String::with_capacity(cap);
        let mut last = 0;
        for (old, new) in &self.0 {
            out.push_str(&input[last..old.start()]);
            out.push_str(new);
            last = old.end();
        }
        out.push_str(&input[last..]);
        out
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}
fn format_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch), // TODO: maybe some non-printables?
        }
    }
    out.push('"');
    out
}
pub fn format_string(s: &str, encoding: &mut Encoding) -> String {
    match encoding {
        Encoding::BasicString => format_basic_string(s),
        Encoding::LiteralString => {
            if s.chars().all(|ch| ch != '\'' && ch != '\n') {
                format!("'{s}'")
            } else {
                *encoding = Encoding::BasicString;
                format_basic_string(s)
            }
        }
        Encoding::MlBasicString => format_basic_string(s),
        Encoding::MlLiteralString => {
            if s.chars().all(|ch| ch != '\'' && ch != '\n') {
                *encoding = Encoding::LiteralString;
                format!("'{s}'")
            } else {
                *encoding = Encoding::BasicString;
                format_basic_string(s)
            }
        }
    }
}
