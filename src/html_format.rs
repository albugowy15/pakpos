//! Readable HTML source previews without rewriting or rendering markup.

use html5gum::{DefaultEmitter, HtmlString, Span, Token, Tokenizer};

use crate::config::{HTML_FORMAT_MAX_DEPTH, HTML_RESPONSE_INDENT_WIDTH, RESPONSE_PREVIEW_LIMIT};

struct Element {
    name: HtmlString,
    block_children: bool,
    inline_content: bool,
    preserve: bool,
}

/// Format single-line source by inserting whitespace around block elements.
///
/// Source spans preserve spelling, attributes, entities, and embedded languages.
/// HTML tokenization handles quoted `>`, void tags, and script/style contents.
/// There is no DOM reconstruction, script execution, or asset loading.
/// Return `None` for unchanged source or when formatting exceeds preview limits.
pub(crate) fn format_html(text: &str) -> Option<String> {
    if text.trim().contains(['\n', '\r']) || text.len() > RESPONSE_PREVIEW_LIMIT {
        return None;
    }
    let mut emitter = DefaultEmitter::<usize>::new_with_span();
    emitter.naively_switch_states(true);
    let mut elements: Vec<Element> = Vec::new();
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for token in Tokenizer::new_with_emitter(text, emitter) {
        let token = token.ok()?;
        let span = match &token {
            Token::StartTag(tag) => tag.span,
            Token::EndTag(tag) => tag.span,
            Token::Comment(comment) => comment.span,
            Token::Doctype(doctype) => doctype.span,
            Token::String(_) | Token::Error(_) => continue,
        };
        let gap = text.get(cursor..span.start)?;
        if !gap.trim().is_empty()
            && let Some(parent) = elements.last_mut()
        {
            parent.inline_content = true;
        }
        output.push_str(gap);
        let raw = source_span(text, span)?;
        let preserve = elements.last().is_some_and(|parent| parent.preserve);
        match token {
            Token::StartTag(tag) => {
                if !preserve {
                    close_optional_elements(&mut elements, &tag.name);
                }
                let block = is_block(&tag.name);
                if !preserve && block {
                    new_line(&mut output, elements.len());
                    if let Some(parent) = elements.last_mut() {
                        parent.block_children = true;
                    }
                } else if let Some(parent) = elements.last_mut() {
                    parent.inline_content = true;
                }
                output.push_str(raw);
                if !tag.self_closing && !is_void(&tag.name) {
                    if elements.len() >= HTML_FORMAT_MAX_DEPTH {
                        return None;
                    }
                    let preserve = preserve || is_preserved(&tag.name);
                    elements.push(Element {
                        name: tag.name,
                        block_children: false,
                        inline_content: false,
                        preserve,
                    });
                }
            }
            Token::EndTag(tag) => {
                if let Some(index) = elements
                    .iter()
                    .rposition(|element| element.name == tag.name)
                {
                    if !preserve && is_block(&tag.name) && elements[index].block_children {
                        new_line(&mut output, index);
                    }
                    elements.truncate(index);
                }
                output.push_str(raw);
            }
            Token::Comment(_) | Token::Doctype(_) => {
                if !preserve && elements.last().is_none_or(|parent| !parent.inline_content) {
                    new_line(&mut output, elements.len());
                    if let Some(parent) = elements.last_mut() {
                        parent.block_children = true;
                    }
                }
                output.push_str(raw);
            }
            Token::String(_) | Token::Error(_) => unreachable!(),
        }
        cursor = span.end;
        if output.len() > RESPONSE_PREVIEW_LIMIT {
            return None;
        }
    }
    output.push_str(text.get(cursor..)?);
    (output.len() <= RESPONSE_PREVIEW_LIMIT && output != text).then_some(output)
}

fn source_span(text: &str, span: Span<usize>) -> Option<&str> {
    text.get(span.start..span.end)
}

fn new_line(output: &mut String, depth: usize) {
    if !output.is_empty() && !output.ends_with(['\n', '\r']) {
        output.push('\n');
    }
    output.extend(std::iter::repeat_n(' ', depth * HTML_RESPONSE_INDENT_WIDTH));
}

fn close_optional_elements(elements: &mut Vec<Element>, name: &[u8]) {
    let (targets, boundaries): (&[&[u8]], &[&[u8]]) = match name {
        b"li" => (&[b"li"], &[b"ul", b"ol", b"menu"]),
        b"dt" | b"dd" => (&[b"dt", b"dd"], &[b"dl"]),
        b"tr" => (&[b"tr"], &[b"table", b"tbody", b"thead", b"tfoot"]),
        b"td" | b"th" => (&[b"td", b"th"], &[b"tr", b"table"]),
        b"thead" | b"tbody" | b"tfoot" => (&[b"thead", b"tbody", b"tfoot"], &[b"table"]),
        b"option" => (&[b"option"], &[b"select", b"datalist", b"optgroup"]),
        b"optgroup" => (&[b"optgroup"], &[b"select"]),
        _ if is_block(name) => (&[b"p"], &[b"body", b"html", b"table"]),
        _ => return,
    };
    for (index, element) in elements.iter().enumerate().rev() {
        if boundaries.contains(&element.name.as_ref()) {
            break;
        }
        if targets.contains(&element.name.as_ref()) {
            elements.truncate(index);
            break;
        }
    }
}

fn is_preserved(name: &[u8]) -> bool {
    matches!(
        name,
        b"script"
            | b"style"
            | b"pre"
            | b"textarea"
            | b"code"
            | b"title"
            | b"xmp"
            | b"plaintext"
            | b"svg"
            | b"math"
    )
}

fn is_void(name: &[u8]) -> bool {
    matches!(
        name,
        b"area"
            | b"base"
            | b"br"
            | b"col"
            | b"embed"
            | b"hr"
            | b"img"
            | b"input"
            | b"link"
            | b"meta"
            | b"param"
            | b"source"
            | b"track"
            | b"wbr"
    )
}

fn is_block(name: &[u8]) -> bool {
    matches!(
        name,
        b"html"
            | b"head"
            | b"body"
            | b"title"
            | b"meta"
            | b"link"
            | b"base"
            | b"script"
            | b"style"
            | b"address"
            | b"article"
            | b"aside"
            | b"blockquote"
            | b"details"
            | b"dialog"
            | b"div"
            | b"dl"
            | b"dt"
            | b"dd"
            | b"fieldset"
            | b"figcaption"
            | b"figure"
            | b"footer"
            | b"form"
            | b"h1"
            | b"h2"
            | b"h3"
            | b"h4"
            | b"h5"
            | b"h6"
            | b"header"
            | b"hgroup"
            | b"hr"
            | b"main"
            | b"nav"
            | b"ol"
            | b"ul"
            | b"li"
            | b"menu"
            | b"p"
            | b"pre"
            | b"section"
            | b"summary"
            | b"table"
            | b"caption"
            | b"colgroup"
            | b"col"
            | b"thead"
            | b"tbody"
            | b"tfoot"
            | b"tr"
            | b"td"
            | b"th"
            | b"template"
            | b"select"
            | b"optgroup"
            | b"option"
            | b"textarea"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_html_documents_with_void_tags_and_inline_phrases() {
        let source = "<!DOCTYPE html><html><head><meta charset=utf-8><title>A &amp; B</title></head><body><main><h1>Hello <strong>world</strong>!</h1><p>one<br>two <img src='x' alt=\"a > b\"></p></main></body></html>";
        let expected = "<!DOCTYPE html>\n<html>\n  <head>\n    <meta charset=utf-8>\n    <title>A &amp; B</title>\n  </head>\n  <body>\n    <main>\n      <h1>Hello <strong>world</strong>!</h1>\n      <p>one<br>two <img src='x' alt=\"a > b\"></p>\n    </main>\n  </body>\n</html>";
        assert_eq!(format_html(source).unwrap(), expected);
        assert!(format_html(expected).is_none());
    }

    #[test]
    fn preserves_embedded_languages_preformatted_text_and_source_spelling() {
        let source = "<HTML><HEAD><style>.x::after{content:'<div> >';}</style><script>if(a<b){document.write(\"<p>hello</p>\");}</script></HEAD><BODY><!-- Keep > this --><pre>  a  &amp; <span> b </span></pre><p>a <code><b>x</b>  y</code> z</p><textarea> &lt;b&gt;  x </textarea></BODY></HTML>";
        let output = format_html(source).unwrap();
        for preserved in [
            "<style>.x::after{content:'<div> >';}</style>",
            "<script>if(a<b){document.write(\"<p>hello</p>\");}</script>",
            "<!-- Keep > this -->",
            "<pre>  a  &amp; <span> b </span></pre>",
            "<p>a <code><b>x</b>  y</code> z</p>",
            "<textarea> &lt;b&gt;  x </textarea>",
        ] {
            assert!(
                output.contains(preserved),
                "changed {preserved:?}: {output}"
            );
        }
        assert!(output.starts_with("<HTML>\n  <HEAD>"));
    }

    #[test]
    fn handles_optional_closing_tags_without_inventing_markup() {
        assert_eq!(
            format_html("<ul><li>one<li>two</ul>").unwrap(),
            "<ul>\n  <li>one\n  <li>two\n</ul>"
        );
        assert_eq!(
            format_html("<div><p>one<p>two</div>").unwrap(),
            "<div>\n  <p>one\n  <p>two\n</div>"
        );
        let nested = format_html("<ul><li>outer<ul><li>inner</ul><li>next</ul>").unwrap();
        assert!(nested.contains("\n      <li>inner"));
        assert!(nested.contains("\n  <li>next"));
    }

    #[test]
    fn leaves_multiline_plain_and_excessively_nested_source_unformatted() {
        for source in [
            "plain text",
            "<p>Hello <b>world</b>!</p>",
            "<div>\n  <p>already formatted</p>\n</div>",
        ] {
            assert!(format_html(source).is_none());
        }
        let nested = format!(
            "{}x{}",
            "<div>".repeat(HTML_FORMAT_MAX_DEPTH + 1),
            "</div>".repeat(HTML_FORMAT_MAX_DEPTH + 1)
        );
        assert!(format_html(&nested).is_none());
        let large = format!(
            "{}{}{}",
            "<div>".repeat(64),
            "<p>x</p>".repeat(RESPONSE_PREVIEW_LIMIT / 128),
            "</div>".repeat(64)
        );
        assert!(large.len() < RESPONSE_PREVIEW_LIMIT);
        assert!(format_html(&large).is_none());
    }
}
