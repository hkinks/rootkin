//! A small lossless XML tree.
//!
//! Text and attribute values are stored in their raw (escaped) form so that
//! an unmodified document serializes back byte-for-byte. Accessors unescape
//! leniently: the vendor XML contains `&#x0;`, which strict parsers reject.

use quick_xml::events::Event;
use quick_xml::Reader;
use std::borrow::Cow;

use crate::{Error, Result};

const BOM: &str = "\u{feff}";

#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Raw bytes before the root element (BOM, XML declaration, whitespace).
    pub prolog: String,
    pub root: Element,
    /// Raw bytes after the root element.
    pub epilog: String,
    /// Lookup tables for `edit`; not part of the document.
    pub(crate) index: crate::index::IndexCell,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    /// Raw, escaped text.
    Text(String),
    /// Comments, CDATA, processing instructions: kept verbatim.
    Raw(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub name: String,
    /// `(name, raw escaped value)` in document order.
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Document {
    pub fn parse(xml: &[u8]) -> Result<Self> {
        let xml = std::str::from_utf8(xml).map_err(|e| Error::Xml(e.to_string()))?;
        let (bom, body) = match xml.strip_prefix(BOM) {
            Some(rest) => (BOM, rest),
            None => ("", xml),
        };
        let mut reader = Reader::from_str(body);
        reader.config_mut().trim_text(false);
        reader.config_mut().check_end_names = true;

        let mut prolog = bom.to_string();
        let mut epilog = String::new();
        let mut stack: Vec<Element> = Vec::new();
        let mut root: Option<Element> = None;

        loop {
            let start = reader.buffer_position() as usize;
            let event = reader.read_event().map_err(|e| xml_err(&reader, e))?;
            let raw = &body[start..reader.buffer_position() as usize];
            match event {
                Event::Start(e) => {
                    // writing, cloning and dropping recurse: refuse what would overflow the stack
                    if stack.len() >= crate::limits::MAX_DEPTH {
                        return Err(Error::Xml(crate::limits::too_deep("the XML")));
                    }
                    stack.push(Element::from_start(&e)?)
                }
                Event::Empty(e) => {
                    let el = Element::from_start(&e)?;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(Node::Element(el)),
                        None => root = Some(el),
                    }
                }
                Event::End(_) => {
                    let el = stack.pop().ok_or_else(|| Error::Xml("unbalanced end tag".into()))?;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(Node::Element(el)),
                        None => root = Some(el),
                    }
                }
                Event::Text(t) => match stack.last_mut() {
                    Some(parent) => {
                        let raw_text = std::str::from_utf8(&t).map_err(|e| Error::Xml(e.to_string()))?;
                        parent.children.push(Node::Text(raw_text.to_string()));
                    }
                    None => outside(&root, &mut prolog, &mut epilog).push_str(raw),
                },
                Event::Eof => break,
                _ => match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::Raw(raw.to_string())),
                    None => outside(&root, &mut prolog, &mut epilog).push_str(raw),
                },
            }
        }
        let root = root.ok_or_else(|| Error::Xml("no root element".into()))?;
        Ok(Document { prolog, root, epilog, index: Default::default() })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = String::with_capacity(512 * 1024);
        out.push_str(&self.prolog);
        self.root.write(&mut out);
        out.push_str(&self.epilog);
        out.into_bytes()
    }
}

fn outside<'a>(root: &Option<Element>, prolog: &'a mut String, epilog: &'a mut String) -> &'a mut String {
    if root.is_some() {
        epilog
    } else {
        prolog
    }
}

fn xml_err(reader: &Reader<&[u8]>, e: quick_xml::Error) -> Error {
    Error::Xml(format!("at byte {}: {e}", reader.buffer_position()))
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Element { name: name.into(), attrs: Vec::new(), children: Vec::new() }
    }

    fn from_start(e: &quick_xml::events::BytesStart) -> Result<Self> {
        let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
        let mut attrs = Vec::new();
        for a in e.attributes().with_checks(false) {
            let a = a.map_err(|err| Error::Xml(err.to_string()))?;
            attrs.push((
                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                String::from_utf8_lossy(&a.value).into_owned(),
            ));
        }
        Ok(Element { name, attrs, children: Vec::new() })
    }

    fn write(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(v);
            out.push('"');
        }
        if self.children.is_empty() {
            // .NET XmlSerializer style
            out.push_str(" />");
            return;
        }
        out.push('>');
        for child in &self.children {
            match child {
                Node::Element(el) => el.write(out),
                Node::Text(t) | Node::Raw(t) => out.push_str(t),
            }
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push('>');
    }

    // ---- attributes ----

    pub fn attr(&self, name: &str) -> Option<Cow<'_, str>> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| unescape(v))
    }

    pub fn set_attr(&mut self, name: &str, value: &str) {
        let escaped = escape(value, true);
        match self.attrs.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => *v = escaped,
            None => self.attrs.push((name.to_string(), escaped)),
        }
    }

    pub fn with_attr(mut self, name: &str, value: &str) -> Self {
        self.set_attr(name, value);
        self
    }

    // ---- children ----

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut Element> {
        self.children.iter_mut().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            _ => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.elements_mut().find(|e| e.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// Follow a `/`-separated path of child element names.
    pub fn path(&self, path: &str) -> Option<&Element> {
        path.split('/').try_fold(self, |el, name| el.child(name))
    }

    pub fn path_mut(&mut self, path: &str) -> Option<&mut Element> {
        path.split('/').try_fold(self, |el, name| el.child_mut(name))
    }

    /// Get the named child, creating it if missing. A new child is inserted
    /// before the first existing sibling whose name appears after `name` in
    /// `order` (the schema order XmlSerializer expects); otherwise appended.
    pub fn ensure_child(&mut self, name: &str, order: &[&str]) -> &mut Element {
        let idx = match self.children.iter().position(|n| matches!(n, Node::Element(e) if e.name == name)) {
            Some(i) => i,
            None => {
                let rank = |n: &str| order.iter().position(|o| *o == n);
                let insert_at = rank(name)
                    .and_then(|r| {
                        self.children
                            .iter()
                            .position(|n| matches!(n, Node::Element(e) if rank(&e.name).is_some_and(|er| er > r)))
                    })
                    .unwrap_or(self.children.len());
                self.children.insert(insert_at, Node::Element(Element::new(name)));
                insert_at
            }
        };
        match &mut self.children[idx] {
            Node::Element(e) => e,
            _ => unreachable!(),
        }
    }

    pub fn push(&mut self, el: Element) -> &mut Element {
        self.children.push(Node::Element(el));
        match self.children.last_mut() {
            Some(Node::Element(e)) => e,
            _ => unreachable!(),
        }
    }

    pub fn remove_children_where(&mut self, mut pred: impl FnMut(&Element) -> bool) -> usize {
        let before = self.children.len();
        self.children.retain(|n| !matches!(n, Node::Element(e) if pred(e)));
        before - self.children.len()
    }

    // ---- text ----

    /// Unescaped concatenation of direct text children.
    pub fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(unescape(t)),
                _ => None,
            })
            .collect()
    }

    pub fn set_text(&mut self, value: &str) {
        self.children.retain(|n| !matches!(n, Node::Text(_)));
        if !value.is_empty() {
            self.children.insert(0, Node::Text(escape(value, false)));
        }
    }

    pub fn child_text(&self, path: &str) -> Option<String> {
        self.path(path).map(Element::text)
    }
}

/// Lenient XML unescape: unknown entities are left as-is.
pub fn unescape(raw: &str) -> Cow<'_, str> {
    if !raw.contains('&') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';') else { break };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .map(|h| u32::from_str_radix(h, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse::<u32>))
                .and_then(|r| r.ok())
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

pub fn escape(value: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\n' if attribute => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => out.push_str(&format!("&#x{:X};", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_is_byte_exact() {
        let xml =
            "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?><A x=\"1&#x0;2\"><B /><C>t &amp; u</C><!-- c --></A>";
        let doc = Document::parse(xml.as_bytes()).unwrap();
        assert_eq!(String::from_utf8(doc.to_bytes()).unwrap(), xml);
        assert_eq!(doc.root.attr("x").unwrap(), "1\u{0}2");
        assert_eq!(doc.root.child_text("C").unwrap(), "t & u");
    }

    #[test]
    fn ensure_child_respects_order() {
        let mut el = Element::new("Name");
        el.push(Element::new("Names"));
        el.push(Element::new("Suffix"));
        el.ensure_child("Surname", &["Names", "SurnamePrefix", "Surname", "Suffix"]);
        let names: Vec<_> = el.elements().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Names", "Surname", "Suffix"]);
    }

    #[test]
    fn escape_roundtrips_through_unescape() {
        let s = "a<b>&\"c\"\u{0}";
        assert_eq!(unescape(&escape(s, true)), s);
    }
}
