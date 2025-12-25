use std::borrow::Cow;
use std::collections::BTreeMap;
use toml_parser::Span;
use toml_parser::decoder::Encoding;

pub struct Edits {
    edits: BTreeMap<Span, Cow<'static, str>>,
    len: usize,
}
impl Edits {
    pub fn new(len: usize) -> Self {
        Self {
            edits: BTreeMap::new(),
            len,
        }
    }
    pub fn end(&self) -> usize {
        self.len
    }
    pub fn insert(&mut self, offset: usize, content: impl Into<Cow<'static, str>>) {
        self.replace(Span::new_unchecked(offset, offset), content);
    }
    pub fn delete(&mut self, span: Span) {
        self.replace(span, "");
    }
    pub fn replace(&mut self, span: Span, content: impl Into<Cow<'static, str>>) {
        self.edits.insert(span, content.into());
    }
    pub fn delete_all(&mut self, spans: impl IntoIterator<Item = Span>) {
        self.extend(spans.into_iter().map(|s| (s, "")));
    }
    pub fn replace_all(
        &mut self,
        spans: impl IntoIterator<Item = Span>,
        with: impl Into<Cow<'static, str>>,
    ) {
        let with = with.into();
        let mut it = spans.into_iter().peekable();
        while let Some(span) = it.next() {
            if it.peek().is_some() {
                self.replace(span, with.clone());
            } else {
                self.replace(span, with);
                break;
            }
        }
    }
    pub fn apply(&self, input: &str) -> String {
        tracing::debug!(edits = ?self.edits, "appying edits");
        let cap = self
            .edits
            .iter()
            .fold(input.len(), |len, (old, new)| len + new.len() - old.len());
        let mut out = String::with_capacity(cap);
        let mut last = 0;
        for (old, new) in &self.edits {
            if last < old.start() {
                out.push_str(&input[last..old.start()]);
            }
            out.push_str(new);
            last = old.end();
        }
        out.push_str(&input[last..]);
        out
    }
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }
    pub fn clear(&mut self) {
        self.edits.clear();
    }
}
impl<C: Into<Cow<'static, str>>> Extend<(Span, C)> for Edits {
    fn extend<T: IntoIterator<Item = (Span, C)>>(&mut self, iter: T) {
        self.edits
            .extend(iter.into_iter().map(|(s, c)| (s, c.into())));
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
        Encoding::MlBasicString => {
            *encoding = Encoding::BasicString;
            format_basic_string(s)
        }
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
pub fn format_key(s: &str, encoding: &mut Option<Encoding>) -> String {
    match encoding {
        None => {
            for ch in s.chars() {
                if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                    continue;
                }
                if ch == '\'' || ch == '\n' {
                    *encoding = Some(Encoding::BasicString);
                    return format_basic_string(s);
                }
                *encoding = Some(Encoding::LiteralString);
            }
            if encoding.is_none() {
                s.to_string()
            } else {
                format!("'{s}'")
            }
        }
        Some(Encoding::BasicString) => format_basic_string(s),
        Some(Encoding::LiteralString) => {
            if s.chars().all(|ch| ch != '\'' && ch != '\n') {
                format!("'{s}'")
            } else {
                *encoding = Some(Encoding::BasicString);
                format_basic_string(s)
            }
        }
        Some(Encoding::MlBasicString) => {
            *encoding = Some(Encoding::BasicString);
            format_basic_string(s)
        }
        Some(Encoding::MlLiteralString) => {
            if s.chars().all(|ch| ch != '\'' && ch != '\n') {
                *encoding = Some(Encoding::LiteralString);
                format!("'{s}'")
            } else {
                *encoding = Some(Encoding::BasicString);
                format_basic_string(s)
            }
        }
    }
}
