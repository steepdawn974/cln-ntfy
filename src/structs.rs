// Copyright (c) 2022-2026 Yuki Kishimoto
// Distributed under the MIT software license

use std::collections::HashSet;

use ntfy::Dispatcher;
use ntfy::dispatcher::Async;

use crate::template::Template;

/// CLN notification events that can be forwarded to ntfy.
///
/// The variant names match the notification names used by Core Lightning
/// (see https://docs.corelightning.org/docs/notification). They are also the
/// accepted values for the `ntfy-events` option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationEvent {
    ChannelOpened,
    ChannelStateChanged,
    Connect,
    Disconnect,
    InvoiceCreation,
    InvoicePayment,
    ForwardEvent,
    SendpaySuccess,
    SendpayFailure,
    CoinMovement,
    Warning,
    BlockAdded,
}

impl NotificationEvent {
    /// All events supported by this plugin, in a stable order.
    pub const ALL: &'static [NotificationEvent] = &[
        NotificationEvent::ChannelOpened,
        NotificationEvent::ChannelStateChanged,
        NotificationEvent::Connect,
        NotificationEvent::Disconnect,
        NotificationEvent::InvoiceCreation,
        NotificationEvent::InvoicePayment,
        NotificationEvent::ForwardEvent,
        NotificationEvent::SendpaySuccess,
        NotificationEvent::SendpayFailure,
        NotificationEvent::CoinMovement,
        NotificationEvent::Warning,
        NotificationEvent::BlockAdded,
    ];

    /// The notification name Core Lightning uses for this event.
    pub fn name(self) -> &'static str {
        match self {
            NotificationEvent::ChannelOpened => "channel_opened",
            NotificationEvent::ChannelStateChanged => "channel_state_changed",
            NotificationEvent::Connect => "connect",
            NotificationEvent::Disconnect => "disconnect",
            NotificationEvent::InvoiceCreation => "invoice_creation",
            NotificationEvent::InvoicePayment => "invoice_payment",
            NotificationEvent::ForwardEvent => "forward_event",
            NotificationEvent::SendpaySuccess => "sendpay_success",
            NotificationEvent::SendpayFailure => "sendpay_failure",
            NotificationEvent::CoinMovement => "coin_movement",
            NotificationEvent::Warning => "warning",
            NotificationEvent::BlockAdded => "block_added",
        }
    }

    /// Parse a single event name (case-sensitive, trimmed) into a variant.
    pub fn from_name(s: &str) -> Option<NotificationEvent> {
        Self::ALL.iter().copied().find(|e| e.name() == s.trim())
    }

    /// Comma-separated list of all supported event names, for error messages.
    pub fn all_names() -> Vec<&'static str> {
        Self::ALL.iter().map(|e| e.name()).collect()
    }
}

#[derive(Debug, Clone)]
pub struct PluginState {
    pub dispatcher: Dispatcher<Async>,
    pub topic: String,
    pub events: HashSet<NotificationEvent>,
    pub template: Option<Template>,
}
