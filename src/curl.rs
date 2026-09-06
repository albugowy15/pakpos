use std::fmt;

use crate::models::{HeaderRow, HttpMethod, RequestBody, RequestDraft, RequestSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlImport {
    pub request: RequestDraft,
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

pub fn to_command(draft: RequestDraft) -> Result<String, CurlError> {
    let entered_url = draft.url.trim().to_owned();
    let snapshot =
        RequestSnapshot::try_from(draft).map_err(|error| CurlError::new(error.to_string()))?;
    let mut arguments = vec![
        "curl".to_owned(),
        "--request".to_owned(),
        snapshot.method.as_str().to_owned(),
        "--url".to_owned(),
        shell_quote(&entered_url),
    ];

    for header in snapshot.headers {
        arguments.push("--header".to_owned());
        let value = if header.value.is_empty() {
            format!("{};", header.name)
        } else {
            format!("{}: {}", header.name, header.value)
        };
        arguments.push(shell_quote(&value));
    }

    if let RequestBody::Json(body) = snapshot.body {
        arguments.push("--data-raw".to_owned());
        arguments.push(shell_quote(&body));
    }

    Ok(arguments.join(" "))
}

pub fn from_command(command: &str) -> Result<CurlImport, CurlError> {
    let normalized = remove_line_continuations(command);
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
            "-F" | "--form" | "--form-string" => {
                return Err(CurlError::new(
                    "Multipart cURL imports are not available yet. Remove the form fields or enter this request manually.",
                ));
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
            value if value.starts_with('-') => return Err(unsupported_option(value)),
            value => set_url(&mut url, value)?,
        }
        index += 1;
    }

    let url = url.ok_or_else(|| CurlError::new("The cURL command does not contain a URL."))?;
    let body = if data_parts.is_empty() {
        RequestBody::None
    } else {
        let text = data_parts.join("&");
        serde_json::from_str::<serde_json::Value>(&text).map_err(|error| {
            CurlError::new(format!(
                "Pakpos currently imports cURL bodies as JSON, but this body is invalid at line {}, column {}: {error}",
                error.line(),
                error.column()
            ))
        })?;
        RequestBody::Json(text)
    };

    if json_option {
        add_header_if_missing(&mut headers, "Content-Type", "application/json");
        add_header_if_missing(&mut headers, "Accept", "application/json");
    }
    let inferred_method = if matches!(body, RequestBody::Json(_)) {
        HttpMethod::Post
    } else {
        HttpMethod::Get
    };
    let request = RequestDraft {
        method: method.unwrap_or(inferred_method),
        url,
        headers,
        body,
    };
    RequestSnapshot::try_from(request.clone())
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

fn parse_inline_data(value: &str, option: &str) -> Result<String, CurlError> {
    if value.starts_with('@') {
        return Err(CurlError::new(format!(
            "File-backed data in {option} is not supported. Paste the JSON body directly."
        )));
    }
    Ok(value.to_owned())
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

fn remove_line_continuations(command: &str) -> String {
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
    normalized
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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
        let request = RequestDraft {
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
        assert_eq!(&imported.request.headers, &request.headers[..2]);
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
    fn rejects_unrepresentable_body_without_changing_editor() {
        let error = from_command("curl -d 'plain text' https://example.com").unwrap_err();
        assert!(error.to_string().contains("imports cURL bodies as JSON"));
    }

    #[test]
    fn exports_empty_header_with_curl_empty_value_syntax() {
        let request = RequestDraft {
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
}
