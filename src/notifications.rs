// Copyright (c) 2022-2026 Yuki Kishimoto
// Distributed under the MIT software license

use cln_plugin::{Error, Plugin};
use ntfy::Payload;

use crate::{NotificationEvent, PluginState, util::format};

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

/// Extract an amount in satoshi from a notification payload.
///
/// CLN reports amounts as a string ending in `msat` (e.g. `"21000msat"`).
/// `key` is the top-level notification field holding the `msat` value.
fn extract_amount(value: &serde_json::Value, key: &str) -> Result<u64, Error> {
    let amount_msat = &value[key]["msat"];
    let amount_str: String = serde_json::from_value(amount_msat.clone())?;
    let amount: usize = amount_str[..amount_str.len() - 4].parse()?;
    Ok((amount / 1000) as u64)
}

pub async fn channel_opened_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got a channel opened notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("New channel opened")
        .tags(["white_check_mark"]);
    send(&p, NotificationEvent::ChannelOpened, payload).await
}

pub async fn channel_state_changed_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got a channel state changed notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("Channel state changed")
        .tags(["arrows_counterclockwise"]);
    send(&p, NotificationEvent::ChannelStateChanged, payload).await
}

pub async fn connect_handler(p: Plugin<PluginState>, v: serde_json::Value) -> Result<(), Error> {
    log::debug!("Got a connect notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("Peer connected")
        .tags(["electric_plug"]);
    send(&p, NotificationEvent::Connect, payload).await
}

pub async fn disconnect_handler(p: Plugin<PluginState>, v: serde_json::Value) -> Result<(), Error> {
    log::debug!("Got a disconnect notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("Peer disconnected")
        .tags(["x"]);
    send(&p, NotificationEvent::Disconnect, payload).await
}

pub async fn invoice_creation_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got an invoice creation notification: {v}");
    let payload = match extract_amount(&v, "invoice_creation") {
        Ok(amount) => Payload::new(&p.state().topic)
            .message(format!("+{} sat", format::number(amount)))
            .title("New invoice created")
            .tags(["hourglass"]),
        Err(e) => {
            log::warn!("Could not extract amount from invoice_creation: {e}");
            Payload::new(&p.state().topic)
                .title("New invoice created")
                .tags(["hourglass"])
        }
    };
    send(&p, NotificationEvent::InvoiceCreation, payload).await
}

pub async fn invoice_payment_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got an invoice payment notification: {v}");
    let payload = match extract_amount(&v, "invoice_payment") {
        Ok(amount) => Payload::new(&p.state().topic)
            .message(format!("+{} sat", format::number(amount)))
            .title("New payment received")
            .tags(["zap"]),
        Err(e) => {
            log::warn!("Could not extract amount from invoice_payment: {e}");
            Payload::new(&p.state().topic)
                .title("New payment received")
                .tags(["zap"])
        }
    };
    send(&p, NotificationEvent::InvoicePayment, payload).await
}

pub async fn forward_event_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got a forward event notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("Forward event")
        .tags(["repeat"]);
    send(&p, NotificationEvent::ForwardEvent, payload).await
}

pub async fn sendpay_success_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got a sendpay success notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("Payment sent")
        .tags(["outbox_tray"]);
    send(&p, NotificationEvent::SendpaySuccess, payload).await
}

pub async fn sendpay_failure_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got a sendpay failure notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("Payment failed")
        .tags(["warning"]);
    send(&p, NotificationEvent::SendpayFailure, payload).await
}

pub async fn coin_movement_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got a coin movement notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("Coin movement")
        .tags(["coin"]);
    send(&p, NotificationEvent::CoinMovement, payload).await
}

pub async fn warning_handler(p: Plugin<PluginState>, v: serde_json::Value) -> Result<(), Error> {
    log::debug!("Got a warning notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("Warning")
        .tags(["warning"]);
    send(&p, NotificationEvent::Warning, payload).await
}

pub async fn block_added_handler(
    p: Plugin<PluginState>,
    v: serde_json::Value,
) -> Result<(), Error> {
    log::debug!("Got a block added notification: {v}");
    let payload = Payload::new(&p.state().topic)
        .title("New block added")
        .tags(["bricks"]);
    send(&p, NotificationEvent::BlockAdded, payload).await
}
