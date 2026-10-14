//! One XML walker. The root tag selects [`dict::SchemaDict`].
//!
//! Bytes are decoded before parsing. A UTF-8 or UTF-16 BOM wins over the
//! declaration; otherwise the `encoding` in the XML declaration is used
//! (`Shift_JIS` included), and UTF-8 is the default. The parser is quick-xml
//! with only the predefined entities. A `DOCTYPE` is ignored, and an unknown
//! `&name;` becomes a diagnostic. Nothing in this module opens a path named by
//! the document.

mod actions;
mod behaviors;
mod dict;
mod dict_en;
mod dict_ja;

use std::borrow::Cow;

use encoding_rs::Encoding;
use quick_xml::XmlVersion;
use quick_xml::escape::EscapeError;
use quick_xml::events::{BytesCData, BytesRef, BytesStart, BytesText, Event};
use quick_xml::name::QName;
use quick_xml::reader::Reader;
use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use shimeji_core::model::{
    Action, Behavior, BehaviorNode, Constant, Information, OrderedMap, Param, Schema, ScriptSource,
};

use self::dict::{CanonicalAttr, SchemaDict};

const MAX_DEPTH: usize = 64;

pub(crate) struct Elem {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Elem>,
    pub text: String,
    pub line: u32,
}

struct Frame {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Elem>,
    text: String,
    line: u32,
}

impl Frame {
    fn into_elem(self) -> Elem {
        Elem {
            name: self.name,
            attrs: self.attrs,
            children: self.children,
            text: self.text,
            line: self.line,
        }
    }
}

pub(crate) struct Ctx<'a> {
    pub pack: &'a str,
    pub file: &'a str,
    pub dict: SchemaDict,
    pub diags: &'a mut Vec<Diagnostic>,
}

impl Ctx<'_> {
    pub(crate) fn at(
        &mut self,
        severity: Severity,
        kind: DiagnosticKind,
        line: u32,
        message: impl Into<String>,
    ) {
        self.diags.push(Diagnostic::new(
            severity,
            kind,
            self.pack,
            self.file,
            Some(line),
            message,
        ));
    }
}

pub(crate) struct LoadedDoc {
    pub schema: Schema,
    pub constants: OrderedMap<Constant>,
    pub actions: OrderedMap<Action>,
    pub behaviors: OrderedMap<Behavior>,
    pub behavior_tree: Vec<BehaviorNode>,
    pub info: Information,
}

impl LoadedDoc {
    fn new(schema: Schema) -> Self {
        Self {
            schema,
            constants: OrderedMap::new(),
            actions: OrderedMap::new(),
            behaviors: OrderedMap::new(),
            behavior_tree: Vec::new(),
            info: Information::default(),
        }
    }
}

/// Parse one configuration document.
///
/// `Ok(None)` means the file is not a usable document. Soft problems are
/// diagnostics on `Ok(Some(_))`.
pub(crate) fn parse_document(
    bytes: &[u8],
    pack: &str,
    file: &str,
) -> (Option<LoadedDoc>, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let text = decode_xml(bytes);
    let lines = newline_starts(&text);
    let root = match walk(&text, &lines, pack, file, &mut diags) {
        Ok(root) => root,
        Err(message) => {
            let line = diags.last().and_then(|diag| diag.line);
            if diags.iter().all(|diag| diag.kind != DiagnosticKind::Xml) {
                diags.push(xml_diag(pack, file, line, message));
            }
            return (None, diags);
        }
    };
    match interpret(&root, pack, file, &mut diags) {
        Ok(doc) => (Some(doc), diags),
        Err(()) => (None, diags),
    }
}

fn interpret(
    root: &Elem,
    pack: &str,
    file: &str,
    diags: &mut Vec<Diagnostic>,
) -> Result<LoadedDoc, ()> {
    if root.name == "Information" {
        let mut doc = LoadedDoc::new(Schema::English);
        let mut ctx = Ctx {
            pack,
            file,
            dict: SchemaDict::English,
            diags,
        };
        behaviors::load_information(&mut ctx, root, &mut doc.info);
        return Ok(doc);
    }
    let Some(dict) = SchemaDict::from_root(&root.name) else {
        diags.push(xml_diag(
            pack,
            file,
            Some(root.line),
            format!("unrecognized root <{}>", root.name),
        ));
        return Err(());
    };
    let mut doc = LoadedDoc::new(dict.schema());
    let mut ctx = Ctx {
        pack,
        file,
        dict,
        diags,
    };
    for child in &root.children {
        match ctx.dict.tag(&child.name) {
            dict::Tag::Constant => behaviors::load_constant(&mut ctx, child, &mut doc),
            dict::Tag::ActionList => actions::load_action_list(&mut ctx, child, &mut doc),
            dict::Tag::BehaviorList => behaviors::load_behavior_list(&mut ctx, child, &mut doc),
            dict::Tag::Information => behaviors::load_information(&mut ctx, child, &mut doc.info),
            _ => {}
        }
    }
    Ok(doc)
}

fn xml_diag(pack: &str, file: &str, line: Option<u32>, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(
        Severity::Error,
        DiagnosticKind::Xml,
        pack,
        file,
        line,
        message,
    )
}

fn walk(
    text: &str,
    lines: &[usize],
    pack: &str,
    file: &str,
    diags: &mut Vec<Diagnostic>,
) -> Result<Elem, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Frame> = Vec::new();
    let mut root: Option<Elem> = None;
    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(error) => {
                let line = line_of(lines, reader.error_position() as usize);
                diags.push(xml_diag(pack, file, Some(line), format!("xml: {error}")));
                return Err(error.to_string());
            }
        };
        let event_at = reader.buffer_position();
        // `buffer_position` is the byte after this event. The byte before it
        // still belongs to the event, so a tag on its own line keeps that line.
        let line = line_of(lines, (event_at as usize).saturating_sub(1));
        match event {
            Event::Start(tag) => {
                if stack.len() >= MAX_DEPTH {
                    diags.push(xml_diag(
                        pack,
                        file,
                        Some(line),
                        format!("xml nested deeper than {MAX_DEPTH}"),
                    ));
                    return Err("depth".to_string());
                }
                stack.push(Frame {
                    name: local_name(tag.name()),
                    attrs: take_attrs(&tag, pack, file, line, diags),
                    children: Vec::new(),
                    text: String::new(),
                    line,
                });
            }
            Event::Empty(tag) => {
                let elem = Elem {
                    name: local_name(tag.name()),
                    attrs: take_attrs(&tag, pack, file, line, diags),
                    children: Vec::new(),
                    text: String::new(),
                    line,
                };
                attach(&mut stack, &mut root, elem)?;
            }
            Event::End(_) => {
                let Some(frame) = stack.pop() else {
                    diags.push(xml_diag(pack, file, Some(line), "unexpected end tag"));
                    return Err("end".to_string());
                };
                attach(&mut stack, &mut root, frame.into_elem())?;
            }
            Event::Text(text) => push_text(&mut stack, &text),
            Event::CData(text) => push_cdata(&mut stack, &text),
            Event::GeneralRef(reference) => {
                push_reference(&mut stack, &reference, pack, file, line, diags);
            }
            Event::Eof => break,
            Event::DocType(_) | Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
        }
    }
    if !stack.is_empty() {
        diags.push(xml_diag(pack, file, None, "unclosed element"));
        return Err("unclosed".to_string());
    }
    root.ok_or_else(|| {
        diags.push(xml_diag(pack, file, None, "empty document"));
        "empty".to_string()
    })
}

fn attach(stack: &mut [Frame], root: &mut Option<Elem>, elem: Elem) -> Result<(), String> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(elem);
        Ok(())
    } else if root.is_none() {
        *root = Some(elem);
        Ok(())
    } else {
        Err("multiple roots".to_string())
    }
}

fn push_text(stack: &mut [Frame], text: &BytesText<'_>) {
    if let Some(frame) = stack.last_mut() {
        if let Ok(value) = text.xml10_content() {
            frame.text.push_str(&value);
        }
    }
}

fn push_cdata(stack: &mut [Frame], text: &BytesCData<'_>) {
    if let Some(frame) = stack.last_mut() {
        if let Ok(value) = text.xml10_content() {
            frame.text.push_str(&value);
        }
    }
}

fn push_reference(
    stack: &mut [Frame],
    reference: &BytesRef<'_>,
    pack: &str,
    file: &str,
    line: u32,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(frame) = stack.last_mut() else {
        return;
    };
    match reference.resolve_char_ref() {
        Ok(Some(ch)) => {
            frame.text.push(ch);
            return;
        }
        Ok(None) => {}
        Err(error) => {
            diags.push(xml_diag(
                pack,
                file,
                Some(line),
                format!("bad character reference: {error}"),
            ));
            return;
        }
    }
    let name = reference.decode().unwrap_or(Cow::Borrowed("")).into_owned();
    match name.as_str() {
        "lt" => frame.text.push('<'),
        "gt" => frame.text.push('>'),
        "amp" => frame.text.push('&'),
        "quot" => frame.text.push('"'),
        "apos" => frame.text.push('\''),
        _ => {
            diags.push(Diagnostic::new(
                Severity::Error,
                DiagnosticKind::UnresolvedEntity,
                pack,
                file,
                Some(line),
                format!("unresolved entity &{name};"),
            ));
        }
    }
}

fn take_attrs(
    tag: &BytesStart<'_>,
    pack: &str,
    file: &str,
    line: u32,
    diags: &mut Vec<Diagnostic>,
) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    for attr in tag.attributes() {
        let attr = match attr {
            Ok(attr) => attr,
            Err(error) => {
                diags.push(xml_diag(
                    pack,
                    file,
                    Some(line),
                    format!("attribute: {error}"),
                ));
                continue;
            }
        };
        let raw_key = String::from_utf8_lossy(attr.key.as_ref());
        if raw_key == "xmlns" || raw_key.starts_with("xmlns:") {
            continue;
        }
        let name = local_name(attr.key);
        let value = match attr.normalized_value(XmlVersion::Implicit1_0) {
            Ok(value) => value.into_owned(),
            Err(error) => {
                if let quick_xml::Error::Escape(EscapeError::UnrecognizedEntity(_, entity)) = &error
                {
                    diags.push(Diagnostic::new(
                        Severity::Error,
                        DiagnosticKind::UnresolvedEntity,
                        pack,
                        file,
                        Some(line),
                        format!("unresolved entity &{entity};"),
                    ));
                } else {
                    diags.push(xml_diag(
                        pack,
                        file,
                        Some(line),
                        format!("attribute {name}: {error}"),
                    ));
                }
                String::from_utf8_lossy(&attr.value).into_owned()
            }
        };
        attrs.push((name, value));
    }
    attrs
}

fn local_name(name: QName<'_>) -> String {
    String::from_utf8_lossy(name.local_name().as_ref()).into_owned()
}

pub(crate) fn raw_attr(dict: SchemaDict, elem: &Elem, want: CanonicalAttr) -> Option<&str> {
    let mut found = None;
    for (name, value) in &elem.attrs {
        if dict.attr(name) == Some(want) {
            found = Some(value.as_str());
        }
    }
    found
}

pub(crate) fn script_source(ctx: &mut Ctx<'_>, raw: &str, line: u32, what: &str) -> ScriptSource {
    let (source, error) = ScriptSource::parse_attribute(raw);
    if let Some(error) = error {
        ctx.at(
            Severity::Error,
            DiagnosticKind::Expression,
            line,
            format!("{what}: {error}"),
        );
    }
    source
}

pub(crate) fn collect_params(ctx: &mut Ctx<'_>, elem: &Elem) -> OrderedMap<Param> {
    let mut map = OrderedMap::new();
    for (name, value) in &elem.attrs {
        let key = match ctx.dict.attr(name) {
            Some(attr) => attr.key().to_string(),
            None => name.clone(),
        };
        let script = script_source(ctx, value, elem.line, name);
        map.insert(
            key.clone(),
            Param {
                key,
                source_name: name.clone(),
                value: script,
                line: elem.line,
            },
        );
    }
    map
}

pub(crate) fn strip_structural(dict: SchemaDict, params: OrderedMap<Param>) -> OrderedMap<Param> {
    let mut kept = OrderedMap::new();
    for (key, param) in params.into_pairs() {
        let structural = dict
            .attr(&param.source_name)
            .is_some_and(CanonicalAttr::is_behavior_structural);
        if !structural {
            kept.insert(key, param);
        }
    }
    kept
}

/// Java `Boolean.parseBoolean`: only the letters `true`, ignoring case, are true.
pub(crate) fn java_bool(value: &str) -> bool {
    value.eq_ignore_ascii_case("true")
}

pub(crate) fn parse_i32_trim(text: &str) -> Option<i32> {
    text.trim().parse().ok()
}

/// `Integer.parseInt`, then the reference's `frequency < 0` rejection.
/// Surrounding whitespace is accepted; the reference does not trim.
pub(crate) fn parse_frequency(text: &str) -> Option<i32> {
    let number = parse_i32_trim(text)?;
    if number < 0 { None } else { Some(number) }
}

pub(crate) fn parse_pair(text: &str) -> Option<(i32, i32)> {
    let mut parts = text.split(',');
    let x = parts.next()?.trim().parse().ok()?;
    let y = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}

fn decode_xml(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return decode_with(encoding_rs::UTF_8, &bytes[3..]);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_with(encoding_rs::UTF_16LE, bytes);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_with(encoding_rs::UTF_16BE, bytes);
    }
    let encoding = sniff_encoding(bytes).unwrap_or(encoding_rs::UTF_8);
    decode_with(encoding, bytes)
}

fn decode_with(encoding: &'static Encoding, bytes: &[u8]) -> String {
    let (text, _, _) = encoding.decode(bytes);
    let mut text = text.into_owned();
    if text.starts_with('\u{feff}') {
        text.remove(0);
    }
    text
}

fn sniff_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = &bytes[..bytes.len().min(240)];
    let lower: Vec<u8> = head.iter().map(|byte| byte.to_ascii_lowercase()).collect();
    let start = lower.windows(8).position(|window| window == b"encoding")?;
    let mut index = start + 8;
    while index < head.len() && head[index].is_ascii_whitespace() {
        index += 1;
    }
    if index >= head.len() || head[index] != b'=' {
        return None;
    }
    index += 1;
    while index < head.len() && head[index].is_ascii_whitespace() {
        index += 1;
    }
    if index >= head.len() {
        return None;
    }
    let quote = head[index];
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    index += 1;
    let end = head[index..].iter().position(|byte| *byte == quote)? + index;
    let label = std::str::from_utf8(&head[index..end]).ok()?;
    Encoding::for_label(label.as_bytes())
}

fn newline_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn line_of(starts: &[usize], byte: usize) -> u32 {
    match starts.binary_search(&byte) {
        Ok(index) => (index as u32) + 1,
        Err(index) => index.max(1) as u32,
    }
}
