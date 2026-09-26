//! The Web card (WEB-001): an address, a page, and its links.
//!
//! Pages come from the kernel's HTTP fetch (NET-033) and are read here
//! as text: headings, paragraphs, lists, preformatted blocks and links,
//! with scripts, styles and markup left out. It is a reader, not a
//! browser engine -- no layout, no images, no scripts -- which is what a
//! plain-text desk can show honestly, and what most of a page is for.
//!
//! The card is drawn with the desk's widgets: an address bar with Back and
//! Go, then the page, wrapped to the card. Links are the accent colour,
//! underlined; a click follows one, as do Tab and Enter. Redirects are
//! followed (plain HTTP only: there is no TLS yet, and the card says so).

extern crate alloc;

use crate::line_edit::{Edit, LineEdit};
use crate::widgets::{rect, ButtonKind, Palette, Ui, GLYPH_W};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

/// One row of the page, in pixels.
pub const ROW: u32 = 20;
/// The address bar's height, and the gap under it.
pub const BAR_H: u32 = 30;
const TOP: u32 = BAR_H + 10;
/// A click on the address field.
pub const KEY_ADDRESS: u8 = 0xB1;
/// Back, and Go (or Reload once a page is up).
pub const KEY_BACK: u8 = 0xB2;
pub const KEY_GO: u8 = 0xB3;
/// Ctrl+R: the page again.
pub const KEY_RELOAD: u8 = 0x12;
/// Visible link `n` answers `LINK_KEY_FIRST + n`.
pub const LINK_KEY_FIRST: u8 = 0xC0;
const LINK_KEYS: usize = 64;
/// Redirects followed before giving up.
pub const MAX_REDIRECTS: u8 = 5;
/// Pages kept to go back to.
const BACK_MAX: usize = 32;

/// A run of text, part of a link or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub link: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Text,
    Heading,
    /// Preformatted: kept as written, wrapped hard.
    Pre,
    /// An empty line between blocks.
    Blank,
    /// `<hr>`.
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
    /// Columns in from the left: nested lists and quotes (WEB-002).
    pub indent: u8,
    /// Further in for the rows after the first: a list item's text hangs
    /// under its own start, not under its bullet.
    pub hang: u8,
}

/// A page, read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Page {
    pub url: String,
    pub title: String,
    pub blocks: Vec<Block>,
    /// Where each link goes, absolute.
    pub links: Vec<String>,
    pub status: u16,
}

/// What the kernel's fetch came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebResponse {
    /// The URL asked for, absolute.
    pub url: String,
    pub status: u16,
    pub reason: String,
    pub location: Option<String>,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

/// A fetch's end: the response, or why there is none.
pub type WebOutcome = Result<WebResponse, String>;

// ---- URLs ----

/// `http://`, `host:port` and the path of an absolute URL, split.
fn split_url(url: &str) -> Option<(&str, &str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    let at = rest.find('/').unwrap_or(rest.len());
    Some((scheme, &rest[..at], &rest[at..]))
}

/// What the user typed, as a URL: `http://` is assumed when no scheme is.
pub fn normalize(typed: &str) -> String {
    let typed = typed.trim();
    if typed.contains("://") {
        String::from(typed)
    } else {
        alloc::format!("http://{typed}")
    }
}

/// `href` on the page at `base`, absolute; `None` for links that go
/// nowhere a fetch can (`#fragment`, `mailto:`, `javascript:`).
pub fn resolve_url(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    let href = href.split('#').next().unwrap_or("");
    if href.is_empty() {
        return None;
    }
    if let Some((scheme, _)) = href.split_once(':') {
        let is_scheme = !scheme.is_empty()
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.');
        if is_scheme {
            return match scheme.to_ascii_lowercase().as_str() {
                "http" | "https" => Some(String::from(href)),
                _ => None,
            };
        }
    }
    let (scheme, authority, base_path) = split_url(base)?;
    if let Some(rest) = href.strip_prefix("//") {
        return Some(alloc::format!("{scheme}://{rest}"));
    }
    let path = if href.starts_with('/') {
        String::from(href)
    } else if href.starts_with('?') {
        let path = base_path.split('?').next().unwrap_or("/");
        alloc::format!("{}{href}", if path.is_empty() { "/" } else { path })
    } else {
        let path = base_path.split('?').next().unwrap_or("");
        let dir = match path.rfind('/') {
            Some(at) => &path[..=at],
            None => "/",
        };
        alloc::format!("{dir}{href}")
    };
    Some(alloc::format!(
        "{scheme}://{authority}{}",
        remove_dots(&path)
    ))
}

/// `a/./b/../c` as `a/c` (RFC 3986 5.2.4), the query left alone.
fn remove_dots(path: &str) -> String {
    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path.split('/').collect();
    let last = segments.len().saturating_sub(1);
    for (i, seg) in segments.iter().enumerate() {
        match *seg {
            "." => {
                if i == last {
                    out.push("");
                }
            }
            ".." => {
                if out.len() > 1 {
                    out.pop();
                }
                if i == last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let mut joined = out.join("/");
    if !joined.starts_with('/') {
        joined.insert(0, '/');
    }
    if let Some(q) = query {
        joined.push('?');
        joined.push_str(q);
    }
    joined
}

// ---- Reading HTML ----

/// A character the desk's font can draw, for one it cannot: the font is
/// ASCII, and a page is not.
fn ascii_of(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{a0}' | '\u{2002}' | '\u{2003}' | '\u{2009}' => " ",
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{2032}' => "'",
        '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{2033}' | '\u{ab}' | '\u{bb}' => "\"",
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2212}' => "-",
        '\u{2014}' | '\u{2015}' => "--",
        '\u{2026}' => "...",
        '\u{2022}' | '\u{b7}' | '\u{2027}' => "*",
        '\u{a9}' => "(c)",
        '\u{ae}' => "(R)",
        '\u{2122}' => "(TM)",
        '\u{d7}' => "x",
        '\u{2192}' => "->",
        '\u{2190}' => "<-",
        '\u{e0}'..='\u{e5}' => "a",
        '\u{c0}'..='\u{c5}' => "A",
        '\u{e8}'..='\u{eb}' => "e",
        '\u{c8}'..='\u{cb}' => "E",
        '\u{ec}'..='\u{ef}' => "i",
        '\u{f2}'..='\u{f6}' | '\u{f8}' => "o",
        '\u{d2}'..='\u{d6}' | '\u{d8}' => "O",
        '\u{f9}'..='\u{fc}' => "u",
        '\u{d9}'..='\u{dc}' => "U",
        '\u{e7}' => "c",
        '\u{c7}' => "C",
        '\u{f1}' => "n",
        '\u{d1}' => "N",
        '\u{df}' => "ss",
        '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' => "",
        _ => return None,
    })
}

/// `&amp;`, `&#39;`, `&#x2014;` and the other entities a page uses, in `s`.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let end = tail[1..]
            .find(|c: char| c == ';' || c == '&' || c.is_whitespace() || c == '<')
            .map(|e| e + 1);
        let decoded = match end {
            Some(end) if tail.as_bytes().get(end) == Some(&b';') => {
                let name = &tail[1..end];
                let c = match name {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    "copy" => Some('\u{a9}'),
                    "reg" => Some('\u{ae}'),
                    "trade" => Some('\u{2122}'),
                    "mdash" => Some('\u{2014}'),
                    "ndash" => Some('\u{2013}'),
                    "hellip" => Some('\u{2026}'),
                    "lsquo" => Some('\u{2018}'),
                    "rsquo" => Some('\u{2019}'),
                    "ldquo" => Some('\u{201c}'),
                    "rdquo" => Some('\u{201d}'),
                    "bull" => Some('\u{2022}'),
                    "middot" => Some('\u{b7}'),
                    "times" => Some('\u{d7}'),
                    "rarr" => Some('\u{2192}'),
                    "larr" => Some('\u{2190}'),
                    "eacute" => Some('\u{e9}'),
                    "egrave" => Some('\u{e8}'),
                    "agrave" => Some('\u{e0}'),
                    "aacute" => Some('\u{e1}'),
                    "iacute" => Some('\u{ed}'),
                    "oacute" => Some('\u{f3}'),
                    "uacute" => Some('\u{fa}'),
                    "auml" => Some('\u{e4}'),
                    "ouml" => Some('\u{f6}'),
                    "uuml" => Some('\u{fc}'),
                    "ccedil" => Some('\u{e7}'),
                    "ntilde" => Some('\u{f1}'),
                    "szlig" => Some('\u{df}'),
                    _ => name
                        .strip_prefix("#x")
                        .or_else(|| name.strip_prefix("#X"))
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| name.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()))
                        .and_then(char::from_u32),
                };
                c.map(|c| (c, end + 1))
            }
            _ => None,
        };
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &tail[len..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Builds a page's blocks as the markup is read.
struct Reader {
    base: String,
    blocks: Vec<Block>,
    spans: Vec<Span>,
    kind: BlockKind,
    link: Option<usize>,
    links: Vec<String>,
    title: String,
    in_title: bool,
    pre: usize,
    /// The lists this text is in, innermost last: `None` for bullets, the
    /// next number for a numbered one (WEB-002).
    lists: Vec<Option<u32>>,
    /// Blockquotes this text is in.
    quote: usize,
    /// The hang of the block being built (a list item's marker width).
    hang: u8,
    /// A space is owed before the next word.
    space: bool,
    /// The row has had a cell: the next one is set off.
    cells: usize,
}

impl Reader {
    fn push_text(&mut self, text: &str) {
        let text = decode_entities(text);
        if self.in_title {
            self.title.push_str(&text);
            return;
        }
        if self.pre > 0 {
            let mut first = true;
            for line in text.split('\n') {
                if !first {
                    self.flush(BlockKind::Pre);
                }
                first = false;
                self.push_chars(line.trim_end_matches('\r'), true);
            }
            return;
        }
        let mut word = String::new();
        for c in text.chars() {
            if c.is_whitespace() && c != '\u{a0}' {
                if !word.is_empty() {
                    self.push_chars(&word, false);
                    word.clear();
                }
                self.space = true;
            } else {
                word.push(c);
            }
        }
        if !word.is_empty() {
            self.push_chars(&word, false);
        }
    }

    /// Append `text` to the current block, in the current link.
    fn push_chars(&mut self, text: &str, keep_spaces: bool) {
        let mut ascii = String::with_capacity(text.len());
        for c in text.chars() {
            match c {
                ' '..='~' => ascii.push(c),
                '\t' => ascii.push_str("    "),
                c => match ascii_of(c) {
                    Some(s) => ascii.push_str(s),
                    None => ascii.push('?'),
                },
            }
        }
        if ascii.is_empty() {
            return;
        }
        let started = self.spans.iter().any(|s| !s.text.is_empty());
        let lead = if self.space && started && !keep_spaces {
            " "
        } else {
            ""
        };
        self.space = false;
        match self.spans.last_mut() {
            Some(last) if last.link == self.link => {
                last.text.push_str(lead);
                last.text.push_str(&ascii);
            }
            _ => {
                // The space between a link and its neighbours is never
                // part of the link: entering one, it stays behind; leaving
                // one, it starts the text after.
                let text = if self.link.is_some() {
                    if !lead.is_empty() {
                        match self.spans.last_mut() {
                            Some(last) => last.text.push_str(lead),
                            None => self.spans.push(Span {
                                text: String::from(lead),
                                link: None,
                            }),
                        }
                    }
                    ascii
                } else {
                    alloc::format!("{lead}{ascii}")
                };
                self.spans.push(Span {
                    text,
                    link: self.link,
                });
            }
        }
    }

    /// End the current block as `kind`, if it has anything in it.
    fn flush(&mut self, kind: BlockKind) {
        let spans = core::mem::take(&mut self.spans);
        self.space = false;
        if spans.iter().any(|s| !s.text.trim().is_empty()) || kind == BlockKind::Pre {
            let indent = (self.lists.len().saturating_sub(1) * 3 + self.quote * 2).min(24) as u8;
            self.blocks.push(Block {
                kind,
                spans,
                indent,
                hang: self.hang,
            });
        }
        self.hang = 0;
        self.kind = if self.pre > 0 {
            BlockKind::Pre
        } else {
            BlockKind::Text
        };
    }

    /// End the block and leave one blank line -- never two, never first.
    fn paragraph(&mut self) {
        self.flush(self.kind);
        let last_blank = self
            .blocks
            .last()
            .is_none_or(|b| b.kind == BlockKind::Blank);
        if !last_blank {
            self.blocks.push(Block {
                kind: BlockKind::Blank,
                spans: Vec::new(),
                indent: 0,
                hang: 0,
            });
        }
    }

    fn tag(&mut self, name: &str, closing: bool, attrs: &str) {
        match name {
            "title" => self.in_title = !closing,
            "br" => self.flush(self.kind),
            "ul" | "ol" | "menu" => {
                // A list inside a list item continues it; one on its own is
                // set off like a paragraph.
                if self.lists.is_empty() {
                    self.paragraph();
                } else {
                    self.flush(self.kind);
                }
                if closing {
                    self.lists.pop();
                } else {
                    let start = attr(attrs, "start").and_then(|s| s.trim().parse::<u32>().ok());
                    self.lists.push((name == "ol").then(|| start.unwrap_or(1)));
                }
            }
            "blockquote" => {
                self.paragraph();
                if closing {
                    self.quote = self.quote.saturating_sub(1);
                } else {
                    self.quote += 1;
                }
            }
            "p" | "dl" | "table" | "figure" | "form" => self.paragraph(),
            "div" | "section" | "article" | "header" | "footer" | "nav" | "main" | "aside"
            | "tr" | "dt" | "dd" | "figcaption" | "address" | "center" | "summary" | "details"
            | "tbody" | "thead" => {
                self.flush(self.kind);
                self.cells = 0;
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if closing {
                    self.flush(BlockKind::Heading);
                    self.paragraph();
                } else {
                    self.paragraph();
                    self.kind = BlockKind::Heading;
                }
            }
            "li" => {
                self.flush(self.kind);
                if !closing {
                    // Numbered in a numbered list, a bullet otherwise.
                    let marker = match self.lists.last_mut() {
                        Some(Some(n)) => {
                            let m = alloc::format!("{n}. ");
                            *n += 1;
                            m
                        }
                        _ => String::from("- "),
                    };
                    self.hang = marker.len() as u8;
                    self.spans.push(Span {
                        text: marker,
                        link: None,
                    });
                }
            }
            "td" | "th" if !closing => {
                if self.cells > 0 {
                    self.push_chars("  ", true);
                }
                self.cells += 1;
            }
            "pre" => {
                self.flush(self.kind);
                if closing {
                    self.pre = self.pre.saturating_sub(1);
                    self.paragraph();
                } else {
                    self.paragraph();
                    self.pre += 1;
                }
                self.kind = if self.pre > 0 {
                    BlockKind::Pre
                } else {
                    BlockKind::Text
                };
            }
            "hr" => {
                self.flush(self.kind);
                self.blocks.push(Block {
                    kind: BlockKind::Rule,
                    spans: Vec::new(),
                    indent: 0,
                    hang: 0,
                });
            }
            "a" => {
                if closing {
                    self.link = None;
                } else {
                    self.link = attr(attrs, "href")
                        .and_then(|href| resolve_url(&self.base, &decode_entities(href)))
                        .map(|url| {
                            self.links.push(url);
                            self.links.len() - 1
                        });
                }
            }
            "img" => {
                if let Some(alt) = attr(attrs, "alt").filter(|a| !a.trim().is_empty()) {
                    let alt = alloc::format!("[{}]", decode_entities(alt).trim());
                    self.space = true;
                    self.push_chars(&alt, false);
                    self.space = true;
                }
            }
            _ => {}
        }
    }

    fn finish(mut self, url: &str, status: u16) -> Page {
        self.flush(self.kind);
        while self
            .blocks
            .last()
            .is_some_and(|b| b.kind == BlockKind::Blank)
        {
            self.blocks.pop();
        }
        let title = decode_entities(
            self.title
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .as_str(),
        );
        let mut ascii_title = String::new();
        for c in title.chars() {
            match c {
                ' '..='~' => ascii_title.push(c),
                c => ascii_title.push_str(ascii_of(c).unwrap_or("?")),
            }
        }
        Page {
            url: String::from(url),
            title: ascii_title,
            blocks: self.blocks,
            links: self.links,
            status,
        }
    }
}

/// The value of attribute `name` in a tag's attribute text.
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let bytes = attrs.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
            at += 1;
        }
        let key_start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'=' {
            at += 1;
        }
        let key = &attrs[key_start..at];
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let mut value = "";
        if at < bytes.len() && bytes[at] == b'=' {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            match bytes.get(at) {
                Some(&q) if q == b'"' || q == b'\'' => {
                    let end = attrs[at + 1..]
                        .find(q as char)
                        .map_or(bytes.len(), |e| at + 1 + e);
                    value = &attrs[at + 1..end];
                    at = end + 1;
                }
                _ => {
                    let start = at;
                    while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                        at += 1;
                    }
                    value = &attrs[start..at];
                }
            }
        }
        if key.eq_ignore_ascii_case(name) {
            return Some(value);
        }
        if key.is_empty() {
            at += 1;
        }
    }
    None
}

/// Tags whose content is not page text. Not `noscript`: scripts never
/// run here, so what a page says to readers without them is its text.
const SKIPPED: [&str; 5] = ["script", "style", "svg", "template", "iframe"];

/// Read an HTML page fetched from `url`.
pub fn read_html(html: &str, url: &str, status: u16) -> Page {
    let mut reader = Reader {
        base: String::from(url),
        blocks: Vec::new(),
        spans: Vec::new(),
        kind: BlockKind::Text,
        link: None,
        links: Vec::new(),
        title: String::new(),
        in_title: false,
        pre: 0,
        lists: Vec::new(),
        quote: 0,
        hang: 0,
        space: false,
        cells: 0,
    };
    let bytes = html.as_bytes();
    let mut at = 0;
    while at < html.len() {
        if html[at..].starts_with("<!--") {
            at = html[at + 4..]
                .find("-->")
                .map_or(html.len(), |e| at + 4 + e + 3);
            continue;
        }
        let is_tag = bytes[at] == b'<'
            && bytes
                .get(at + 1)
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'/' || *b == b'!' || *b == b'?');
        if !is_tag {
            let end = html[at + 1..].find('<').map_or(html.len(), |e| at + 1 + e);
            reader.push_text(&html[at..end]);
            at = end;
            continue;
        }
        // The tag runs to the first `>` outside quotes.
        let mut end = at + 1;
        let mut quote: Option<u8> = None;
        while end < bytes.len() {
            match (quote, bytes[end]) {
                (None, b'>') => break,
                (None, q @ (b'"' | b'\'')) => quote = Some(q),
                (Some(q), c) if c == q => quote = None,
                _ => {}
            }
            end += 1;
        }
        let inner = &html[at + 1..end.min(html.len())];
        at = (end + 1).min(html.len());
        if inner.starts_with('!') || inner.starts_with('?') {
            continue;
        }
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/');
        let name_end = inner
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        let attrs = &inner[name_end..];
        if !closing && SKIPPED.contains(&name.as_str()) && !inner.ends_with('/') {
            // Everything up to its closing tag is not text.
            let close = alloc::format!("</{name}");
            let lower = html[at..].to_ascii_lowercase();
            at = match lower.find(&close) {
                Some(e) => {
                    let after = at + e;
                    html[after..]
                        .find('>')
                        .map_or(html.len(), |g| after + g + 1)
                }
                None => html.len(),
            };
            continue;
        }
        reader.tag(&name, closing, attrs);
    }
    reader.finish(url, status)
}

/// A plain-text page: its lines, kept as they are.
pub fn read_text(text: &str, url: &str, status: u16) -> Page {
    let mut reader = Reader {
        base: String::from(url),
        blocks: Vec::new(),
        spans: Vec::new(),
        kind: BlockKind::Pre,
        link: None,
        links: Vec::new(),
        title: String::new(),
        in_title: false,
        pre: 1,
        lists: Vec::new(),
        quote: 0,
        hang: 0,
        space: false,
        cells: 0,
    };
    for line in text.lines() {
        reader.push_chars(line, true);
        reader.flush(BlockKind::Pre);
    }
    let mut page = reader.finish(url, status);
    page.title = String::from(url.rsplit('/').find(|s| !s.is_empty()).unwrap_or(url));
    page
}

// ---- Wrapping ----

/// One drawn row: runs of text, each part of a link or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: BlockKind,
    pub runs: Vec<(String, Option<usize>)>,
}

/// The page's blocks as rows at most `cols` characters wide: words wrap,
/// preformatted lines break hard.
pub fn wrap(page: &Page, cols: usize) -> Vec<Row> {
    let cols = cols.max(8);
    let mut rows = Vec::new();
    for block in &page.blocks {
        match block.kind {
            BlockKind::Blank => rows.push(Row {
                kind: BlockKind::Blank,
                runs: Vec::new(),
            }),
            BlockKind::Rule => rows.push(Row {
                kind: BlockKind::Rule,
                runs: Vec::new(),
            }),
            kind => {
                let chars: Vec<(char, Option<usize>)> = block
                    .spans
                    .iter()
                    .flat_map(|s| s.text.chars().map(move |c| (c, s.link)))
                    .collect();
                let indent = (block.indent as usize).min(cols / 2);
                let hang = (block.hang as usize).min(cols / 4);
                let mut start = 0;
                loop {
                    // The first row starts at the indent, the rest also hang.
                    let lead = if start == 0 { indent } else { indent + hang };
                    let cols = cols - lead;
                    let remaining = chars.len() - start;
                    let end = if remaining <= cols {
                        chars.len()
                    } else if kind == BlockKind::Pre {
                        start + cols
                    } else {
                        // After the last space that fits, or hard.
                        (start + 1..=start + cols)
                            .rev()
                            .find(|i| chars[*i - 1].0 == ' ')
                            .unwrap_or(start + cols)
                    };
                    let mut runs = runs_of(&chars[start..end]);
                    if lead > 0 {
                        runs.insert(0, (" ".repeat(lead), None));
                    }
                    rows.push(Row { kind, runs });
                    start = end;
                    // A wrapped line does not start with the space it broke at.
                    while kind != BlockKind::Pre && start < chars.len() && chars[start].0 == ' ' {
                        start += 1;
                    }
                    if start >= chars.len() {
                        break;
                    }
                }
            }
        }
    }
    rows
}

fn runs_of(chars: &[(char, Option<usize>)]) -> Vec<(String, Option<usize>)> {
    let mut runs: Vec<(String, Option<usize>)> = Vec::new();
    for &(c, link) in chars {
        match runs.last_mut() {
            Some((text, l)) if *l == link => text.push(c),
            _ => runs.push((String::from(c), link)),
        }
    }
    runs
}

// ---- The card ----

/// The start page's address: bookmarks and recent pages, made here, not
/// fetched (WEB-002).
pub const START: &str = "about:start";
/// Forward, the start page, and bookmarking the page shown.
pub const KEY_FORWARD: u8 = 0xB4;
pub const KEY_START: u8 = 0xB5;
pub const KEY_BOOKMARK: u8 = 0xB6;
/// Ctrl+F finds in the page; Ctrl+C copies the link Tab reached, or the
/// page's address.
pub const CTRL_F: u8 = 0x06;
pub const CTRL_C: u8 = 0x03;
/// Pages kept to show again at once, going back or forward.
const CACHE_MAX: usize = 8;
/// Pages the start page lists as recent.
const RECENT_MAX: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebEffect {
    None,
    Redraw,
    /// Fetch this URL, then call `loaded`.
    Fetch(String),
    Close,
    /// The bookmarks changed: keep these (every Web card shares them).
    Bookmarks(Vec<Bookmark>),
    /// Put this on the clipboard.
    Copy(String),
}

/// A page kept to come back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

/// The bookmarks as kept on disk: `url<TAB>title`, a line each.
pub fn bookmarks_text(list: &[Bookmark]) -> String {
    let mut text = String::new();
    for b in list {
        text.push_str(&b.url);
        text.push('\t');
        text.push_str(&b.title.replace(['\t', '\n'], " "));
        text.push('\n');
    }
    text
}

/// Bookmarks read back from `bookmarks_text`.
pub fn parse_bookmarks(text: &str) -> Vec<Bookmark> {
    text.lines()
        .filter_map(|line| {
            let (url, title) = line.split_once('\t').unwrap_or((line, ""));
            let url = url.trim();
            (!url.is_empty()).then(|| Bookmark {
                title: String::from(title.trim()),
                url: String::from(url),
            })
        })
        .collect()
}

/// `text` safe inside HTML.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct WebView {
    address: LineEdit,
    /// The address bar has the keys; otherwise the page does.
    pub editing: bool,
    page: Option<Page>,
    /// Being fetched now.
    loading: Option<String>,
    /// Why the last fetch showed nothing.
    message: Option<String>,
    scroll: usize,
    back: Vec<String>,
    forward: Vec<String>,
    /// Pages already read, and where each was scrolled to, so Back and
    /// Forward show them at once.
    cache: Vec<(Page, usize)>,
    /// Pages visited, newest first: `(title, url)`.
    recent: Vec<(String, String)>,
    bookmarks: Vec<Bookmark>,
    /// The link Tab has reached, by index into the page's links.
    selected: Option<usize>,
    redirects: u8,
    /// Find in page: the query, and which match is the current one.
    find: Option<(LineEdit, usize)>,
    /// The page wrapped at the last width drawn, and that width.
    rows: RefCell<Option<(usize, Vec<Row>)>>,
    /// The canvas the card was last drawn at, so a link's key can be
    /// mapped back to the link.
    last_size: core::cell::Cell<(u32, u32)>,
    /// The link under the pointer when last drawn: the footer says where
    /// it goes.
    hover_link: core::cell::Cell<Option<usize>>,
}

impl Default for WebView {
    fn default() -> Self {
        Self::new()
    }
}

impl WebView {
    pub fn new() -> Self {
        let mut view = Self {
            address: LineEdit::new(),
            editing: true,
            page: None,
            loading: None,
            message: None,
            scroll: 0,
            back: Vec::new(),
            forward: Vec::new(),
            cache: Vec::new(),
            recent: Vec::new(),
            bookmarks: Vec::new(),
            selected: None,
            redirects: 0,
            find: None,
            rows: RefCell::new(None),
            last_size: core::cell::Cell::new((0, 0)),
            hover_link: core::cell::Cell::new(None),
        };
        view.show_start();
        view
    }

    /// The page shown, if any.
    pub fn page(&self) -> Option<&Page> {
        self.page.as_ref()
    }

    fn showing_start(&self) -> bool {
        self.page.as_ref().is_some_and(|p| p.url == START)
    }

    /// The URL of the page shown, or being fetched.
    pub fn url(&self) -> Option<String> {
        self.loading
            .clone()
            .or_else(|| self.page.as_ref().map(|p| p.url.clone()))
    }

    /// The card's title: the page's, once there is one.
    pub fn title(&self) -> String {
        match &self.page {
            Some(page) if !page.title.is_empty() && page.url != START => {
                alloc::format!("Web - {}", page.title)
            }
            _ => String::from("Web"),
        }
    }

    /// The bookmarks every Web card shares, from the desk.
    pub fn set_bookmarks(&mut self, list: Vec<Bookmark>) {
        self.bookmarks = list;
        if self.showing_start() {
            let scroll = self.scroll;
            self.show_start();
            self.scroll = scroll;
        }
    }

    /// Whether the page shown is bookmarked.
    pub fn bookmarked(&self) -> bool {
        self.page
            .as_ref()
            .is_some_and(|p| self.bookmarks.iter().any(|b| b.url == p.url))
    }

    /// The start page: bookmarks, then recent pages, as links.
    fn start_page(&self) -> Page {
        let mut html = String::from("<title>Start</title><h1>Bookmarks</h1>");
        if self.bookmarks.is_empty() {
            html.push_str("<p>None yet: the Bookmark chip keeps the page you are on.</p>");
        } else {
            html.push_str("<ul>");
            for b in &self.bookmarks {
                let title = if b.title.is_empty() { &b.url } else { &b.title };
                html.push_str(&alloc::format!(
                    "<li><a href=\"{}\">{}</a></li>",
                    escape_html(&b.url),
                    escape_html(title)
                ));
            }
            html.push_str("</ul>");
        }
        html.push_str("<h1>Recent</h1>");
        if self.recent.is_empty() {
            html.push_str(
                "<p>Nothing yet. Type an address above: example.com, or an IP and a port.</p>",
            );
        } else {
            html.push_str("<ul>");
            for (title, url) in &self.recent {
                let title = if title.is_empty() { url } else { title };
                html.push_str(&alloc::format!(
                    "<li><a href=\"{}\">{}</a></li>",
                    escape_html(url),
                    escape_html(title)
                ));
            }
            html.push_str("</ul>");
        }
        html.push_str(
            "<h1>Keys</h1><p>Tab and Enter follow links; Backspace goes back; Ctrl+F finds; \
             Ctrl+C copies a link; Esc goes to the address.</p>",
        );
        read_html(&html, START, 200)
    }

    fn show_start(&mut self) {
        let page = self.start_page();
        self.show(page, 0);
        self.address.set("");
        self.editing = true;
    }

    /// Put `page` on screen at `scroll`.
    fn show(&mut self, page: Page, scroll: usize) {
        self.address.set(&page.url);
        self.page = Some(page);
        *self.rows.borrow_mut() = None;
        self.scroll = scroll;
        self.selected = None;
        self.editing = false;
        self.find = None;
    }

    /// Keep the page shown, and where it is scrolled, for Back and Forward.
    fn stash(&mut self) {
        let Some(page) = self.page.clone() else {
            return;
        };
        if page.url == START {
            return;
        }
        self.cache.retain(|(p, _)| p.url != page.url);
        self.cache.push((page, self.scroll));
        if self.cache.len() > CACHE_MAX {
            self.cache.remove(0);
        }
    }

    /// Start fetching `url` (a restored card, or a link from elsewhere).
    pub fn open(&mut self, url: &str) -> WebEffect {
        if url == START {
            self.stash();
            self.show_start();
            return WebEffect::Redraw;
        }
        let url = normalize(url);
        self.address.set(&url);
        self.loading = Some(url.clone());
        self.message = None;
        self.redirects = 0;
        WebEffect::Fetch(url)
    }

    /// Go to `url`, remembering the page shown to come back to.
    fn go(&mut self, url: String) -> WebEffect {
        if let Some(page) = &self.page {
            if page.url != url {
                self.back.push(page.url.clone());
                if self.back.len() > BACK_MAX {
                    self.back.remove(0);
                }
            }
        }
        self.forward.clear();
        self.stash();
        self.editing = false;
        self.open(&url)
    }

    /// Show `url` for Back or Forward: from the cache when it is there.
    fn revisit(&mut self, url: String) -> WebEffect {
        self.stash();
        if url == START {
            self.show_start();
            return WebEffect::Redraw;
        }
        match self.cache.iter().position(|(p, _)| p.url == url) {
            Some(at) => {
                let (page, scroll) = self.cache[at].clone();
                self.loading = None;
                self.message = None;
                self.show(page, scroll);
                WebEffect::Redraw
            }
            None => {
                self.editing = false;
                self.open(&url)
            }
        }
    }

    fn back(&mut self) -> WebEffect {
        let Some(url) = self.back.pop() else {
            return WebEffect::None;
        };
        if let Some(page) = &self.page {
            self.forward.push(page.url.clone());
        }
        self.revisit(url)
    }

    fn forward(&mut self) -> WebEffect {
        let Some(url) = self.forward.pop() else {
            return WebEffect::None;
        };
        if let Some(page) = &self.page {
            self.back.push(page.url.clone());
        }
        self.revisit(url)
    }

    /// The fetch for `asked` ended (NET-034): taken only if it is the one
    /// this card is waiting for -- a click on another link while one was
    /// loading leaves the first one's answer stale.
    pub fn loaded_for(&mut self, asked: &str, outcome: WebOutcome) -> WebEffect {
        if self.loading.as_deref() != Some(asked) {
            return WebEffect::None;
        }
        self.loaded(outcome)
    }

    /// The fetch for this card ended.
    pub fn loaded(&mut self, outcome: WebOutcome) -> WebEffect {
        if self.loading.take().is_none() {
            return WebEffect::None;
        }
        let response = match outcome {
            Ok(r) => r,
            Err(why) => {
                self.message = Some(why);
                return WebEffect::Redraw;
            }
        };
        // A redirect is followed, a few times, over plain HTTP.
        if (300..400).contains(&response.status) {
            if let Some(to) = response
                .location
                .as_deref()
                .and_then(|l| resolve_url(&response.url, l))
            {
                if to.to_ascii_lowercase().starts_with("https://") {
                    self.message = Some(alloc::format!(
                        "{} moved to {to}, and there is no TLS here yet to follow it",
                        response.url
                    ));
                    return WebEffect::Redraw;
                }
                if self.redirects >= MAX_REDIRECTS {
                    self.message = Some(String::from("Too many redirects"));
                    return WebEffect::Redraw;
                }
                self.redirects += 1;
                self.loading = Some(to.clone());
                self.address.set(&to);
                return WebEffect::Fetch(to);
            }
        }
        self.redirects = 0;
        let body = String::from_utf8_lossy(&response.body);
        let is_html = response
            .content_type
            .as_deref()
            .map(|t| t.to_ascii_lowercase().contains("html"))
            .unwrap_or_else(|| body.trim_start().starts_with('<'));
        let page = if is_html {
            read_html(&body, &response.url, response.status)
        } else {
            read_text(&body, &response.url, response.status)
        };
        self.recent.retain(|(_, u)| *u != page.url);
        self.recent
            .insert(0, (page.title.clone(), page.url.clone()));
        self.recent.truncate(RECENT_MAX);
        self.show(page, 0);
        self.stash();
        self.message = if (200..300).contains(&response.status) {
            None
        } else {
            Some(alloc::format!("{} {}", response.status, response.reason))
        };
        WebEffect::Redraw
    }

    /// The page's rows at `cols`, wrapped once per width.
    fn rows(&self, cols: usize) -> core::cell::Ref<'_, Vec<Row>> {
        let stale = self.rows.borrow().as_ref().is_none_or(|(c, _)| *c != cols);
        if stale {
            let rows = self
                .page
                .as_ref()
                .map(|p| wrap(p, cols))
                .unwrap_or_default();
            *self.rows.borrow_mut() = Some((cols, rows));
        }
        core::cell::Ref::map(self.rows.borrow(), |r| &r.as_ref().expect("wrapped").1)
    }

    fn cols(&self) -> usize {
        let (w, _) = self.last_size.get();
        (w.max(8 * GLYPH_W) / GLYPH_W) as usize
    }

    fn page_rows(h: u32) -> usize {
        (h.saturating_sub(TOP) / ROW).max(1) as usize
    }

    fn shown_rows(&self) -> usize {
        Self::page_rows(self.last_size.get().1.max(TOP + ROW))
    }

    /// The links on screen, in order, as the keys that click them answer.
    fn visible_links(&self, w: u32, h: u32) -> Vec<usize> {
        let rows = self.rows((w / GLYPH_W) as usize);
        let mut seen = Vec::new();
        for row in rows.iter().skip(self.scroll).take(Self::page_rows(h)) {
            for (_, link) in &row.runs {
                if let Some(link) = link {
                    if !seen.contains(link) {
                        seen.push(*link);
                    }
                }
            }
        }
        seen.truncate(LINK_KEYS);
        seen
    }

    /// Scroll by `rows` (the wheel).
    pub fn scroll_by(&mut self, rows: i32) {
        let total = self.rows(self.cols()).len();
        let max = total.saturating_sub(self.shown_rows());
        self.scroll = (self.scroll as i64 + rows as i64).clamp(0, max as i64) as usize;
    }

    /// Bring row `at` into view, a third of the way down.
    fn scroll_to_row(&mut self, at: usize) {
        let shown = self.shown_rows();
        if at < self.scroll || at >= self.scroll + shown {
            self.scroll = at.saturating_sub(shown / 3);
        }
    }

    fn scroll_to_link(&mut self, link: usize) {
        let at = self
            .rows(self.cols())
            .iter()
            .position(|r| r.runs.iter().any(|(_, l)| *l == Some(link)));
        if let Some(at) = at {
            self.scroll_to_row(at);
        }
    }

    /// Where the find query is on the page: `(row, column, length)`,
    /// ignoring case (WEB-002).
    fn matches(&self) -> Vec<(usize, usize, usize)> {
        let Some((query, _)) = &self.find else {
            return Vec::new();
        };
        let needle = String::from_utf8_lossy(query.text()).to_ascii_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut found = Vec::new();
        for (r, row) in self.rows(self.cols()).iter().enumerate() {
            let text: String = row.runs.iter().map(|(t, _)| t.as_str()).collect();
            let lower = text.to_ascii_lowercase();
            let mut from = 0;
            while let Some(at) = lower[from..].find(&needle) {
                let col = lower[..from + at].chars().count();
                found.push((r, col, needle.chars().count()));
                from += at + needle.len();
            }
        }
        found
    }

    /// The find bar's keys.
    fn find_key(&mut self, byte: u8) -> WebEffect {
        use crate::notepad::{ESC, KEY_DOWN, KEY_UP};
        let count = self.matches().len();
        let Some((query, current)) = self.find.as_mut() else {
            return WebEffect::None;
        };
        match byte {
            ESC => {
                self.find = None;
            }
            b'\n' | b'\r' | KEY_DOWN if count > 0 => *current = (*current + 1) % count,
            KEY_UP if count > 0 => *current = (*current + count - 1) % count,
            _ => match query.key(byte, &[]) {
                Edit::Unhandled | Edit::Nothing => return WebEffect::None,
                // A new query starts from the first match.
                _ => *current = 0,
            },
        }
        let current = self.find.as_ref().map(|(_, c)| *c);
        if let (Some(current), Some(&(row, _, _))) =
            (current, self.matches().get(current.unwrap_or(0)))
        {
            let _ = current;
            self.scroll_to_row(row);
        }
        WebEffect::Redraw
    }

    pub fn handle_byte(&mut self, byte: u8) -> WebEffect {
        use crate::notepad::{CTRL_W, ESC};
        match byte {
            CTRL_W => return WebEffect::Close,
            KEY_ADDRESS => {
                self.editing = true;
                self.find = None;
                return WebEffect::Redraw;
            }
            KEY_GO if self.editing || self.page.is_none() || self.showing_start() => {
                return self.submit();
            }
            KEY_GO | KEY_RELOAD => {
                return match self.page.as_ref().map(|p| p.url.clone()) {
                    Some(url) if url != START => self.open(&url),
                    _ => WebEffect::None,
                };
            }
            KEY_BACK => return self.back(),
            KEY_FORWARD => return self.forward(),
            KEY_START => {
                return if self.showing_start() {
                    WebEffect::None
                } else {
                    self.go(String::from(START))
                };
            }
            // Ctrl+D, as browsers have it.
            KEY_BOOKMARK | crate::notepad::CTRL_D => return self.toggle_bookmark(),
            CTRL_F if self.page.is_some() => {
                self.find = Some((LineEdit::new(), 0));
                self.editing = false;
                return WebEffect::Redraw;
            }
            CTRL_C => {
                let url = self
                    .selected
                    .and_then(|s| self.page.as_ref()?.links.get(s).cloned())
                    .or_else(|| self.page.as_ref().map(|p| p.url.clone()))
                    .filter(|u| u != START);
                return match url {
                    Some(url) => {
                        self.message = Some(alloc::format!("Copied {url}"));
                        WebEffect::Copy(url)
                    }
                    None => WebEffect::None,
                };
            }
            b if (LINK_KEY_FIRST..=LINK_KEY_FIRST + (LINK_KEYS - 1) as u8).contains(&b) => {
                let (w, h) = self.last_size.get();
                let link = self
                    .visible_links(w, h)
                    .get((b - LINK_KEY_FIRST) as usize)
                    .copied();
                return match link.and_then(|l| self.page.as_ref()?.links.get(l).cloned()) {
                    Some(url) => self.go(url),
                    None => WebEffect::None,
                };
            }
            _ => {}
        }
        if self.find.is_some() {
            return self.find_key(byte);
        }
        // On the start page, with nothing typed, Tab, the arrows and Enter
        // are the page's: they walk and open its links.
        if self.editing && self.showing_start() && self.address.is_empty() {
            use crate::notepad::{KEY_DOWN, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_UP};
            if matches!(
                byte,
                b'\t' | b'\n' | b'\r' | KEY_UP | KEY_DOWN | KEY_PAGE_UP | KEY_PAGE_DOWN
            ) {
                return self.page_key(byte);
            }
        }
        if self.editing {
            return match self.address.key(byte, &[]) {
                Edit::Submit => self.submit(),
                Edit::Unhandled if byte == ESC && !self.showing_start() && self.page.is_some() => {
                    self.editing = false;
                    if let Some(url) = self.page.as_ref().map(|p| p.url.clone()) {
                        self.address.set(&url);
                    }
                    WebEffect::Redraw
                }
                // On the start page the arrows and Tab still reach its links.
                Edit::Unhandled if self.showing_start() => self.page_key(byte),
                Edit::Unhandled => WebEffect::None,
                _ => WebEffect::Redraw,
            };
        }
        self.page_key(byte)
    }

    /// A key for the page itself.
    fn page_key(&mut self, byte: u8) -> WebEffect {
        use crate::notepad::{
            BACKSPACE, ESC, KEY_DOWN, KEY_END, KEY_HOME, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_UP,
        };
        let links = self.page.as_ref().map_or(0, |p| p.links.len());
        match byte {
            KEY_UP => self.scroll_by(-1),
            KEY_DOWN => self.scroll_by(1),
            KEY_PAGE_UP => self.scroll_by(-10),
            KEY_PAGE_DOWN => self.scroll_by(10),
            KEY_HOME => self.scroll = 0,
            KEY_END => self.scroll_by(i32::MAX / 2),
            b'\t' if links > 0 => {
                let next = self.selected.map_or(0, |s| (s + 1) % links);
                self.selected = Some(next);
                self.scroll_to_link(next);
            }
            b'\n' | b'\r' => {
                let url = self
                    .selected
                    .and_then(|s| self.page.as_ref()?.links.get(s).cloned());
                return match url {
                    Some(url) => self.go(url),
                    None => WebEffect::None,
                };
            }
            BACKSPACE => return self.back(),
            ESC => {
                self.editing = true;
                self.address.set("");
            }
            // Typing on the page starts a new address.
            0x20..=0x7E => {
                self.editing = true;
                self.address.set("");
                let _ = self.address.key(byte, &[]);
            }
            _ => return WebEffect::None,
        }
        WebEffect::Redraw
    }

    fn toggle_bookmark(&mut self) -> WebEffect {
        let Some(page) = self.page.as_ref().filter(|p| p.url != START) else {
            return WebEffect::None;
        };
        let (title, url) = (page.title.clone(), page.url.clone());
        if self.bookmarks.iter().any(|b| b.url == url) {
            self.bookmarks.retain(|b| b.url != url);
            self.message = Some(String::from("Bookmark removed"));
        } else {
            self.bookmarks.push(Bookmark { title, url });
            self.message = Some(String::from("Bookmarked: it is on the start page"));
        }
        WebEffect::Bookmarks(self.bookmarks.clone())
    }

    fn submit(&mut self) -> WebEffect {
        let typed = String::from_utf8_lossy(self.address.text()).into_owned();
        if typed.trim().is_empty() {
            return WebEffect::None;
        }
        let url = if typed.trim() == START {
            String::from(START)
        } else {
            normalize(&typed)
        };
        self.go(url)
    }

    pub fn footer(&self) -> String {
        if let Some(url) = &self.loading {
            return alloc::format!("Loading {url} ...");
        }
        if let Some((query, current)) = &self.find {
            let count = self.matches().len();
            let where_ = match count {
                0 if query.is_empty() => String::new(),
                0 => String::from("   no matches"),
                n => alloc::format!("   {} of {n}", current + 1),
            };
            return alloc::format!(
                "Find: {}_{where_}   Enter or Down next, Up back, Esc closes",
                query.shown()
            );
        }
        if let Some(link) = self.hover_link.get() {
            if let Some(url) = self.page.as_ref().and_then(|p| p.links.get(link)) {
                return url.clone();
            }
        }
        if let Some(message) = &self.message {
            return message.clone();
        }
        match &self.page {
            None => String::from("Type an address and press Enter   http:// only for now"),
            Some(page) if page.url == START => {
                String::from("Type an address, or pick a page   http:// only for now")
            }
            Some(page) => alloc::format!(
                "{} link{}   Tab and Enter follow them   Backspace goes back   Ctrl+F finds",
                page.links.len(),
                if page.links.len() == 1 { "" } else { "s" }
            ),
        }
    }

    pub fn ui(&self, w: u32, h: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        self.last_size.set((w, h));
        let p = palette;
        let mut ui = Ui::new(palette, hover);
        // The bar: Back, Forward, the address, Go.
        let quiet = |empty: bool| {
            if empty {
                ButtonKind::Quiet
            } else {
                ButtonKind::Plain
            }
        };
        ui.button(
            rect(0, 0, 32, BAR_H),
            "<",
            KEY_BACK,
            quiet(self.back.is_empty()),
        );
        ui.button(
            rect(38, 0, 32, BAR_H),
            ">",
            KEY_FORWARD,
            quiet(self.forward.is_empty()),
        );
        let field_x = 78;
        let field = rect(field_x, 0, w.saturating_sub(field_x as u32 + 84), BAR_H);
        ui.fill(field, p.raised, 8);
        ui.outline(
            field,
            if self.editing { p.accent } else { p.hairline },
            8,
            if self.editing { 2 } else { 1 },
        );
        let text = self.address.shown();
        let room = (field.width.saturating_sub(24) / GLYPH_W) as usize;
        let count = text.chars().count();
        let shown: String = if count > room {
            text.chars().skip(count - room).collect()
        } else {
            text.clone()
        };
        let text_x = field_x + 12;
        let (shown, ink) = if text.is_empty() && self.editing {
            (String::from("Type an address"), p.muted)
        } else {
            (shown, p.text)
        };
        ui.text(text_x, (BAR_H as i32 - 16) / 2, &shown, ink, 1);
        if self.editing {
            let caret = if text.is_empty() {
                text_x
            } else {
                text_x + (count.min(room) as i32) * GLYPH_W as i32
            };
            ui.fill(rect(caret, 7, 2, BAR_H - 14), p.accent, 0);
        }
        ui.hit_area(field, KEY_ADDRESS);
        let go = if self.editing || self.page.is_none() || self.showing_start() {
            "Go"
        } else {
            "Reload"
        };
        ui.button(
            rect(w.saturating_sub(76) as i32, 0, 76, BAR_H),
            go,
            KEY_GO,
            ButtonKind::Primary,
        );
        ui.line(
            0,
            BAR_H as i32 + 5,
            w as i32,
            BAR_H as i32 + 5,
            p.hairline,
            1,
        );

        let Some(_) = &self.page else {
            let say = self
                .message
                .clone()
                .or_else(|| {
                    self.loading
                        .as_ref()
                        .map(|u| alloc::format!("Loading {u} ..."))
                })
                .unwrap_or_default();
            ui.text(4, TOP as i32 + 8, &say, p.muted, 1);
            self.hover_link.set(None);
            return ui;
        };
        let cols = (w / GLYPH_W) as usize;
        let visible = self.visible_links(w, h);
        let matches = self.matches();
        let current = self.find.as_ref().map(|(_, c)| *c);
        let rows = self.rows(cols);
        let mut hovered = None;
        let shown_rows = Self::page_rows(h);
        // Find's matches, behind the text: the current one in the accent.
        for (n, &(row, col, len)) in matches.iter().enumerate() {
            if row < self.scroll || row >= self.scroll + shown_rows {
                continue;
            }
            let y = TOP as i32 + (row - self.scroll) as i32 * ROW as i32;
            let area = rect(
                col as i32 * GLYPH_W as i32,
                y,
                len as u32 * GLYPH_W,
                ROW - 2,
            );
            if Some(n) == current {
                ui.fill(area, p.accent, 3);
            } else {
                ui.fill(area, p.raised, 3);
                ui.outline(area, p.hairline, 3, 1);
            }
        }
        for (n, row) in rows.iter().skip(self.scroll).take(shown_rows).enumerate() {
            let y = TOP as i32 + n as i32 * ROW as i32;
            if row.kind == BlockKind::Rule {
                ui.line(
                    0,
                    y + ROW as i32 / 2,
                    w as i32,
                    y + ROW as i32 / 2,
                    p.hairline,
                    1,
                );
                continue;
            }
            let mut x = 0i32;
            for (text, link) in &row.runs {
                let width = (text.chars().count() as u32 * GLYPH_W) as i32;
                let ink = p.text;
                if let Some(link) = link {
                    let area = rect(x, y, width.max(0) as u32, ROW);
                    if self.selected == Some(*link) {
                        ui.fill(area, p.raised, 4);
                    }
                    if ui.hovered(&area) {
                        hovered = Some(*link);
                    }
                    ui.text(x, y + 2, text, p.accent, 1);
                    ui.line(x, y + 17, x + width, y + 17, p.accent, 1);
                    if let Some(n) = visible.iter().position(|v| v == link) {
                        ui.hit_area(area, LINK_KEY_FIRST + n as u8);
                    }
                } else {
                    ui.text(x, y + 2, text, ink, 1);
                    if row.kind == BlockKind::Heading {
                        // Headings are bold: the text again, a pixel over.
                        ui.text(x + 1, y + 2, text, ink, 1);
                    }
                }
                x += width;
            }
            // The current match's own letters, again, on the accent.
            if let Some(&(r, col, len)) = current.and_then(|c| matches.get(c)) {
                if r == self.scroll + n {
                    let text: String = row
                        .runs
                        .iter()
                        .flat_map(|(t, _)| t.chars())
                        .skip(col)
                        .take(len)
                        .collect();
                    ui.text(col as i32 * GLYPH_W as i32, y + 2, &text, p.on_accent, 1);
                }
            }
        }
        self.hover_link.set(hovered);
        ui
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<!doctype html>
<html><head><title>The  Panda &amp; Co</title>
<style>body { color: red }</style>
<script>document.write("<p>nope</p>")</script></head>
<body>
<!-- a comment <p>hidden</p> -->
<h1>Welcome</h1>
<p>Hello,   <b>world</b>&nbsp;&mdash; see <a href="/about.html">about us</a>
and <a href='news/today.html?x=1&amp;y=2'>today</a>.</p>
<ul><li>one</li><li>two <a href="mailto:x@y">mail</a></li></ul>
<pre>  a  b
c</pre>
<hr>
<img src="p.png" alt="a panda"> &#65;&#x42; caf&eacute; &unknown;
</body></html>"#;

    fn text_of(block: &Block) -> String {
        block.spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn a_page_reads_as_headings_paragraphs_lists_and_links() {
        let page = read_html(PAGE, "http://example.com/dir/index.html", 200);
        assert_eq!(page.title, "The Panda & Co");
        let texts: Vec<(BlockKind, String)> =
            page.blocks.iter().map(|b| (b.kind, text_of(b))).collect();
        assert_eq!(texts[0], (BlockKind::Heading, "Welcome".into()));
        assert_eq!(texts[1].0, BlockKind::Blank);
        assert_eq!(
            texts[2],
            (
                BlockKind::Text,
                "Hello, world -- see about us and today.".into()
            )
        );
        assert!(
            texts.contains(&(BlockKind::Text, "- one".into())),
            "{texts:?}"
        );
        assert!(
            texts.contains(&(BlockKind::Text, "- two mail".into())),
            "{texts:?}"
        );
        assert!(
            texts.contains(&(BlockKind::Pre, "  a  b".into())),
            "{texts:?}"
        );
        assert!(texts.contains(&(BlockKind::Pre, "c".into())), "{texts:?}");
        assert!(texts.iter().any(|(k, _)| *k == BlockKind::Rule));
        let last = &texts.last().unwrap().1;
        assert!(last.starts_with("[a panda] AB cafe &unknown;"), "{last}");
        assert!(!texts
            .iter()
            .any(|(_, t)| t.contains("nope") || t.contains("hidden")));
        assert_eq!(
            page.links,
            [
                "http://example.com/about.html",
                "http://example.com/dir/news/today.html?x=1&y=2",
            ]
        );
        // The mailto link is text, not a link.
        let links_in_list: usize = page
            .blocks
            .iter()
            .flat_map(|b| &b.spans)
            .filter(|s| s.link.is_some())
            .count();
        assert_eq!(links_in_list, 2);
    }

    #[test]
    fn links_resolve_against_the_page() {
        let base = "http://host:8080/a/b/c.html?q=1";
        assert_eq!(
            resolve_url(base, "d.html").as_deref(),
            Some("http://host:8080/a/b/d.html")
        );
        assert_eq!(
            resolve_url(base, "../e").as_deref(),
            Some("http://host:8080/a/e")
        );
        assert_eq!(
            resolve_url(base, "./").as_deref(),
            Some("http://host:8080/a/b/")
        );
        assert_eq!(
            resolve_url(base, "/root").as_deref(),
            Some("http://host:8080/root")
        );
        assert_eq!(
            resolve_url(base, "//other/x").as_deref(),
            Some("http://other/x")
        );
        assert_eq!(
            resolve_url(base, "?z=2").as_deref(),
            Some("http://host:8080/a/b/c.html?z=2")
        );
        assert_eq!(
            resolve_url(base, "HTTPS://x.org").as_deref(),
            Some("HTTPS://x.org")
        );
        assert_eq!(resolve_url(base, "#top"), None);
        assert_eq!(resolve_url(base, "javascript:void(0)"), None);
        assert_eq!(resolve_url("http://h", "x").as_deref(), Some("http://h/x"));
        assert_eq!(normalize("example.com"), "http://example.com");
    }

    #[test]
    fn rows_wrap_words_and_break_preformatted_lines_hard() {
        let page = read_html(
            "<p>alpha beta <a href='/g'>gamma delta</a> epsilon</p><pre>0123456789abcdef</pre>",
            "http://h/",
            200,
        );
        let rows = wrap(&page, 12);
        let texts: Vec<String> = rows
            .iter()
            .map(|r| r.runs.iter().map(|(t, _)| t.as_str()).collect())
            .collect();
        assert_eq!(texts[0], "alpha beta ");
        assert_eq!(texts[1], "gamma delta ");
        assert_eq!(texts[2], "epsilon");
        // The link keeps its runs across the wrap.
        assert_eq!(rows[1].runs[0], ("gamma delta".into(), Some(0)));
        assert!(texts.contains(&"0123456789ab".to_string()), "{texts:?}");
        assert!(texts.contains(&"cdef".to_string()));
    }

    fn response(url: &str, status: u16, body: &str) -> WebOutcome {
        Ok(WebResponse {
            url: url.into(),
            status,
            reason: "OK".into(),
            location: None,
            content_type: Some("text/html".into()),
            body: body.as_bytes().to_vec(),
        })
    }

    #[test]
    fn the_card_goes_to_an_address_follows_links_and_comes_back() {
        let mut view = WebView::new();
        for b in b"10.0.2.2:18080/" {
            view.handle_byte(*b);
        }
        assert_eq!(
            view.handle_byte(b'\n'),
            WebEffect::Fetch("http://10.0.2.2:18080/".into())
        );
        assert!(view.footer().starts_with("Loading"));
        view.loaded(response(
            "http://10.0.2.2:18080/",
            200,
            "<title>Home</title><p>Go <a href='next.html'>next</a> or <a href='/x'>x</a></p>",
        ));
        assert_eq!(view.title(), "Web - Home");
        assert!(!view.editing);
        // Tab reaches the first link; Enter follows it.
        view.handle_byte(b'\t');
        assert_eq!(
            view.handle_byte(b'\n'),
            WebEffect::Fetch("http://10.0.2.2:18080/next.html".into())
        );
        view.loaded(response(
            "http://10.0.2.2:18080/next.html",
            200,
            "<p>Second</p>",
        ));
        // Back: at once, from what was read (WEB-002), no fetch.
        assert_eq!(
            view.handle_byte(crate::notepad::BACKSPACE),
            WebEffect::Redraw
        );
        assert_eq!(view.url().as_deref(), Some("http://10.0.2.2:18080/"));
        // Forward, the same way.
        assert_eq!(view.handle_byte(KEY_FORWARD), WebEffect::Redraw);
        assert_eq!(
            view.url().as_deref(),
            Some("http://10.0.2.2:18080/next.html")
        );
        view.handle_byte(KEY_BACK);
        // A click on a link: the drawn card's hit areas.
        let palette = Palette::from_theme(&services_gui_host::Theme::DEFAULT);
        let ui = view.ui(640, 400, palette, None);
        let x = 3 * GLYPH_W as i32 + 4;
        let key = ui
            .hit(x, TOP as i32 + 5)
            .expect("the link is under the pointer");
        assert_eq!(key, LINK_KEY_FIRST);
        assert_eq!(
            view.handle_byte(key),
            WebEffect::Fetch("http://10.0.2.2:18080/next.html".into())
        );
    }

    #[test]
    fn redirects_are_followed_over_http_and_https_is_said_plainly() {
        let mut view = WebView::new();
        view.open("http://a/");
        let moved = Ok(WebResponse {
            url: "http://a/".into(),
            status: 301,
            reason: "Moved".into(),
            location: Some("/new".into()),
            content_type: None,
            body: Vec::new(),
        });
        assert_eq!(view.loaded(moved), WebEffect::Fetch("http://a/new".into()));
        let secure = Ok(WebResponse {
            url: "http://a/new".into(),
            status: 302,
            reason: "Found".into(),
            location: Some("https://a/new".into()),
            content_type: None,
            body: Vec::new(),
        });
        assert_eq!(view.loaded(secure), WebEffect::Redraw);
        assert!(view.footer().contains("no TLS"), "{}", view.footer());
        // A failure says why.
        view.open("http://nowhere/");
        view.loaded(Err("fetch: nowhere: no such name".into()));
        assert!(view.footer().contains("no such name"));
    }

    #[test]
    fn plain_text_is_kept_line_for_line() {
        let mut view = WebView::new();
        view.open("http://h/notes.txt");
        view.loaded(Ok(WebResponse {
            url: "http://h/notes.txt".into(),
            status: 200,
            reason: "OK".into(),
            location: None,
            content_type: Some("text/plain".into()),
            body: b"line one\n  indented <b>not bold</b>\n".to_vec(),
        }));
        let page = view.page().unwrap();
        assert_eq!(page.title, "notes.txt");
        assert_eq!(text_of(&page.blocks[1]), "  indented <b>not bold</b>");
    }

    #[test]
    fn lists_are_numbered_and_nested_lists_and_quotes_indent() {
        let page = read_html(
            "<ol start=3><li>three</li><li>four and a long tail that wraps<ul><li>inner</li></ul></li></ol><blockquote>quoted</blockquote>",
            "http://h/",
            200,
        );
        let rows = wrap(&page, 20);
        let texts: Vec<String> = rows
            .iter()
            .map(|r| r.runs.iter().map(|(t, _)| t.as_str()).collect())
            .collect();
        assert_eq!(texts[0], "3. three");
        assert_eq!(texts[1], "4. four and a long ");
        // A wrapped item hangs under its text, not its number.
        assert_eq!(texts[2], "   tail that wraps");
        assert_eq!(texts[3], "   - inner", "{texts:?}");
        assert!(texts.contains(&"  quoted".to_string()), "{texts:?}");
    }

    #[test]
    fn the_start_page_lists_bookmarks_and_recent_pages() {
        let mut view = WebView::new();
        assert_eq!(view.url().as_deref(), Some(START));
        assert_eq!(view.title(), "Web");
        assert!(view.editing, "ready for an address");
        view.set_bookmarks(alloc::vec![Bookmark {
            title: "Panda".into(),
            url: "http://panda.test/".into()
        }]);
        let page = view.page().unwrap();
        assert_eq!(page.links, ["http://panda.test/"]);
        // Down and Tab reach it even while the address has the keys.
        view.handle_byte(b'\t');
        assert_eq!(
            view.handle_byte(b'\n'),
            WebEffect::Fetch("http://panda.test/".into()),
            "Enter follows the reached link when the address is empty"
        );
    }

    #[test]
    fn a_page_is_bookmarked_and_unbookmarked_and_the_list_survives_a_round_trip() {
        let mut view = WebView::new();
        view.open("http://a/");
        view.loaded(response("http://a/", 200, "<title>A page</title><p>x</p>"));
        let WebEffect::Bookmarks(list) = view.handle_byte(KEY_BOOKMARK) else {
            panic!("a bookmark list to keep");
        };
        assert_eq!(
            list,
            [Bookmark {
                title: "A page".into(),
                url: "http://a/".into()
            }]
        );
        assert!(view.bookmarked());
        assert_eq!(parse_bookmarks(&bookmarks_text(&list)), list);
        let WebEffect::Bookmarks(list) = view.handle_byte(KEY_BOOKMARK) else {
            panic!();
        };
        assert!(list.is_empty());
        // The start page shows recent pages.
        view.handle_byte(KEY_START);
        assert!(view
            .page()
            .unwrap()
            .links
            .contains(&"http://a/".to_string()));
    }

    #[test]
    fn find_counts_matches_moves_between_them_and_scrolls() {
        let mut view = WebView::new();
        view.open("http://a/");
        let mut body = String::new();
        for i in 0..60 {
            body.push_str(&alloc::format!("<p>line {i}</p>"));
        }
        body.push_str("<p>the Needle here</p><p>and a needle there</p>");
        view.loaded(response("http://a/", 200, &body));
        let palette = Palette::from_theme(&services_gui_host::Theme::DEFAULT);
        let _ = view.ui(640, 400, palette, None);
        view.handle_byte(CTRL_F);
        for b in b"needle" {
            view.handle_byte(*b);
        }
        assert!(view.footer().contains("1 of 2"), "{}", view.footer());
        assert!(view.scroll > 0, "scrolled to the first match");
        view.handle_byte(b'\n');
        assert!(view.footer().contains("2 of 2"));
        view.handle_byte(crate::notepad::KEY_UP);
        assert!(view.footer().contains("1 of 2"));
        view.handle_byte(crate::notepad::ESC);
        assert!(!view.footer().starts_with("Find"));
    }

    #[test]
    fn a_hovered_link_says_where_it_goes_and_ctrl_c_copies_links() {
        let mut view = WebView::new();
        view.open("http://a/");
        view.loaded(response("http://a/", 200, "<p>Go <a href='/b'>b</a></p>"));
        let palette = Palette::from_theme(&services_gui_host::Theme::DEFAULT);
        let over = (3 * GLYPH_W as i32 + 2, TOP as i32 + 5);
        let _ = view.ui(640, 400, palette, Some(over));
        assert_eq!(view.footer(), "http://a/b");
        let _ = view.ui(640, 400, palette, None);
        assert_ne!(view.footer(), "http://a/b");
        assert_eq!(
            view.handle_byte(CTRL_C),
            WebEffect::Copy("http://a/".into())
        );
        view.handle_byte(b'\t');
        assert_eq!(
            view.handle_byte(CTRL_C),
            WebEffect::Copy("http://a/b".into())
        );
    }

    #[test]
    fn an_answer_for_a_page_no_longer_wanted_is_dropped() {
        let mut view = WebView::new();
        view.open("http://a/");
        // Another link before the first answered.
        view.open("http://b/");
        assert_eq!(
            view.loaded_for("http://a/", response("http://a/", 200, "<p>stale</p>")),
            WebEffect::None
        );
        assert_eq!(view.url().as_deref(), Some("http://b/"));
        assert_eq!(
            view.loaded_for("http://b/", response("http://b/", 200, "<p>fresh</p>")),
            WebEffect::Redraw
        );
        assert_eq!(view.page().unwrap().url, "http://b/");
    }
}
