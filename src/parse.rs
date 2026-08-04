// Copyright (c) 2022-2026 Yuki Kishimoto
// Distributed under the MIT software license

use std::collections::HashSet;

use anyhow::anyhow;
use cln_plugin::ConfiguredPlugin;
use ntfy::{Auth, dispatcher::builder::DispatcherBuilder};
use url::Url;

use crate::{
    NotificationEvent, OPT_NTFY_EVENTS, OPT_NTFY_PASSWORD, OPT_NTFY_PROXY, OPT_NTFY_TOKEN,
    OPT_NTFY_TOPIC, OPT_NTFY_URL, OPT_NTFY_USERNAME, PluginState,
};

/// Parse and validate all startup options, building the [`PluginState`].
///
/// Any validation failure is returned as an error so the caller can
/// `disable()` the plugin with a helpful message instead of panicking.
pub fn get_startup_options(
    plugin: &ConfiguredPlugin<PluginState, tokio::io::Stdin, tokio::io::Stdout>,
) -> Result<PluginState, anyhow::Error> {
    // -- ntfy-url (required) ---------------------------------------------------
    let url_str: Option<String> = plugin.option(&OPT_NTFY_URL)?;
    let mut url_str = match url_str {
        Some(s) if !s.is_empty() => s,
        _ => {
            return Err(anyhow!(
                "`{}` is required, please specify the ntfy base URL (e.g. https://ntfy.sh).",
                OPT_NTFY_URL.name()
            ));
        }
    };
    // Normalize so the base always ends with a trailing slash, matching how
    // the ntfy crate joins the topic path.
    if !url_str.ends_with('/') {
        url_str.push('/');
    }
    let url: Url = url_str.parse()?;
    if !url.has_host() {
        return Err(anyhow!("Invalid ntfy URL! Missing host part: {url}"));
    }

    // -- ntfy-topic (default "cln-alerts") -------------------------------------
    let topic: String = plugin.option(&OPT_NTFY_TOPIC)?;
    if topic.is_empty() {
        return Err(anyhow!("`{}` must not be empty.", OPT_NTFY_TOPIC.name()));
    }

    // -- auth: basic vs token, mutually exclusive ------------------------------
    let username: Option<String> = plugin.option(&OPT_NTFY_USERNAME)?;
    let password: Option<String> = plugin.option(&OPT_NTFY_PASSWORD)?;
    let token: Option<String> = plugin.option(&OPT_NTFY_TOKEN)?;

    let has_basic = username.is_some() || password.is_some();
    let has_token = token.is_some();

    if has_basic && has_token {
        return Err(anyhow!(
            "`{}`/`{}` and `{}` are mutually exclusive. Use either basic auth or an access token, not both.",
            OPT_NTFY_USERNAME.name(),
            OPT_NTFY_PASSWORD.name(),
            OPT_NTFY_TOKEN.name()
        ));
    }

    let auth: Option<Auth> = if has_basic {
        match (username, password) {
            (Some(u), Some(p)) => Some(Auth::credentials(u, p)),
            (Some(_), None) => {
                return Err(anyhow!(
                    "`{}` is set but `{}` is missing. Provide both or neither.",
                    OPT_NTFY_USERNAME.name(),
                    OPT_NTFY_PASSWORD.name()
                ));
            }
            (None, Some(_)) => {
                return Err(anyhow!(
                    "`{}` is set but `{}` is missing. Provide both or neither.",
                    OPT_NTFY_PASSWORD.name(),
                    OPT_NTFY_USERNAME.name()
                ));
            }
            (None, None) => None,
        }
    } else {
        token.map(Auth::token)
    };

    // -- ntfy-proxy (optional) -------------------------------------------------
    let proxy: Option<String> = plugin.option(&OPT_NTFY_PROXY)?;

    // -- ntfy-events (default "all") -------------------------------------------
    let events_str: String = plugin.option(&OPT_NTFY_EVENTS)?;
    let events: HashSet<NotificationEvent> = parse_events(&events_str)?;

    // -- build dispatcher ------------------------------------------------------
    let mut builder = DispatcherBuilder::new(url.to_string());
    if let Some(auth) = auth {
        builder = builder.credentials(auth);
    }
    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy);
    }
    let dispatcher = builder.build_async()?;

    Ok(PluginState {
        dispatcher,
        topic,
        events,
    })
}

/// Parse the comma-separated `ntfy-events` value into a set of events.
///
/// `"all"` (case-insensitive) selects every supported event. Unknown names
/// produce an error listing the valid options.
pub fn parse_events(s: &str) -> Result<HashSet<NotificationEvent>, anyhow::Error> {
    let trimmed = s.trim();
    if trimmed.eq_ignore_ascii_case("all") {
        return Ok(NotificationEvent::ALL.iter().copied().collect());
    }
    // `none` explicitly disables all forwarding. This is the safe default
    // used by the install script so that no node data is published until the
    // user opts in by setting a real event list.
    if trimmed.eq_ignore_ascii_case("none") {
        return Ok(HashSet::new());
    }

    let mut set = HashSet::new();
    for part in trimmed.split(',') {
        let name = part.trim();
        if name.is_empty() {
            continue;
        }
        match NotificationEvent::from_name(name) {
            Some(e) => {
                set.insert(e);
            }
            None => {
                return Err(anyhow!(
                    "Unknown event `{name}` in `{}`. Valid events are: {}, `all` or `none`.",
                    OPT_NTFY_EVENTS.name(),
                    NotificationEvent::all_names().join(", ")
                ));
            }
        }
    }

    if set.is_empty() {
        return Err(anyhow!(
            "`{}` is empty. Use `all`, `none`, or a comma-separated list of events: {}",
            OPT_NTFY_EVENTS.name(),
            NotificationEvent::all_names().join(", ")
        ));
    }

    Ok(set)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_all_events() {
        let set = parse_events("all").unwrap();
        assert_eq!(set.len(), NotificationEvent::ALL.len());
    }

    #[test]
    fn parse_all_events_case_insensitive() {
        let set = parse_events("ALL").unwrap();
        assert_eq!(set.len(), NotificationEvent::ALL.len());
    }

    #[test]
    fn parse_subset_events() {
        let set = parse_events("invoice_payment, invoice_creation , channel_opened").unwrap();
        assert_eq!(set.len(), 3);
        assert!(set.contains(&NotificationEvent::InvoicePayment));
        assert!(set.contains(&NotificationEvent::InvoiceCreation));
        assert!(set.contains(&NotificationEvent::ChannelOpened));
    }

    #[test]
    fn parse_none_events() {
        let set = parse_events("none").unwrap();
        assert!(set.is_empty());
        // Case-insensitive.
        assert!(parse_events("NONE").unwrap().is_empty());
    }

    #[test]
    fn parse_unknown_event_errors() {
        let err = parse_events("invoice_payment,bogus").unwrap_err();
        assert!(err.to_string().contains("bogus"));
        assert!(err.to_string().contains("invoice_creation"));
    }

    #[test]
    fn parse_empty_errors() {
        // Empty / whitespace-only / stray commas are NOT the same as `none`
        // and must error so a misconfigured value is loud, not silent.
        assert!(parse_events("").is_err());
        assert!(parse_events("   ").is_err());
        assert!(parse_events(",,,").is_err());
    }

    #[test]
    fn event_roundtrip_names() {
        for e in NotificationEvent::ALL {
            assert_eq!(NotificationEvent::from_name(e.name()), Some(*e));
        }
    }
}
