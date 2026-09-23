//! Safe conversion between Pakpos requests and a supported cURL command subset.
//!
//! Export emits shell-quoted text suitable for copying to a terminal. Import uses
//! `shell_words` only as a lexer: it never invokes a shell or executes pasted
//! input. Options are accepted only when their behavior maps faithfully to the
//! native [`Request`] model; unsafe or unrepresentable options produce an error,
//! while harmless presentation differences may produce warnings.
//!
//! Keep this module a pure converter. Clipboard access belongs to the UI, file
//! access belongs to request validation/runtime work, and HTTP behavior belongs
//! to the network adapter.

use std::{
    borrow::{Borrow, Cow},
    fmt,
};

use crate::models::{
    FormField, HeaderRow, HttpMethod, MultipartField, MultipartValue, Request, RequestBody,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlImport {
    pub request: Request,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlError(String);

impl CurlError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CurlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CurlError {}

pub fn to_command(request: impl Borrow<Request>) -> Result<String, CurlError> {
    let request = request.borrow();
    request
        .check(true)
        .map_err(|error| CurlError::new(error.to_string()))?;
    let mut command = format!("curl --request {} --url ", request.method);
    shell_quote_into(&mut command, request.url.trim());

    for header in request.headers.iter().filter(|header| {
        header.enabled && !(header.name.trim().is_empty() && header.value.is_empty())
    }) {
        command.push_str(" --header ");
        let value = if header.value.is_empty() {
            format!("{};", header.name)
        } else {
            format!("{}: {}", header.name, header.value)
        };
        shell_quote_into(&mut command, &value);
    }

    let has_content_type = request.headers.iter().any(|header| {
        header.enabled
            && !(header.name.trim().is_empty() && header.value.is_empty())
            && header.name.eq_ignore_ascii_case("content-type")
    });
    let generated_content_type = match &request.body {
        RequestBody::Json(_) => Some("application/json"),
        RequestBody::FormUrlEncoded(_) => Some("application/x-www-form-urlencoded"),
        RequestBody::Text(_) => Some("text/plain"),
        RequestBody::None | RequestBody::Multipart(_) => None,
    };
    if !has_content_type && let Some(content_type) = generated_content_type {
        command.push_str(" --header ");
        shell_quote_into(&mut command, &format!("Content-Type: {content_type}"));
    }

    match &request.body {
        RequestBody::None => {}
        RequestBody::Json(body) | RequestBody::Text(body) => {
            command.push_str(" --data-raw ");
            shell_quote_into(&mut command, body);
        }
        RequestBody::FormUrlEncoded(fields) => {
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for field in fields
                .iter()
                .filter(|field| field.enabled && !(field.name.is_empty() && field.value.is_empty()))
            {
                serializer.append_pair(&field.name, &field.value);
            }
            command.push_str(" --data-raw ");
            shell_quote_into(&mut command, &serializer.finish());
        }
        RequestBody::Multipart(fields) => {
            for field in fields
                .iter()
                .filter(|field| field.enabled && !field.name.trim().is_empty())
            {
                match &field.value {
                    MultipartValue::Text(value) => {
                        command.push_str(" --form-string ");
                        shell_quote_into(&mut command, &format!("{}={value}", field.name));
                    }
                    MultipartValue::File(path) => {
                        command.push_str(" --form ");
                        shell_quote_into(
                            &mut command,
                            &format!(
                                "{}=@{}",
                                field.name,
                                quote_form_file_path(&path.to_string_lossy())
                            ),
                        );
                    }
                }
            }
        }
    }
    Ok(command)
}

pub fn from_command(command: &str) -> Result<CurlImport, CurlError> {
    let normalized = remove_line_continuations(command);
    // Tokenization reproduces shell quoting rules without giving pasted text an
    // opportunity to execute substitutions, redirects, or arbitrary programs.
    let mut arguments = shell_words::split(&normalized).map_err(|error| {
        CurlError::new(format!("The cURL command has invalid quoting: {error}."))
    })?;
    if arguments.first().is_some_and(|argument| argument == "$") {
        arguments.remove(0);
    }
    let Some(program) = arguments.first() else {
        return Err(CurlError::new("Paste a cURL command."));
    };
    let program = program.rsplit('/').next().unwrap_or(program);
    if !matches!(program, "curl" | "curl.exe") {
        return Err(CurlError::new("The pasted text must start with curl."));
    }

    let mut method = None;
    let mut url = None;
    let mut headers = Vec::new();
    let mut data_parts = Vec::new();
    let mut multipart_fields = Vec::new();
    let mut json_option = false;
    let mut warnings = Vec::new();
    let mut index = 1;
    let mut options_ended = false;

    while index < arguments.len() {
        let argument = &arguments[index];
        if options_ended {
            set_url(&mut url, argument)?;
            index += 1;
            continue;
        }
        if argument == "--" {
            options_ended = true;
            index += 1;
            continue;
        }

        if argument.starts_with("--")
            && let Some((option, value)) = argument.split_once('=')
        {
            match option {
                "--request" => method = Some(parse_method(value)?),
                "--url" => set_url(&mut url, value)?,
                "--header" => headers.push(parse_header(value)?),
                "--data" | "--data-ascii" | "--data-raw" | "--data-binary" => {
                    data_parts.push(parse_inline_data(value, option)?)
                }
                "--form" => multipart_fields.push(parse_form(value, false)?),
                "--form-string" => multipart_fields.push(parse_form(value, true)?),
                "--json" => {
                    json_option = true;
                    data_parts.push(parse_inline_data(value, option)?);
                }
                _ => handle_long_flag(option, &mut warnings)?,
            }
            index += 1;
            continue;
        }

        match argument.as_str() {
            "-X" | "--request" => {
                let value = next_value(&arguments, &mut index, argument)?;
                method = Some(parse_method(value)?);
            }
            "--url" => {
                let value = next_value(&arguments, &mut index, argument)?;
                set_url(&mut url, value)?;
            }
            "-H" | "--header" => {
                let value = next_value(&arguments, &mut index, argument)?;
                headers.push(parse_header(value)?);
            }
            "-d" | "--data" | "--data-ascii" | "--data-raw" | "--data-binary" => {
                let value = next_value(&arguments, &mut index, argument)?;
                data_parts.push(parse_inline_data(value, argument)?);
            }
            "--json" => {
                let value = next_value(&arguments, &mut index, argument)?;
                json_option = true;
                data_parts.push(parse_inline_data(value, argument)?);
            }
            "-I" | "--head" => method = Some(HttpMethod::Head),
            "-L" | "--location" => warnings.push(
                "Ignored --location; Pakpos displays redirect responses without following them."
                    .to_owned(),
            ),
            "-s" | "--silent" | "-S" | "--show-error" | "--compressed" | "--globoff"
            | "--include" | "--fail" | "--fail-with-body" | "-v" | "--verbose" => {}
            "-k" | "--insecure" => {
                return Err(CurlError::new(
                    "Pakpos always verifies HTTPS certificates. Remove --insecure before pasting.",
                ));
            }
            "-F" | "--form" => {
                let value = next_value(&arguments, &mut index, argument)?;
                multipart_fields.push(parse_form(value, false)?);
            }
            "--form-string" => {
                let value = next_value(&arguments, &mut index, argument)?;
                multipart_fields.push(parse_form(value, true)?);
            }
            "--data-urlencode" | "-G" | "--get" | "-u" | "--user" | "-b" | "--cookie" | "-A"
            | "--user-agent" | "-e" | "--referer" | "-K" | "--config" | "--connect-timeout"
            | "-m" | "--max-time" | "--proxy" | "-x" | "--cert" | "--key" => {
                return Err(unsupported_option(argument));
            }
            value if value.starts_with("-X") && value.len() > 2 => {
                method = Some(parse_method(&value[2..])?);
            }
            value if value.starts_with("-H") && value.len() > 2 => {
                headers.push(parse_header(&value[2..])?);
            }
            value if value.starts_with("-d") && value.len() > 2 => {
                data_parts.push(parse_inline_data(&value[2..], "-d")?);
            }
            value if value.starts_with("-F") && value.len() > 2 => {
                multipart_fields.push(parse_form(&value[2..], false)?);
            }
            value if value.starts_with('-') => return Err(unsupported_option(value)),
            value => set_url(&mut url, value)?,
        }
        index += 1;
    }

    let url = url.ok_or_else(|| CurlError::new("The cURL command does not contain a URL."))?;
    if !data_parts.is_empty() && !multipart_fields.is_empty() {
        return Err(CurlError::new(
            "Pakpos cannot import a cURL command that mixes data and multipart form options.",
        ));
    }
    if json_option {
        add_header_if_missing(&mut headers, "Content-Type", "application/json");
        add_header_if_missing(&mut headers, "Accept", "application/json");
    }
    let body = if !multipart_fields.is_empty() {
        RequestBody::Multipart(multipart_fields)
    } else if data_parts.is_empty() {
        RequestBody::None
    } else {
        let text = data_parts.join("&");
        let content_type = headers
            .iter()
            .find(|header| header.enabled && header.name.eq_ignore_ascii_case("content-type"))
            .map(|header| {
                header
                    .value
                    .split(';')
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase()
            });
        let is_json = json_option
            || content_type
                .as_deref()
                .is_some_and(|value| value == "application/json" || value.ends_with("+json"))
            || (content_type.is_none() && crate::models::validate_json(&text).is_ok());
        if is_json {
            crate::models::validate_json(&text).map_err(|error| {
                CurlError::new(format!(
                    "The JSON body is invalid at line {}, column {}: {error}",
                    error.line(),
                    error.column()
                ))
            })?;
            RequestBody::Json(text)
        } else if content_type.as_deref() == Some("text/plain") {
            RequestBody::Text(text)
        } else if content_type.as_deref() == Some("application/x-www-form-urlencoded")
            || content_type.is_none()
        {
            RequestBody::FormUrlEncoded(
                url::form_urlencoded::parse(text.as_bytes())
                    .map(|(name, value)| FormField::enabled(name, value))
                    .collect(),
            )
        } else {
            RequestBody::Text(text)
        }
    };

    let inferred_method = if matches!(body, RequestBody::None) {
        HttpMethod::Get
    } else {
        HttpMethod::Post
    };
    let request = Request {
        method: method.unwrap_or(inferred_method),
        url,
        headers,
        body,
    };
    // Imported file references may no longer exist on this machine. Preserve
    // them for user reselection and defer filesystem validation until sending.
    request
        .check(false)
        .map_err(|error| CurlError::new(error.to_string()))?;

    Ok(CurlImport { request, warnings })
}

fn next_value<'a>(
    arguments: &'a [String],
    index: &mut usize,
    option: &str,
) -> Result<&'a str, CurlError> {
    *index += 1;
    arguments
        .get(*index)
        .map(String::as_str)
        .ok_or_else(|| CurlError::new(format!("The {option} option needs a value.")))
}

fn parse_method(value: &str) -> Result<HttpMethod, CurlError> {
    value
        .to_ascii_uppercase()
        .parse()
        .map_err(|_| CurlError::new(format!("Pakpos does not support the {value} method.")))
}

fn parse_header(value: &str) -> Result<HeaderRow, CurlError> {
    if value.starts_with('@') {
        return Err(CurlError::new(
            "Header files are not supported. Paste the header values directly.",
        ));
    }
    if let Some((name, value)) = value.split_once(':') {
        if value.is_empty() {
            return Err(CurlError::new(format!(
                "The cURL header ‘{name}:’ suppresses an automatic header, which Pakpos cannot import safely. Use ‘{name};’ to send an empty value."
            )));
        }
        return Ok(HeaderRow::enabled(name.trim(), value.trim_start()));
    }
    if let Some(name) = value.strip_suffix(';') {
        return Ok(HeaderRow::enabled(name.trim(), ""));
    }
    Err(CurlError::new(format!(
        "The cURL header ‘{value}’ must contain a colon."
    )))
}

fn parse_inline_data<'a>(value: &'a str, option: &str) -> Result<&'a str, CurlError> {
    if value.starts_with('@') {
        return Err(CurlError::new(format!(
            "File-backed data in {option} is not supported. Paste the request body directly."
        )));
    }
    Ok(value)
}

fn parse_form(value: &str, literal: bool) -> Result<MultipartField, CurlError> {
    let (name, value) = value
        .split_once('=')
        .ok_or_else(|| CurlError::new("A multipart cURL field must use the form name=value."))?;
    if name.is_empty() {
        return Err(CurlError::new("A multipart cURL field needs a name."));
    }
    if literal {
        return Ok(MultipartField::text(name, value));
    }
    if value.starts_with('<') {
        return Err(CurlError::new(
            "Multipart fields that read file contents as text are not supported. Use a text value or @file upload.",
        ));
    }
    if let Some(path) = value.strip_prefix('@') {
        if path.is_empty() {
            return Err(CurlError::new("A multipart file field needs a path."));
        }
        return Ok(MultipartField::file(name, parse_form_file_path(path)?));
    }
    if value.contains(';') {
        return Err(CurlError::new(
            "Multipart form modifiers are not supported. Use --form-string for literal text.",
        ));
    }
    Ok(MultipartField::text(name, value))
}

fn quote_form_file_path(path: &str) -> String {
    let mut quoted = String::with_capacity(path.len() + 2);
    quoted.push('"');
    for character in path.chars() {
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

fn parse_form_file_path(value: &str) -> Result<String, CurlError> {
    if !value.starts_with('"') {
        if value.contains(';') {
            return Err(CurlError::new(
                "Multipart file modifiers are not supported. Select the file directly in Pakpos.",
            ));
        }
        return Ok(value.to_owned());
    }

    let mut path = String::new();
    let mut characters = value[1..].chars();
    while let Some(character) = characters.next() {
        match character {
            '"' => {
                if characters.next().is_some() {
                    return Err(CurlError::new(
                        "Multipart file modifiers are not supported. Select the file directly in Pakpos.",
                    ));
                }
                if path.is_empty() {
                    return Err(CurlError::new("A multipart file field needs a path."));
                }
                return Ok(path);
            }
            '\\' => match characters.next() {
                Some(next @ ('"' | '\\')) => path.push(next),
                Some(next) => {
                    path.push('\\');
                    path.push(next);
                }
                None => {
                    return Err(CurlError::new(
                        "The quoted multipart file path ends with an incomplete escape.",
                    ));
                }
            },
            _ => path.push(character),
        }
    }

    Err(CurlError::new(
        "The multipart file path has an unclosed quote.",
    ))
}

fn set_url(url: &mut Option<String>, value: &str) -> Result<(), CurlError> {
    if url.is_some() {
        return Err(CurlError::new(
            "Pakpos can import one URL from a cURL command at a time.",
        ));
    }
    *url = Some(value.to_owned());
    Ok(())
}

fn add_header_if_missing(headers: &mut Vec<HeaderRow>, name: &str, value: &str) {
    if !headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case(name))
    {
        headers.push(HeaderRow::enabled(name, value));
    }
}

fn handle_long_flag(option: &str, warnings: &mut Vec<String>) -> Result<(), CurlError> {
    match option {
        "--location" => {
            warnings.push(
                "Ignored --location; Pakpos displays redirect responses without following them."
                    .to_owned(),
            );
            Ok(())
        }
        "--silent" | "--show-error" | "--compressed" | "--globoff" | "--include" | "--fail"
        | "--fail-with-body" | "--verbose" => Ok(()),
        "--insecure" => Err(CurlError::new(
            "Pakpos always verifies HTTPS certificates. Remove --insecure before pasting.",
        )),
        other => Err(unsupported_option(other)),
    }
}

fn unsupported_option(option: &str) -> CurlError {
    CurlError::new(format!(
        "The cURL option ‘{option}’ is not supported for import. Remove it or enter its effect manually."
    ))
}

fn remove_line_continuations(command: &str) -> Cow<'_, str> {
    if !command.contains("\\\n") {
        return Cow::Borrowed(command);
    }
    let mut normalized = String::with_capacity(command.len());
    let mut characters = command.chars().peekable();
    let mut in_single_quotes = false;
    while let Some(character) = characters.next() {
        if character == '\'' {
            in_single_quotes = !in_single_quotes;
            normalized.push(character);
        } else if character == '\\'
            && !in_single_quotes
            && characters.peek().is_some_and(|next| *next == '\n')
        {
            characters.next();
        } else {
            normalized.push(character);
        }
    }
    Cow::Owned(normalized)
}

fn shell_quote_into(output: &mut String, value: &str) {
    output.reserve(value.len() + 2 + value.bytes().filter(|byte| *byte == b'\'').count() * 4);
    output.push('\'');
    let mut parts = value.split('\'');
    if let Some(first) = parts.next() {
        output.push_str(first);
    }
    for part in parts {
        output.push_str("'\"'\"'");
        output.push_str(part);
    }
    output.push('\'');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_postman_style_json_command() {
        let imported = from_command(
            r#"curl --location 'https://example.com/items?b=2&a=1' --header 'Authorization: Bearer secret' --header 'X-Tag: one' --header 'X-Tag: two' --data-raw '{"name":"Pakpos"}'"#,
        )
        .unwrap();

        assert_eq!(imported.request.method, HttpMethod::Post);
        assert_eq!(imported.request.url, "https://example.com/items?b=2&a=1");
        assert_eq!(imported.request.headers.len(), 3);
        assert_eq!(
            imported.request.body,
            RequestBody::Json(r#"{"name":"Pakpos"}"#.to_owned())
        );
        assert_eq!(imported.warnings.len(), 1);
    }

    #[test]
    fn supports_attached_short_options_and_prompt_prefix() {
        let imported = from_command(
            "$ curl -XPATCH -H'Content-Type: application/json' -d'42' https://example.com",
        )
        .unwrap();
        assert_eq!(imported.request.method, HttpMethod::Patch);
        assert_eq!(imported.request.body, RequestBody::Json("42".to_owned()));
    }

    #[test]
    fn imports_multiline_command_and_inline_binary_json() {
        let imported = from_command(
            r#"curl --request POST \
             --url 'https://example.com/items' \
             --data-binary '{"ok":true}'"#,
        )
        .unwrap();
        assert_eq!(imported.request.method, HttpMethod::Post);
        assert_eq!(
            imported.request.body,
            RequestBody::Json("{\"ok\":true}".to_owned())
        );
    }

    #[test]
    fn round_trips_quotes_newlines_and_duplicate_headers() {
        let request = Request {
            method: HttpMethod::Put,
            url: "https://example.com/people?name=O'Reilly".to_owned(),
            headers: vec![
                HeaderRow::enabled("X-Tag", "one"),
                HeaderRow::enabled("X-Tag", "two"),
                HeaderRow {
                    enabled: false,
                    name: "X-Skip".to_owned(),
                    value: "hidden".to_owned(),
                },
            ],
            body: RequestBody::Json("{\n  \"name\": \"O'Reilly\"\n}".to_owned()),
        };
        let command = to_command(request.clone()).unwrap();
        let imported = from_command(&command).unwrap();

        assert_eq!(imported.request.method, request.method);
        assert_eq!(imported.request.url, request.url);
        assert_eq!(&imported.request.headers[..2], &request.headers[..2]);
        assert_eq!(
            imported.request.headers[2],
            HeaderRow::enabled("Content-Type", "application/json")
        );
        assert_eq!(imported.request.body, request.body);
    }

    #[test]
    fn json_option_adds_curl_default_headers() {
        let imported = from_command("curl --json '{}' https://example.com").unwrap();
        assert_eq!(imported.request.method, HttpMethod::Post);
        assert_eq!(imported.request.headers.len(), 2);
        assert_eq!(imported.request.headers[0].name, "Content-Type");
        assert_eq!(imported.request.headers[1].name, "Accept");
    }

    #[test]
    fn rejects_insecure_and_file_backed_data() {
        assert!(from_command("curl -k https://example.com").is_err());
        assert!(from_command("curl -d @body.json https://example.com").is_err());
    }

    #[test]
    fn imports_form_and_plain_text_bodies() {
        let form = from_command("curl -d 'name=Pakpos&tag=one+two' https://example.com").unwrap();
        assert_eq!(
            form.request.body,
            RequestBody::FormUrlEncoded(vec![
                FormField::enabled("name", "Pakpos"),
                FormField::enabled("tag", "one two"),
            ])
        );

        let text =
            from_command("curl -H 'Content-Type: text/plain' -d 'first line' https://example.com")
                .unwrap();
        assert_eq!(
            text.request.body,
            RequestBody::Text("first line".to_owned())
        );
    }

    #[test]
    fn exports_default_content_types_for_textual_bodies() {
        for (body, expected_content_type) in [
            (
                RequestBody::FormUrlEncoded(vec![FormField::enabled("name", "Pakpos")]),
                "application/x-www-form-urlencoded",
            ),
            (RequestBody::Text("hello".to_owned()), "text/plain"),
        ] {
            let command = to_command(Request {
                method: HttpMethod::Post,
                url: "https://example.com".to_owned(),
                body,
                ..Request::default()
            })
            .unwrap();
            assert!(command.contains(&format!("Content-Type: {expected_content_type}")));
        }
    }

    #[test]
    fn exports_empty_header_with_curl_empty_value_syntax() {
        let request = Request {
            method: HttpMethod::Get,
            url: "https://example.com".to_owned(),
            headers: vec![HeaderRow::enabled("X-Empty", "")],
            body: RequestBody::None,
        };
        let command = to_command(request).unwrap();
        assert!(command.contains("--header 'X-Empty;'"));
        let imported = from_command(&command).unwrap();
        assert_eq!(imported.request.headers[0].value, "");
    }

    #[test]
    fn round_trips_multipart_text_and_file_fields() {
        let file_path = std::env::current_dir().unwrap().join("Cargo.toml");
        let request = Request {
            method: HttpMethod::Patch,
            url: "https://example.com/upload".to_owned(),
            headers: Vec::new(),
            body: RequestBody::Multipart(vec![
                MultipartField::text("tag", "one"),
                MultipartField::text("tag", "@literal"),
                MultipartField::file("asset", &file_path),
            ]),
        };

        let command = to_command(request.clone()).unwrap();
        assert!(command.contains("--form-string 'tag=@literal'"));
        let imported = from_command(&command).unwrap();
        assert_eq!(imported.request, request);
    }

    #[test]
    fn imports_unavailable_multipart_file_for_reselection() {
        let unavailable = "/missing/pakpos-import-review-file.bin";
        let imported = from_command(&format!(
            "curl --form 'asset=@{unavailable}' https://example.com/upload"
        ))
        .unwrap();

        assert_eq!(
            imported.request.body,
            RequestBody::Multipart(vec![MultipartField::file("asset", unavailable)])
        );
        assert!(matches!(
            imported.request.validated().unwrap_err(),
            crate::models::ValidationError::UnreadableMultipartFile { row: 1, .. }
        ));
    }

    #[test]
    fn round_trips_semicolon_in_multipart_file_path() {
        let file_path =
            std::env::temp_dir().join(format!("pakpos-curl;review-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&file_path, b"review fixture").unwrap();
        let request = Request {
            method: HttpMethod::Post,
            url: "https://example.com/upload".to_owned(),
            headers: Vec::new(),
            body: RequestBody::Multipart(vec![MultipartField::file("asset", &file_path)]),
        };

        let command = to_command(request.clone()).unwrap();
        assert!(command.contains("=@\""));
        assert!(command.contains(';'));
        let imported = from_command(&command).unwrap();
        assert_eq!(imported.request, request);
        std::fs::remove_file(file_path).unwrap();
    }

    #[test]
    fn rejects_multipart_modifiers_and_mixed_body_modes() {
        assert!(
            from_command("curl -F 'asset=@Cargo.toml;type=text/plain' https://example.com")
                .is_err()
        );
        assert!(from_command("curl -d '{}' -F 'name=value' https://example.com").is_err());
    }
}
