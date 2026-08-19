//! Markup, and the single door data is allowed through.
//!
//! # Why this is not a template engine
//!
//! The catalog contains `ai_desc` prose and, since remote IFR ingestion,
//! **repository ids and interface names that came off a foreign wire**. A
//! peer's Interface Repository is an untrusted describer (`docs/PLAN.md` §9.0,
//! "tool poisoning via remote metadata"), and the page built from it is read by
//! the one person deciding what an agent may reach. A `<script>` in an ingested
//! name rendering as markup would be the console attacking its own operator —
//! the console would become the delivery vehicle for exactly the input the
//! registry marks as untrusted.
//!
//! So escaping here is not a formatting concern, and a "remember to escape"
//! rule is not good enough. [`Markup`] is a fragment that is **already safe**,
//! and it has exactly one constructor that accepts data: [`Markup::text`],
//! which escapes. Tag names and class names are `&'static str`, checked to be
//! plain names, so no value can reach an attribute position or a tag position
//! at all. There is no `Markup::raw`. Getting an unescaped byte into a page
//! therefore requires editing this file, which is the property the escaping
//! test asserts and the reason the test is short.
//!
//! # Self-contained
//!
//! [`page`] emits one file with its stylesheet inline and no `src`, `href`,
//! `@import` or `<script>` anywhere. It renders identically with the network
//! off, which is the only way an operator can trust that what they are reading
//! is what was written.

use std::fmt;

/// Escapes `text` so it is inert in an HTML text node or a quoted attribute.
///
/// The five characters HTML gives meaning to, `&` first so the replacements it
/// introduces are not re-escaped. Nothing else is touched: mangling a
/// repository id would make the page lie about what is in the catalog, and the
/// page's job is to show what is there.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// A fragment of HTML that is already safe to place in a document.
///
/// The invariant: every byte inside came either from a literal in this crate or
/// from [`escape`]. See the module docs for why that is enforced by the type
/// rather than by discipline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Markup(String);

impl Markup {
    /// Nothing at all.
    pub fn empty() -> Self {
        Self(String::new())
    }

    /// Data, escaped. **The only way a value enters a page.**
    pub fn text(value: &str) -> Self {
        Self(escape(value))
    }

    /// Several fragments, concatenated in order.
    pub fn seq<I: IntoIterator<Item = Markup>>(parts: I) -> Self {
        let mut out = String::new();
        for part in parts {
            out.push_str(&part.0);
        }
        Self(out)
    }

    /// An element wrapping `inner`.
    ///
    /// `tag` and `class` are `&'static str` and are checked to be plain names,
    /// so neither can carry a quote, an angle bracket or an event handler.
    /// A caller that manages to fail the check has written a bad literal, which
    /// is a bug in this crate and is meant to be loud.
    pub fn element(tag: &'static str, class: &'static str, inner: Markup) -> Self {
        assert!(is_name(tag), "tag names are literals in this crate: {tag:?}");
        assert!(class.is_empty() || is_class(class), "class names are literals: {class:?}");
        let mut out = String::with_capacity(inner.0.len() + 32);
        out.push('<');
        out.push_str(tag);
        if !class.is_empty() {
            out.push_str(" class=\"");
            out.push_str(class);
            out.push('"');
        }
        out.push('>');
        out.push_str(&inner.0);
        out.push_str("</");
        out.push_str(tag);
        out.push('>');
        Self(out)
    }

    /// An element wrapping one escaped value — the common case.
    pub fn labelled(tag: &'static str, class: &'static str, value: &str) -> Self {
        Self::element(tag, class, Self::text(value))
    }

    /// Appends `part`.
    pub fn push(&mut self, part: Markup) {
        self.0.push_str(&part.0);
    }

    /// Whether this fragment renders nothing.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The markup, for writing out.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Markup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_name(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn is_class(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b' ')
}

/// The stylesheet, inline. No font is fetched, no colour is loaded, nothing is
/// requested: a page that phoned home would be reporting the operator's catalog
/// to somebody.
const STYLE: &str = "\
:root{--bg:#fbfbfa;--fg:#1a1a19;--dim:#6b6b66;--rule:#dcdcd6;--card:#fff;\
--warn:#8a3b00;--warn-bg:#fdf1e6;--stop:#8a1500;--stop-bg:#fdecea;\
--ok:#14532d;--ok-bg:#eaf5ee;--cool:#1e3a5f;--cool-bg:#e9eff6}\
@media(prefers-color-scheme:dark){:root{--bg:#16171a;--fg:#e6e6e3;--dim:#9a9a94;\
--rule:#33343a;--card:#1e1f24;--warn:#ffc38a;--warn-bg:#3a2410;--stop:#ff9d8f;\
--stop-bg:#3d1713;--ok:#9edeb3;--ok-bg:#15301f;--cool:#a8c8ec;--cool-bg:#17273a}}\
*{box-sizing:border-box}\
body{margin:0;padding:2rem 1.25rem 4rem;background:var(--bg);color:var(--fg);\
font:15px/1.55 ui-sans-serif,-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif}\
main{max-width:70rem;margin:0 auto}\
h1{font-size:1.5rem;margin:0 0 .25rem}\
h2{font-size:1.05rem;margin:2rem 0 .5rem;padding-bottom:.3rem;border-bottom:1px solid var(--rule)}\
p{margin:.4rem 0}\
.sub{color:var(--dim);margin:0 0 1.25rem}\
.note{color:var(--dim);font-size:.86rem}\
.scroll{overflow-x:auto;-webkit-overflow-scrolling:touch}\
table{border-collapse:collapse;width:100%;font-size:.88rem}\
th,td{text-align:left;padding:.4rem .55rem;border-bottom:1px solid var(--rule);vertical-align:top}\
th{font-weight:600;color:var(--dim);font-size:.78rem;text-transform:uppercase;letter-spacing:.04em}\
code,.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:.92em}\
.card{background:var(--card);border:1px solid var(--rule);border-radius:6px;\
padding:.85rem 1rem;margin:0 0 1rem}\
.iface{background:var(--card);border:1px solid var(--rule);border-radius:6px;\
margin:0 0 1rem;padding:.85rem 1rem}\
.iface.ingested{border-left:6px solid var(--warn)}\
.iface.exposed{border-left:6px solid var(--stop)}\
.iface.exposed.ingested{border-left:6px solid var(--stop);\
box-shadow:inset 6px 0 0 -3px var(--warn)}\
.id{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;\
font-size:.95rem;font-weight:600;word-break:break-all}\
.badges{margin:.35rem 0 .5rem}\
.badge{display:inline-block;padding:.08rem .45rem;margin:.12rem .3rem .12rem 0;\
border-radius:3px;font-size:.74rem;font-weight:700;letter-spacing:.04em;\
text-transform:uppercase;border:1px solid transparent;white-space:nowrap}\
.b-exposed{background:var(--stop-bg);color:var(--stop);border-color:var(--stop)}\
.b-dark{background:transparent;color:var(--dim);border-color:var(--rule)}\
.b-ingested{background:var(--warn-bg);color:var(--warn);border-color:var(--warn)}\
.b-derived{background:var(--warn-bg);color:var(--warn)}\
.b-idl{background:var(--cool-bg);color:var(--cool)}\
.b-destructive{background:var(--stop-bg);color:var(--stop);border-color:var(--stop)}\
.b-scope{background:var(--cool-bg);color:var(--cool)}\
.b-ok{background:var(--ok-bg);color:var(--ok)}\
.b-dry{background:transparent;color:var(--dim);border:1px dashed var(--dim)}\
.b-unknown{background:var(--warn-bg);color:var(--warn);border-color:var(--warn)}\
.absent{color:var(--dim);font-style:italic}\
.peers{margin:.25rem 0 .5rem}\
.peer{padding:.35rem 0;border-top:1px dashed var(--rule)}\
.desc{color:var(--dim);margin:.15rem 0 .5rem}\
.row-refuse{background:var(--stop-bg)}\
.row-dry{background:transparent;color:var(--dim)}\
tr.row-dry td{border-bottom:1px dashed var(--rule)}\
.summary{display:flex;flex-wrap:wrap;gap:.5rem 1.25rem;margin:.25rem 0 0}\
.stat{font-size:.85rem;color:var(--dim)}\
.stat b{color:var(--fg);font-size:1.05rem;font-weight:700}\
.stat.stop b{color:var(--stop)}\
.stat.warn b{color:var(--warn)}\
footer{margin-top:3rem;color:var(--dim);font-size:.8rem;\
border-top:1px solid var(--rule);padding-top:.75rem}";

/// One self-contained HTML file.
///
/// `title` is escaped like any other value: a page named after a repository id
/// that came off the wire is a page whose title is untrusted input.
pub fn page(title: &str, body: Markup) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>{}</style>\n</head>\n<body>\n<main>\n{}\n</main>\n\
         </body>\n</html>\n",
        escape(title),
        STYLE,
        body
    )
}

/// A page footer that says what the console is and, more importantly, what it
/// is not.
pub fn provenance_footer() -> Markup {
    Markup::element(
        "footer",
        "",
        Markup::text(
            "orbweaver-console renders what the registry, the differ and the audit already \
             decided. It takes no decision of its own, and it shows no number it was not given \
             — an absent field is rendered absent, never as a value.",
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_five_characters_html_gives_meaning_to_are_escaped() {
        assert_eq!(escape("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
    }

    /// `&` must go first or `<` becomes `&amp;lt;`.
    #[test]
    fn escaping_does_not_double_escape_its_own_output() {
        assert_eq!(escape("<"), "&lt;");
        assert_eq!(escape("&lt;"), "&amp;lt;");
    }

    #[test]
    fn text_is_the_only_door_and_it_escapes() {
        let m = Markup::text("<script>alert(1)</script>");
        assert!(!m.as_str().contains("<script"), "{m}");
        assert!(m.as_str().contains("&lt;script&gt;"), "{m}");
    }

    #[test]
    fn elements_nest_and_carry_their_class() {
        let m = Markup::element("div", "card", Markup::labelled("span", "id", "a<b"));
        assert_eq!(m.as_str(), "<div class=\"card\"><span class=\"id\">a&lt;b</span></div>");
    }

    #[test]
    fn a_page_fetches_nothing() {
        let html = page("t", Markup::text("body"));
        for forbidden in ["<script", "src=", "href=", "@import", "http://", "https://"] {
            assert!(!html.contains(forbidden), "page reaches out: {forbidden}");
        }
    }

    #[test]
    fn a_page_title_is_escaped_like_any_other_value() {
        let html = page("</title><script>x</script>", Markup::empty());
        assert!(!html.contains("<script"), "{html}");
    }
}
