//! Explicit formatting of textual request bodies, independent of GTK.

use std::fmt;

use crate::{config::XML_FORMAT_MAX_DEPTH, models::format_json};

#[derive(Clone, Copy)]
pub enum BodyFormat {
    Json,
    Xml,
    Auto,
}

#[derive(Debug)]
pub enum FormatError {
    Json(serde_json::Error),
    Xml(roxmltree::Error),
    UnsupportedText,
    XmlNestingLimit,
}

impl fmt::Display for FormatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(formatter, "Invalid JSON: {error}"),
            Self::Xml(error) => write!(formatter, "Invalid XML: {error}"),
            Self::UnsupportedText => formatter
                .write_str("Plain Text formatting supports JSON or XML. This body is neither."),
            Self::XmlNestingLimit => write!(
                formatter,
                "XML is nested too deeply to format (maximum {XML_FORMAT_MAX_DEPTH} levels)."
            ),
        }
    }
}

impl std::error::Error for FormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Xml(error) => Some(error),
            Self::UnsupportedText | Self::XmlNestingLimit => None,
        }
    }
}

pub fn format_body(
    text: &str,
    format: BodyFormat,
    indent_width: usize,
) -> Result<String, FormatError> {
    match format {
        BodyFormat::Json => format_json(text, indent_width).map_err(FormatError::Json),
        BodyFormat::Xml => format_xml(text, indent_width),
        BodyFormat::Auto if text.trim_start().starts_with('<') => format_xml(text, indent_width),
        BodyFormat::Auto => {
            format_json(text, indent_width).map_err(|_| FormatError::UnsupportedText)
        }
    }
}

fn format_xml(text: &str, indent_width: usize) -> Result<String, FormatError> {
    let document = roxmltree::Document::parse(text).map_err(FormatError::Xml)?;
    let root = document.root_element();
    let range = root.range();
    let mut output = String::with_capacity(text.len());
    let prefix = text[..range.start].trim();
    if !prefix.is_empty() {
        output.push_str(prefix);
        output.push('\n');
    }
    format_xml_node(root, text, &" ".repeat(indent_width), 0, &mut output)?;
    let suffix = text[range.end..].trim();
    if !suffix.is_empty() {
        output.push('\n');
        output.push_str(suffix);
    }
    Ok(output)
}

fn format_xml_node(
    node: roxmltree::Node<'_, '_>,
    text: &str,
    indent: &str,
    depth: usize,
    output: &mut String,
) -> Result<(), FormatError> {
    if depth >= XML_FORMAT_MAX_DEPTH {
        return Err(FormatError::XmlNestingLimit);
    }
    let range = node.range();
    let source = &text[range.clone()];
    let preserve_space = node
        .ancestors()
        .find_map(|ancestor| ancestor.attribute((roxmltree::NS_XML_URI, "space")))
        == Some("preserve");
    let children = node.children().filter(|child| !child.is_text());
    if preserve_space || children.clone().next().is_none() {
        output.push_str(source);
        return Ok(());
    }

    // Use raw source ranges rather than serializing a DOM. This keeps attribute
    // order, quotes, namespace prefixes, entities, CDATA, and comments intact.
    let mut quote = None;
    let opening_end = range.start
        + source
            .char_indices()
            .find_map(|(index, character)| match (quote, character) {
                (Some(delimiter), character) if delimiter == character => {
                    quote = None;
                    None
                }
                (None, '\'' | '"') => {
                    quote = Some(character);
                    None
                }
                (None, '>') => Some(index + 1),
                _ => None,
            })
            .expect("validated XML has an opening tag");
    let closing_start = range.start + source.rfind("</").expect("element has children");
    let mut cursor = opening_end;
    for child in children.clone() {
        // Significant text or CDATA makes this mixed content. Preserve the
        // whole subtree, including spaces around inline elements.
        if !text[cursor..child.range().start].chars().all(is_xml_space) {
            output.push_str(source);
            return Ok(());
        }
        cursor = child.range().end;
    }
    if !text[cursor..closing_start].chars().all(is_xml_space) {
        output.push_str(source);
        return Ok(());
    }

    output.push_str(&text[range.start..opening_end]);
    for child in children {
        append_indent(output, indent, depth + 1);
        if child.is_element() {
            format_xml_node(child, text, indent, depth + 1, output)?;
        } else {
            output.push_str(&text[child.range()]);
        }
    }
    append_indent(output, indent, depth);
    output.push_str(&text[closing_start..range.end]);
    Ok(())
}

fn is_xml_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\r' | '\n')
}

fn append_indent(output: &mut String, indent: &str, depth: usize) {
    output.push('\n');
    for _ in 0..depth {
        output.push_str(indent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_xml_preserving_declarations_attributes_and_markup() {
        let input = "<?xml version='1.0'?><root xmlns:n='urn:test' z='>' a=\"&amp;\"><!--keep--><?task go?><n:items><n:item/><n:item>value &amp; &#65;</n:item></n:items><data><![CDATA[<raw>]]></data></root><!--after-->";
        let expected = "<?xml version='1.0'?>\n<root xmlns:n='urn:test' z='>' a=\"&amp;\">\n  <!--keep-->\n  <?task go?>\n  <n:items>\n    <n:item/>\n    <n:item>value &amp; &#65;</n:item>\n  </n:items>\n  <data><![CDATA[<raw>]]></data>\n</root>\n<!--after-->";
        let output = format_body(input, BodyFormat::Xml, 2).unwrap();
        assert_eq!(output, expected);
        assert_eq!(format_body(&output, BodyFormat::Xml, 2).unwrap(), output);
    }

    #[test]
    fn preserves_mixed_content_cdata_and_explicit_whitespace() {
        for content in [
            "<p>Hello <b> world </b> !</p>",
            "<p><![CDATA[ ]]><b/> </p>",
            "<p> &amp; <b/> text</p>",
            "<p> &#32; <b/> </p>",
            "<p xml:space='preserve'>  <b><i/></b>\n</p>",
        ] {
            let input = format!("<root>{content}<other/></root>");
            assert_eq!(
                format_body(&input, BodyFormat::Xml, 2).unwrap(),
                format!("<root>\n  {content}\n  <other/>\n</root>")
            );
        }
    }

    #[test]
    fn detects_formats_for_plain_text_and_rejects_unrecognized_content() {
        assert_eq!(
            format_body("{\"a\":1}", BodyFormat::Auto, 2).unwrap(),
            "{\n  \"a\": 1\n}"
        );
        assert_eq!(
            format_body("<root><child/></root>", BodyFormat::Auto, 4).unwrap(),
            "<root>\n    <child/>\n</root>"
        );
        assert!(matches!(
            format_body("arbitrary plain text", BodyFormat::Auto, 2),
            Err(FormatError::UnsupportedText)
        ));
        assert!(matches!(
            format_body("<root><broken></root>", BodyFormat::Auto, 2),
            Err(FormatError::Xml(_))
        ));
    }

    #[test]
    fn rejects_malformed_xml_and_limits_nested_formatting() {
        for text in [
            "",
            "<root>",
            "<root><a></root>",
            "<a/><b/>",
            "<root a='1' a='2'/>",
            "<root>&unknown;</root>",
        ] {
            assert!(
                matches!(
                    format_body(text, BodyFormat::Xml, 2),
                    Err(FormatError::Xml(_))
                ),
                "accepted {text:?}"
            );
        }
        let input = format!(
            "{}<leaf/>{}",
            "<e>".repeat(XML_FORMAT_MAX_DEPTH),
            "</e>".repeat(XML_FORMAT_MAX_DEPTH)
        );
        assert!(matches!(
            format_body(&input, BodyFormat::Xml, 2),
            Err(FormatError::XmlNestingLimit)
        ));
    }
}
