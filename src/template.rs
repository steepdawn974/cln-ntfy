// Copyright (c) 2022-2026 Yuki Kishimoto
// Distributed under the MIT software license

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::DateTime;
use serde_json::Value;

use crate::NotificationEvent;

/// The rendered result of applying a template to a notification payload.
pub struct RenderedPayload {
    pub title: Option<String>,
    pub message: Option<String>,
    pub tags: Vec<String>,
}

/// A user-defined message template loaded from a TOML-like file.
///
/// The file is a simple line-based format (not full TOML, to avoid a heavy
/// dependency). Each event has an optional `[event_name]` section with
/// `title`, `message`, and `tags` keys. A `[default]` section applies to any
/// event without its own section. Values support `{{path.to.field}}`
/// placeholders that are resolved against the raw CLN notification JSON.
///
/// Example file:
///
/// ```text
/// [default]
/// title = "CLN: {{event}}"
/// message = "{{json}}"
///
/// [invoice_payment]
/// title = "Payment received"
/// message = "+{{invoice_payment.msat}} msat"
/// tags = ["zap", "money"]
/// ```
#[derive(Debug, Clone)]
pub struct Template {
    sections: HashMap<String, TemplateSection>,
}

#[derive(Debug, Clone, Default)]
struct TemplateSection {
    title: Option<String>,
    message: Option<String>,
    tags: Vec<String>,
}

impl Template {
    /// Load and parse a template file from disk.
    ///
    /// Returns `Ok` with an empty template (no sections) if the file is empty,
    /// or an error if the file exists but is malformed.
    pub fn load(path: &Path) -> Result<Self, anyhow::Error> {
        let content = fs::read_to_string(path).map_err(|e| {
            anyhow::anyhow!("Could not read template file `{}`: {e}", path.display())
        })?;
        Self::parse(&content)
    }

    /// Parse template content from a string (mainly for testing).
    pub fn parse(content: &str) -> Result<Self, anyhow::Error> {
        let mut sections: HashMap<String, TemplateSection> = HashMap::new();
        let mut current_section: Option<String> = None;
        let mut current = TemplateSection::default();

        for (line_no, raw_line) in content.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Section header: [event_name]
            if line.starts_with('[') && line.ends_with(']') {
                // Save the previous section.
                if let Some(name) = current_section.take() {
                    sections.insert(name, std::mem::take(&mut current));
                }
                let name = line[1..line.len() - 1].trim().to_string();
                if name.is_empty() {
                    anyhow::bail!("Line {}: empty section name", line_no + 1);
                }
                current_section = Some(name);
                continue;
            }

            // key = value
            let Some((key, val)) = line.split_once('=') else {
                anyhow::bail!(
                    "Line {}: expected `key = value` or `[section]`, got: {line}",
                    line_no + 1
                );
            };
            let key = key.trim();
            let val = val.trim();

            match key {
                "title" => {
                    current.title = Some(parse_string_val(val, line_no)?);
                }
                "message" => {
                    current.message = Some(parse_string_val(val, line_no)?);
                }
                "tags" => {
                    current.tags = parse_tags_val(val, line_no)?;
                }
                _ => {
                    anyhow::bail!(
                        "Line {}: unknown key `{key}` (expected title, message, or tags)",
                        line_no + 1
                    );
                }
            }
        }

        // Save the last section.
        if let Some(name) = current_section {
            sections.insert(name, current);
        }

        Ok(Template { sections })
    }

    /// Render a payload for the given event, or `None` if no template section
    /// matches (and the caller should use the built-in default).
    pub fn render(&self, event: NotificationEvent, value: &Value) -> Option<RenderedPayload> {
        let section = self
            .sections
            .get(event.name())
            .or_else(|| self.sections.get("default"))?;

        let event_name = event.name();
        let json = value.to_string();

        Some(RenderedPayload {
            title: section
                .title
                .as_ref()
                .map(|t| substitute(t, value, event_name, &json)),
            message: section
                .message
                .as_ref()
                .map(|m| substitute(m, value, event_name, &json)),
            tags: section
                .tags
                .iter()
                .map(|t| substitute(t, value, event_name, &json))
                .collect(),
        })
    }
}

/// Parse a quoted string value: `"hello"` -> `hello`.
///
/// The escape sequences `\n` (newline) and `\t` (tab) are expanded to their
/// literal characters. `\\` becomes a single backslash.
fn parse_string_val(val: &str, line_no: usize) -> Result<String, anyhow::Error> {
    let val = val.trim();
    if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
        let inner = &val[1..val.len() - 1];
        Ok(unescape(inner))
    } else {
        anyhow::bail!(
            "Line {}: expected a quoted string like \"value\", got: {val}",
            line_no + 1
        )
    }
}

/// Expand `\n`, `\t`, and `\\` escape sequences in a string.
fn unescape(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Parse a tags array: `["zap", "money"]` -> `["zap", "money"]`.
fn parse_tags_val(val: &str, line_no: usize) -> Result<Vec<String>, anyhow::Error> {
    let val = val.trim();
    if !val.starts_with('[') || !val.ends_with(']') {
        anyhow::bail!(
            "Line {}: expected an array like [\"tag1\", \"tag2\"], got: {val}",
            line_no + 1
        );
    }
    let inner = &val[1..val.len() - 1];
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut tags = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.len() >= 2 && part.starts_with('"') && part.ends_with('"') {
            tags.push(part[1..part.len() - 1].to_string());
        } else {
            anyhow::bail!(
                "Line {}: expected quoted tag string, got: {part}",
                line_no + 1
            );
        }
    }
    Ok(tags)
}

/// Replace `{{...}}` placeholders in a template string.
///
/// Supported placeholders:
/// - `{{event}}` — the event name (e.g. `invoice_payment`)
/// - `{{json}}` — the full raw notification JSON
/// - `{{path.to.field}}` — dotted path into the notification JSON, e.g.
///   `{{invoice_payment.msat}}` resolves `value["invoice_payment"]["msat"]`.
///   If the path doesn't exist, the placeholder is replaced with an empty
///   string.
/// - `{{path / 1000}}` — arithmetic on a numeric path. Supports `+`, `-`,
///   `*`, `/` with integer constants, left-to-right. e.g.
///   `{{invoice_payment.msat / 1000}}` converts msat to sat.
/// - `{{path | date}}` — converts a Unix epoch timestamp (integer or float)
///   to `YYYY-MM-DD HH:MM:SS UTC`.
/// - `{{path / 1000 | date}}` — arithmetic then filter, combined with `|`.
fn substitute(tmpl: &str, value: &Value, event_name: &str, json: &str) -> String {
    let mut result = String::with_capacity(tmpl.len());
    let mut rest = tmpl;

    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        if let Some(end) = after_start.find("}}") {
            let expr = after_start[..end].trim();
            let replacement = evaluate_expr(expr, value, event_name, json);
            result.push_str(&replacement);
            rest = &after_start[end + 2..];
        } else {
            // No closing }}, keep the rest as-is.
            result.push_str("{{");
            rest = after_start;
        }
    }
    result.push_str(rest);
    result
}

/// Evaluate a `{{...}}` expression, which may be a simple keyword, a path,
/// a path with arithmetic, or a path with a filter.
fn evaluate_expr(expr: &str, value: &Value, event_name: &str, json: &str) -> String {
    // Special keywords.
    if expr == "event" {
        return event_name.to_string();
    }
    if expr == "json" {
        return json.to_string();
    }

    // Split into expression part and filter part on `|`.
    let (expr_part, filter) = match expr.split_once('|') {
        Some((e, f)) => (e.trim(), f.trim()),
        None => (expr, ""),
    };

    // Evaluate the arithmetic expression (or plain path).
    let raw = evaluate_arithmetic(expr_part, value);

    // Apply filter if present.
    match filter {
        "date" | "datetime" => format_date(&raw),
        "" => raw,
        other => {
            log::warn!("Unknown template filter `{other}`");
            raw
        }
    }
}

/// Evaluate an expression that may contain arithmetic operations.
///
/// Supports a dotted path followed by zero or more `op number` pairs,
/// evaluated left-to-right. e.g. `invoice_payment.msat / 1000` or
/// `forward_event.in_msat / 1000 * 2`.
fn evaluate_arithmetic(expr: &str, value: &Value) -> String {
    let tokens: Vec<&str> = expr.split_whitespace().collect();
    if tokens.is_empty() {
        return String::new();
    }

    // First token is the path.
    let path = tokens[0];

    // If there are no operators, return the raw path value (preserving the
    // original string representation, e.g. "10000msat").
    if tokens.len() == 1 {
        return resolve_path(value, path);
    }

    // There are operators — resolve the path to a number for arithmetic.
    let mut current = match resolve_numeric(value, path) {
        Some(n) => n,
        None => return resolve_path(value, path),
    };

    // Remaining tokens come in pairs: operator, number.
    let mut i = 1;
    while i + 1 < tokens.len() {
        let op = tokens[i];
        let n: i64 = match tokens[i + 1].parse() {
            Ok(n) => n,
            Err(_) => {
                log::warn!(
                    "Template: cannot parse `{}` as number in expression `{expr}`",
                    tokens[i + 1]
                );
                return current.to_string();
            }
        };
        current = match op {
            "/" => {
                if n == 0 {
                    log::warn!("Template: division by zero in `{expr}`");
                    current
                } else {
                    current / n
                }
            }
            "*" => current * n,
            "+" => current + n,
            "-" => current - n,
            other => {
                log::warn!("Template: unknown operator `{other}` in `{expr}`");
                return current.to_string();
            }
        };
        i += 2;
    }

    current.to_string()
}

/// Resolve a dotted path to a numeric value (`i64`).
///
/// Handles both integer JSON values and string values that contain a number
/// (optionally with a `msat` suffix, which is stripped).
fn resolve_numeric(value: &Value, path: &str) -> Option<i64> {
    let mut current = value;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    match current {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => {
            let s = s.strip_suffix("msat").unwrap_or(s);
            s.parse().ok()
        }
        _ => None,
    }
}

/// Convert a Unix epoch timestamp string to a human-readable UTC datetime.
fn format_date(ts_str: &str) -> String {
    let ts_str = ts_str.trim();
    // Handle both integer and float timestamps.
    let secs: i64 = match ts_str.parse() {
        Ok(n) => n,
        Err(_) => {
            // Try parsing as float and truncating.
            match ts_str.parse::<f64>() {
                Ok(f) => f as i64,
                Err(_) => {
                    log::warn!("Template: cannot parse `{ts_str}` as Unix timestamp");
                    return ts_str.to_string();
                }
            }
        }
    };
    match chrono::DateTime::from_timestamp(secs, 0) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        None => {
            log::warn!("Template: timestamp {secs} out of range");
            ts_str.to_string()
        }
    }
}

/// Resolve a dotted path like `invoice_payment.msat` against a JSON value.
/// Returns the raw JSON representation of the leaf value (string without
/// quotes for string values, number as string, etc.).
fn resolve_path(value: &Value, path: &str) -> String {
    let mut current = value;
    for part in path.split('.') {
        match current.get(part) {
            Some(v) => current = v,
            None => return String::new(),
        }
    }
    match current {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_simple_template() {
        let content = r#"
# A comment
[default]
title = "CLN: {{event}}"
message = "{{json}}"

[invoice_payment]
title = "Payment received"
message = "+{{invoice_payment.msat}} msat"
tags = ["zap", "money"]
"#;
        let tmpl = Template::parse(content).unwrap();
        assert_eq!(tmpl.sections.len(), 2);
        let default = tmpl.sections.get("default").unwrap();
        assert_eq!(default.title.as_deref(), Some("CLN: {{event}}"));
        let pay = tmpl.sections.get("invoice_payment").unwrap();
        assert_eq!(pay.title.as_deref(), Some("Payment received"));
        assert_eq!(pay.tags, vec!["zap", "money"]);
    }

    #[test]
    fn render_with_substitution() {
        let content = r#"
[invoice_payment]
title = "Payment received"
message = "+{{invoice_payment.msat}} msat ({{invoice_payment.label}})"
tags = ["zap"]
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({
            "invoice_payment": {
                "msat": "10000msat",
                "label": "test-label"
            }
        });
        let rendered = tmpl
            .render(NotificationEvent::InvoicePayment, &value)
            .unwrap();
        assert_eq!(rendered.title.as_deref(), Some("Payment received"));
        assert_eq!(
            rendered.message.as_deref(),
            Some("+10000msat msat (test-label)")
        );
        assert_eq!(rendered.tags, vec!["zap"]);
    }

    #[test]
    fn render_falls_back_to_default_section() {
        let content = r#"
[default]
title = "CLN: {{event}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({});
        let rendered = tmpl.render(NotificationEvent::BlockAdded, &value).unwrap();
        assert_eq!(rendered.title.as_deref(), Some("CLN: block_added"));
    }

    #[test]
    fn render_returns_none_when_no_section() {
        let tmpl = Template {
            sections: HashMap::new(),
        };
        let value = serde_json::json!({});
        assert!(tmpl.render(NotificationEvent::BlockAdded, &value).is_none());
    }

    #[test]
    fn resolve_missing_path_is_empty() {
        let content = r#"
[invoice_payment]
message = "{{invoice_payment.nonexistent}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"invoice_payment": {"msat": "1000msat"}});
        let rendered = tmpl
            .render(NotificationEvent::InvoicePayment, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some(""));
    }

    #[test]
    fn resolve_integer_path() {
        let content = r#"
[block_added]
message = "Block #{{block_added.height}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"block_added": {"height": 800000}});
        let rendered = tmpl.render(NotificationEvent::BlockAdded, &value).unwrap();
        assert_eq!(rendered.message.as_deref(), Some("Block #800000"));
    }

    #[test]
    fn parse_error_on_bad_line() {
        let content = "not a valid line";
        assert!(Template::parse(content).is_err());
    }

    #[test]
    fn parse_error_on_unknown_key() {
        let content = r#"
[default]
unknown_key = "value"
"#;
        assert!(Template::parse(content).is_err());
    }

    #[test]
    fn arithmetic_division() {
        let content = r#"
[invoice_payment]
message = "+{{invoice_payment.msat / 1000}} sat"
"#;
        let tmpl = Template::parse(content).unwrap();
        // msat as string with "msat" suffix
        let value = serde_json::json!({"invoice_payment": {"msat": "10000msat"}});
        let rendered = tmpl
            .render(NotificationEvent::InvoicePayment, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some("+10 sat"));

        // msat as integer
        let value = serde_json::json!({"invoice_payment": {"msat": 25000}});
        let rendered = tmpl
            .render(NotificationEvent::InvoicePayment, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some("+25 sat"));
    }

    #[test]
    fn arithmetic_multiple_ops() {
        let content = r#"
[forward_event]
message = "{{forward_event.in_msat / 1000 * 2}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"forward_event": {"in_msat": "50000msat"}});
        let rendered = tmpl
            .render(NotificationEvent::ForwardEvent, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some("100"));
    }

    #[test]
    fn date_filter() {
        let content = r#"
[forward_event]
message = "{{forward_event.received_time | date}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"forward_event": {"received_time": 1691160000}});
        let rendered = tmpl
            .render(NotificationEvent::ForwardEvent, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some("2023-08-04 14:40:00 UTC"));
    }

    #[test]
    fn date_filter_with_float() {
        let content = r#"
[forward_event]
message = "{{forward_event.received_time | date}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"forward_event": {"received_time": 1691160000.5}});
        let rendered = tmpl
            .render(NotificationEvent::ForwardEvent, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some("2023-08-04 14:40:00 UTC"));
    }

    #[test]
    fn arithmetic_then_date_filter() {
        let content = r#"
[forward_event]
message = "{{forward_event.received_time / 1000 | date}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        // 1691160000000 ms -> /1000 -> 1691160000 -> 2023-08-04
        let value = serde_json::json!({"forward_event": {"received_time": 1691160000000i64}});
        let rendered = tmpl
            .render(NotificationEvent::ForwardEvent, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some("2023-08-04 14:40:00 UTC"));
    }

    #[test]
    fn division_by_zero_is_safe() {
        let content = r#"
[invoice_payment]
message = "{{invoice_payment.msat / 0}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"invoice_payment": {"msat": 10000}});
        let rendered = tmpl
            .render(NotificationEvent::InvoicePayment, &value)
            .unwrap();
        // Should return the original value, not crash.
        assert_eq!(rendered.message.as_deref(), Some("10000"));
    }

    #[test]
    fn multiline_message_with_escape() {
        let content = r#"
[forward_event]
title = "Forward {{forward_event.status}}"
message = "In: {{forward_event.in_msat / 1000}} sat\nOut: {{forward_event.out_msat / 1000}} sat\nFee: {{forward_event.fee_msat / 1000}} sat\nTime: {{forward_event.received_time | date}}"
tags = ["repeat"]
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({
            "forward_event": {
                "status": "settled",
                "in_msat": "50000msat",
                "out_msat": "49000msat",
                "fee_msat": "1000msat",
                "received_time": 1691160000
            }
        });
        let rendered = tmpl
            .render(NotificationEvent::ForwardEvent, &value)
            .unwrap();
        assert_eq!(rendered.title.as_deref(), Some("Forward settled"));
        assert_eq!(
            rendered.message.as_deref(),
            Some("In: 50 sat\nOut: 49 sat\nFee: 1 sat\nTime: 2023-08-04 14:40:00 UTC")
        );
    }

    #[test]
    fn unescape_tab_and_backslash() {
        assert_eq!(unescape("a\\tb\\nc"), "a\tb\nc");
        assert_eq!(unescape("path\\\\file"), "path\\file");
        assert_eq!(unescape("no escapes"), "no escapes");
        assert_eq!(unescape("trailing\\"), "trailing\\");
        assert_eq!(unescape("unknown\\x"), "unknown\\x");
    }

    #[test]
    fn unescape_in_message() {
        let content = r#"
[default]
message = "col1\tcol2\nline2"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({});
        let rendered = tmpl.render(NotificationEvent::BlockAdded, &value).unwrap();
        assert_eq!(rendered.message.as_deref(), Some("col1\tcol2\nline2"));
    }

    #[test]
    fn arithmetic_addition_and_subtraction() {
        let content = r#"
[default]
message = "{{default.x + 100}} and {{default.y - 50}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"default": {"x": 200, "y": 100}});
        let rendered = tmpl.render(NotificationEvent::BlockAdded, &value).unwrap();
        assert_eq!(rendered.message.as_deref(), Some("300 and 50"));
    }

    #[test]
    fn arithmetic_strips_msat_suffix() {
        let content = r#"
[invoice_payment]
message = "{{invoice_payment.msat / 1000}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"invoice_payment": {"msat": "123456msat"}});
        let rendered = tmpl
            .render(NotificationEvent::InvoicePayment, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some("123"));
    }

    #[test]
    fn date_filter_invalid_input() {
        // Non-numeric string passed to date filter should return the raw string.
        let content = r#"
[default]
message = "{{default.bad | date}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"default": {"bad": "not-a-timestamp"}});
        let rendered = tmpl.render(NotificationEvent::BlockAdded, &value).unwrap();
        assert_eq!(rendered.message.as_deref(), Some("not-a-timestamp"));
    }

    #[test]
    fn unknown_filter_returns_raw() {
        let content = r#"
[default]
message = "{{default.x | uppercase}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"default": {"x": 42}});
        let rendered = tmpl.render(NotificationEvent::BlockAdded, &value).unwrap();
        // Unknown filter falls back to the raw value.
        assert_eq!(rendered.message.as_deref(), Some("42"));
    }

    #[test]
    fn plain_path_preserves_msat_suffix() {
        // Without arithmetic, the raw string value (including "msat") is kept.
        let content = r#"
[invoice_payment]
message = "{{invoice_payment.msat}}"
"#;
        let tmpl = Template::parse(content).unwrap();
        let value = serde_json::json!({"invoice_payment": {"msat": "10000msat"}});
        let rendered = tmpl
            .render(NotificationEvent::InvoicePayment, &value)
            .unwrap();
        assert_eq!(rendered.message.as_deref(), Some("10000msat"));
    }

    #[test]
    fn template_load_from_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("cln_ntfy_test_template.conf");
        let content = "\n[default]\ntitle = \"Test\"\nmessage = \"Hello {{default.name}}\"\n";
        std::fs::write(&path, content).unwrap();

        let tmpl = Template::load(&path).unwrap();
        let value = serde_json::json!({"default": {"name": "world"}});
        let rendered = tmpl.render(NotificationEvent::BlockAdded, &value).unwrap();
        assert_eq!(rendered.title.as_deref(), Some("Test"));
        assert_eq!(rendered.message.as_deref(), Some("Hello world"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn template_load_missing_file_errors() {
        let path = std::path::Path::new("/nonexistent/path/template.conf");
        assert!(Template::load(path).is_err());
    }

    #[test]
    fn empty_template_renders_none() {
        let tmpl = Template::parse("").unwrap();
        let value = serde_json::json!({});
        assert!(tmpl.render(NotificationEvent::BlockAdded, &value).is_none());
    }
}
