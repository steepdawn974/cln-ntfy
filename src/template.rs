// Copyright (c) 2022-2026 Yuki Kishimoto
// Distributed under the MIT software license

use std::collections::HashMap;
use std::fs;
use std::path::Path;

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
fn parse_string_val(val: &str, line_no: usize) -> Result<String, anyhow::Error> {
    let val = val.trim();
    if val.len() >= 2 && val.starts_with('"') && val.ends_with('"') {
        Ok(val[1..val.len() - 1].to_string())
    } else {
        anyhow::bail!(
            "Line {}: expected a quoted string like \"value\", got: {val}",
            line_no + 1
        );
    }
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
fn substitute(tmpl: &str, value: &Value, event_name: &str, json: &str) -> String {
    let mut result = String::with_capacity(tmpl.len());
    let mut rest = tmpl;

    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        if let Some(end) = after_start.find("}}") {
            let key = after_start[..end].trim();
            let replacement = match key {
                "event" => event_name.to_string(),
                "json" => json.to_string(),
                _ => resolve_path(value, key),
            };
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
}
