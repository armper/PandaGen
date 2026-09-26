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
    lists: usize,
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
            self.blocks.push(Block { kind, spans });
        }
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
            });
        }
    }

    fn tag(&mut self, name: &str, closing: bool, attrs: &str) {
        match name {
            "title" => self.in_title = !closing,
            "br" => self.flush(self.kind),
            "p" | "ul" | "ol" | "dl" | "table" | "blockquote" | "figure" | "form" => {
                if name == "ul" || name == "ol" {
                    if closing {
                        self.lists = self.lists.saturating_sub(1);
                    } else {
                        self.lists += 1;
                    }
                }
                self.paragraph();
            }
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
                    let indent = "  ".repeat(self.lists.saturating_sub(1));
                    self.spans.push(Span {
                        text: alloc::format!("{indent}- "),
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
        lists: 0,
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
        lists: 0,
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
                let mut start = 0;
                loop {
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
                    rows.push(Row {
                        kind,
                        runs: runs_of(&chars[start..end]),
                    });
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebEffect {
    None,
    Redraw,
    /// Fetch this URL, then call `loaded`.
    Fetch(String),
    Close,
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
    /// The link Tab has reached, by index into the page's links.
    selected: Option<usize>,
    redirects: u8,
    /// The page wrapped at the last width drawn, and that width.
    rows: RefCell<Option<(usize, Vec<Row>)>>,
    /// The canvas the card was last drawn at, so a link's key can be
    /// mapped back to the link.
    last_size: core::cell::Cell<(u32, u32)>,
}

impl Default for WebView {
    fn default() -> Self {
        Self::new()
    }
}

impl WebView {
    pub fn new() -> Self {
        Self {
            address: LineEdit::new(),
            editing: true,
            page: None,
            loading: None,
            message: None,
            scroll: 0,
            back: Vec::new(),
            selected: None,
            redirects: 0,
            rows: RefCell::new(None),
            last_size: core::cell::Cell::new((0, 0)),
        }
    }

    /// The page shown, if any.
    pub fn page(&self) -> Option<&Page> {
        self.page.as_ref()
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
            Some(page) if !page.title.is_empty() => alloc::format!("Web - {}", page.title),
            _ => String::from("Web"),
        }
    }

    /// Start fetching `url` (a restored card, or a link from elsewhere).
    pub fn open(&mut self, url: &str) -> WebEffect {
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
        self.editing = false;
        self.open(&url)
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
                if to.starts_with("https://") {
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
        self.address.set(&page.url);
        self.page = Some(page);
        *self.rows.borrow_mut() = None;
        self.scroll = 0;
        self.selected = None;
        self.editing = false;
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

    fn page_rows(h: u32) -> usize {
        (h.saturating_sub(TOP) / ROW).max(1) as usize
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
        let (w, h) = self.last_size.get();
        let total = self.rows((w.max(8 * GLYPH_W) / GLYPH_W) as usize).len();
        let max = total.saturating_sub(Self::page_rows(h.max(TOP + ROW)));
        self.scroll = (self.scroll as i64 + rows as i64).clamp(0, max as i64) as usize;
    }

    fn scroll_to_link(&mut self, link: usize) {
        let (w, h) = self.last_size.get();
        let rows = self.rows((w.max(8 * GLYPH_W) / GLYPH_W) as usize);
        if let Some(at) = rows
            .iter()
            .position(|r| r.runs.iter().any(|(_, l)| *l == Some(link)))
        {
            let shown = Self::page_rows(h.max(TOP + ROW));
            drop(rows);
            if at < self.scroll || at >= self.scroll + shown {
                self.scroll = at.saturating_sub(shown / 3);
            }
        }
    }

    pub fn handle_byte(&mut self, byte: u8) -> WebEffect {
        use crate::notepad::{
            BACKSPACE, CTRL_W, ESC, KEY_DOWN, KEY_END, KEY_HOME, KEY_PAGE_DOWN, KEY_PAGE_UP, KEY_UP,
        };
        match byte {
            CTRL_W => return WebEffect::Close,
            KEY_ADDRESS => {
                self.editing = true;
                return WebEffect::Redraw;
            }
            KEY_GO if self.editing || self.page.is_none() => {
                return self.submit();
            }
            KEY_GO | KEY_RELOAD => {
                return match self.page.as_ref().map(|p| p.url.clone()) {
                    Some(url) => self.open(&url),
                    None => WebEffect::None,
                };
            }
            KEY_BACK => return self.back(),
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
        if self.editing {
            return match self.address.key(byte, &[]) {
                Edit::Submit => self.submit(),
                Edit::Unhandled if byte == ESC && self.page.is_some() => {
                    self.editing = false;
                    if let Some(url) = self.page.as_ref().map(|p| p.url.clone()) {
                        self.address.set(&url);
                    }
                    WebEffect::Redraw
                }
                Edit::Unhandled => WebEffect::None,
                _ => WebEffect::Redraw,
            };
        }
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

    fn submit(&mut self) -> WebEffect {
        let typed = String::from_utf8_lossy(self.address.text()).into_owned();
        if typed.trim().is_empty() {
            return WebEffect::None;
        }
        self.go(normalize(&typed))
    }

    fn back(&mut self) -> WebEffect {
        match self.back.pop() {
            Some(url) => {
                self.editing = false;
                self.open(&url)
            }
            None => WebEffect::None,
        }
    }

    pub fn footer(&self) -> String {
        if let Some(url) = &self.loading {
            return alloc::format!("Loading {url} ...");
        }
        if let Some(message) = &self.message {
            return message.clone();
        }
        match &self.page {
            None => String::from("Type an address and press Enter   http:// only for now"),
            Some(page) => alloc::format!(
                "{} link{}   Tab and Enter follow them   Backspace goes back   Esc: address",
                page.links.len(),
                if page.links.len() == 1 { "" } else { "s" }
            ),
        }
    }

    pub fn ui(&self, w: u32, h: u32, palette: Palette, hover: Option<(i32, i32)>) -> Ui {
        self.last_size.set((w, h));
        let p = palette;
        let mut ui = Ui::new(palette, hover);
        // The address bar: Back, the field, Go.
        let back_kind = if self.back.is_empty() {
            ButtonKind::Quiet
        } else {
            ButtonKind::Plain
        };
        ui.button(rect(0, 0, 36, BAR_H), "<", KEY_BACK, back_kind);
        let field = rect(44, 0, w.saturating_sub(44 + 84), BAR_H);
        ui.fill(field, p.raised, 8);
        ui.outline(
            field,
            if self.editing { p.accent } else { p.hairline },
            8,
            if self.editing { 2 } else { 1 },
        );
        let text = self.address.shown();
        let room = (field.width.saturating_sub(24) / GLYPH_W) as usize;
        let mut shown: String = if text.chars().count() > room {
            text.chars().skip(text.chars().count() - room).collect()
        } else {
            text.clone()
        };
        let ink = if text.is_empty() { p.muted } else { p.text };
        if text.is_empty() && self.editing {
            shown = String::from("Type an address");
        }
        ui.text(56, (BAR_H as i32 - 16) / 2, &shown, ink, 1);
        if self.editing {
            let caret = 56 + (text.chars().count().min(room) as i32) * GLYPH_W as i32;
            let caret = if text.is_empty() { 56 } else { caret };
            ui.fill(rect(caret, 7, 2, BAR_H - 14), p.accent, 0);
        }
        ui.hit_area(field, KEY_ADDRESS);
        let go = if self.editing || self.page.is_none() {
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

        // The page.
        let Some(page) = &self.page else {
            let say = self
                .message
                .clone()
                .or_else(|| {
                    self.loading
                        .as_ref()
                        .map(|u| alloc::format!("Loading {u} ..."))
                })
                .unwrap_or_else(|| String::from("A page from the network, as text"));
            ui.text(4, TOP as i32 + 8, &say, p.muted, 1);
            return ui;
        };
        let cols = (w / GLYPH_W) as usize;
        let visible = self.visible_links(w, h);
        let rows = self.rows(cols);
        for (n, row) in rows
            .iter()
            .skip(self.scroll)
            .take(Self::page_rows(h))
            .enumerate()
        {
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
                let ink = match (row.kind, link) {
                    (_, Some(_)) => p.accent,
                    (BlockKind::Heading, None) => p.text,
                    (BlockKind::Pre, None) => p.text,
                    _ => p.text,
                };
                if let Some(link) = link {
                    let area = rect(x, y, width.max(0) as u32, ROW);
                    if self.selected == Some(*link) {
                        ui.fill(area, p.raised, 4);
                    }
                    ui.text(x, y + 2, text, ink, 1);
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
        }
        let _ = page;
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
        // Back.
        assert_eq!(
            view.handle_byte(crate::notepad::BACKSPACE),
            WebEffect::Fetch("http://10.0.2.2:18080/".into())
        );
        view.loaded(response(
            "http://10.0.2.2:18080/",
            200,
            "<p>Go <a href='next.html'>next</a></p>",
        ));
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
}
