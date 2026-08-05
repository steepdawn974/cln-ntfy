// Copyright (c) 2022-2026 Yuki Kishimoto
// Distributed under the MIT software license

use cln_plugin::{Error, Plugin};
use ntfy::Payload;
use serde_json::Value;

use crate::{NotificationEvent, PluginState, template::RenderResult, util::format};

/// Send a payload to ntfy, but never propagate send errors.
///
/// A dead/unreachable ntfy server must not kill the plugin or break CLN's
/// notification dispatch, so failures are logged and swallowed. The event
/// membership check happens here: if the user did not subscribe to `event`
/// via `ntfy-events`, the payload is silently dropped.
async fn send(
    p: &Plugin<PluginState>,
    event: NotificationEvent,
    payload: Payload,
) -> Result<(), Error> {
    if !p.state().events.contains(&event) {
        return Ok(());
    }
    if let Err(e) = p.state().dispatcher.send(&payload).await {
        log::warn!("Failed to send ntfy notification for {}: {e}", event.name());
    }
    Ok(())
}

/// Build a [`Payload`] either from the user's template (if configured for this
/// event) or from the built-in default. Returns `None` if a template filter
/// rejected the event (the notification should be dropped entirely).
fn build_payload(
    p: &Plugin<PluginState>,
    event: NotificationEvent,
    value: &Value,
    default_title: &str,
    default_message: Option<&str>,
    default_tags: &[&str],
) -> Option<Payload> {
    // Try the user template first.
    if let Some(ref tmpl) = p.state().template {
        match tmpl.render(event, value) {
            RenderResult::Render(rendered) => {
                let mut payload = Payload::new(&p.state().topic);
                if let Some(t) = rendered.title {
                    payload = payload.title(t);
                }
                if let Some(m) = rendered.message {
                    payload = payload.message(m);
                }
                if !rendered.tags.is_empty() {
                    payload = payload.tags(rendered.tags.iter().map(|s| s.as_str()));
                }
                return Some(payload);
            }
            RenderResult::Filtered => {
                log::debug!("Event {} filtered out by template filter", event.name());
                return None;
            }
            RenderResult::NoSection => {} // fall through to built-in default
        }
    }

    // Fall back to the built-in default.
    let mut payload = Payload::new(&p.state().topic).title(default_title);
    if let Some(msg) = default_message {
        payload = payload.message(msg);
    }
    payload = payload.tags(default_tags.iter().copied());
    Some(payload)
}

/// Extract an amount in satoshi from a notification payload.
///
/// CLN reports amounts under `value[key]["msat"]`. Depending on the CLN
/// version and notification, `msat` may be:
/// - a string ending in `msat` (e.g. `"21000msat"`) — older format
/// - an integer (e.g. `21000`) — newer format (see issue #10)
///
/// Returns the amount in whole satoshi (truncated, not rounded).
fn extract_amount(value: &Value, key: &str) -> Result<u64, Error> {
    let msat_val = &value[key]["msat"];
    match msat_val {
        Value::String(s) => {
            // Strip the trailing "msat" suffix and parse.
            let trimmed = s.strip_suffix("msat").unwrap_or(s);
            let amount: u64 = trimmed.parse()?;
            Ok(amount / 1000)
        }
        Value::Number(n) => {
            let amount: u64 = n
                .as_u64()
                .ok_or_else(|| cln_plugin::anyhow!("msat value is not a valid u64: {n}"))?;
            Ok(amount / 1000)
        }
        _ => Err(cln_plugin::anyhow!(
            "msat value has unexpected type: {msat_val}"
        )),
    }
}

pub async fn channel_opened_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a channel opened notification: {v}");
    let amount = extract_amount(&v, "channel_opened").ok();
    let msg = match amount {
        Some(a) => format!("Channel opened with +{} sat", format::number(a)),
        None => "Channel opened".to_string(),
    };
    let payload = build_payload(
        &p,
        NotificationEvent::ChannelOpened,
        &v,
        "New channel opened",
        Some(&msg),
        &["white_check_mark"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::ChannelOpened, payload).await
    } else {
        Ok(())
    }
}

pub async fn channel_state_changed_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a channel state changed notification: {v}");
    let old_state = v["channel_state_changed"]["old_state"]
        .as_str()
        .unwrap_or("unknown");
    let new_state = v["channel_state_changed"]["new_state"]
        .as_str()
        .unwrap_or("unknown");
    let msg = format!("{old_state} -> {new_state}");
    let payload = build_payload(
        &p,
        NotificationEvent::ChannelStateChanged,
        &v,
        "Channel state changed",
        Some(&msg),
        &["arrows_counterclockwise"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::ChannelStateChanged, payload).await
    } else {
        Ok(())
    }
}

pub async fn connect_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a connect notification: {v}");
    let peer = v["connect"]["id"].as_str().unwrap_or("unknown");
    let msg = format!("Peer connected: {peer}");
    let payload = build_payload(
        &p,
        NotificationEvent::Connect,
        &v,
        "Peer connected",
        Some(&msg),
        &["electric_plug"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::Connect, payload).await
    } else {
        Ok(())
    }
}

pub async fn disconnect_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a disconnect notification: {v}");
    let peer = v["disconnect"]["id"].as_str().unwrap_or("unknown");
    let msg = format!("Peer disconnected: {peer}");
    let payload = build_payload(
        &p,
        NotificationEvent::Disconnect,
        &v,
        "Peer disconnected",
        Some(&msg),
        &["x"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::Disconnect, payload).await
    } else {
        Ok(())
    }
}

pub async fn invoice_creation_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got an invoice creation notification: {v}");
    let msg = match extract_amount(&v, "invoice_creation") {
        Ok(amount) => format!("+{} sat", format::number(amount)),
        Err(_) => "Invoice created".to_string(),
    };
    let payload = build_payload(
        &p,
        NotificationEvent::InvoiceCreation,
        &v,
        "New invoice created",
        Some(&msg),
        &["hourglass"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::InvoiceCreation, payload).await
    } else {
        Ok(())
    }
}

pub async fn invoice_payment_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got an invoice payment notification: {v}");
    let msg = match extract_amount(&v, "invoice_payment") {
        Ok(amount) => format!("+{} sat", format::number(amount)),
        Err(_) => "Payment received".to_string(),
    };
    let payload = build_payload(
        &p,
        NotificationEvent::InvoicePayment,
        &v,
        "New payment received",
        Some(&msg),
        &["zap"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::InvoicePayment, payload).await
    } else {
        Ok(())
    }
}

pub async fn forward_event_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a forward event notification: {v}");
    let fwd = &v["forward_event"];
    let status = fwd["status"].as_str().unwrap_or("unknown");
    let in_amount = extract_amount(&v, "forward_event").ok();
    let fee = fwd["fee_msat"]
        .as_str()
        .and_then(|s| s.strip_suffix("msat"))
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| fwd["fee_msat"].as_u64())
        .map(|f| f / 1000);

    let msg = match (status, in_amount, fee) {
        ("settled", Some(amt), Some(fee)) => {
            format!(
                "Forward settled: +{} sat in, +{} sat fee",
                format::number(amt),
                format::number(fee)
            )
        }
        ("settled", Some(amt), None) => {
            format!("Forward settled: +{} sat in", format::number(amt))
        }
        (_, Some(amt), _) => {
            format!("Forward {status}: +{} sat in", format::number(amt))
        }
        _ => format!("Forward {status}"),
    };
    let payload = build_payload(
        &p,
        NotificationEvent::ForwardEvent,
        &v,
        "Forward event",
        Some(&msg),
        &["repeat"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::ForwardEvent, payload).await
    } else {
        Ok(())
    }
}

pub async fn sendpay_success_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a sendpay success notification: {v}");
    let payload = build_payload(
        &p,
        NotificationEvent::SendpaySuccess,
        &v,
        "Payment sent",
        None,
        &["outbox_tray"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::SendpaySuccess, payload).await
    } else {
        Ok(())
    }
}

pub async fn sendpay_failure_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a sendpay failure notification: {v}");
    let payload = build_payload(
        &p,
        NotificationEvent::SendpayFailure,
        &v,
        "Payment failed",
        None,
        &["warning"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::SendpayFailure, payload).await
    } else {
        Ok(())
    }
}

pub async fn coin_movement_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a coin movement notification: {v}");
    let payload = build_payload(
        &p,
        NotificationEvent::CoinMovement,
        &v,
        "Coin movement",
        None,
        &["coin"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::CoinMovement, payload).await
    } else {
        Ok(())
    }
}

pub async fn warning_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a warning notification: {v}");
    let msg = v["warning"]["message"].as_str().map(|s| s.to_string());
    let payload = build_payload(
        &p,
        NotificationEvent::Warning,
        &v,
        "Warning",
        msg.as_deref(),
        &["warning"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::Warning, payload).await
    } else {
        Ok(())
    }
}

pub async fn block_added_handler(p: Plugin<PluginState>, v: Value) -> Result<(), Error> {
    log::debug!("Got a block added notification: {v}");
    let height = v["block_added"]["height"].as_u64();
    let msg = height.map(|h| format!("Block #{h}"));
    let payload = build_payload(
        &p,
        NotificationEvent::BlockAdded,
        &v,
        "New block added",
        msg.as_deref(),
        &["bricks"],
    );
    if let Some(payload) = payload {
        send(&p, NotificationEvent::BlockAdded, payload).await
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn extract_amount_string_msat() {
        let v = serde_json::json!({"invoice_payment": {"msat": "21000msat"}});
        assert_eq!(extract_amount(&v, "invoice_payment").unwrap(), 21);
    }

    #[test]
    fn extract_amount_integer_msat() {
        // Issue #10: newer CLN sends msat as an integer, not a string.
        let v = serde_json::json!({"invoice_payment": {"msat": 21000}});
        assert_eq!(extract_amount(&v, "invoice_payment").unwrap(), 21);
    }

    #[test]
    fn extract_amount_large_integer() {
        let v = serde_json::json!({"invoice_payment": {"msat": 1000000000}});
        assert_eq!(extract_amount(&v, "invoice_payment").unwrap(), 1000000);
    }

    #[test]
    fn extract_amount_truncates_remainder() {
        // 10500 msat = 10 sat (truncated, not rounded).
        let v = serde_json::json!({"invoice_payment": {"msat": "10500msat"}});
        assert_eq!(extract_amount(&v, "invoice_payment").unwrap(), 10);
    }

    #[test]
    fn extract_amount_null_errors() {
        let v = serde_json::json!({"invoice_payment": {"msat": null}});
        assert!(extract_amount(&v, "invoice_payment").is_err());
    }

    #[test]
    fn extract_amount_missing_key_errors() {
        let v = serde_json::json!({"invoice_payment": {}});
        assert!(extract_amount(&v, "invoice_payment").is_err());
    }
}
