// Copyright (c) 2022-2026 Yuki Kishimoto
// Distributed under the MIT software license

use cln_plugin::Builder;
use cln_plugin::options::{ConfigOption, DefaultStringConfigOption, StringConfigOption};
use notifications::{
    block_added_handler, channel_opened_handler, channel_state_changed_handler,
    coin_movement_handler, connect_handler, disconnect_handler, forward_event_handler,
    invoice_creation_handler, invoice_payment_handler, sendpay_failure_handler,
    sendpay_success_handler, warning_handler,
};
use parse::get_startup_options;
use structs::{NotificationEvent, PluginState};
use tokio::io::{stdin, stdout};

mod notifications;
mod parse;
mod structs;
mod util;

const OPT_NTFY_URL: StringConfigOption =
    ConfigOption::new_str_no_default("ntfy-url", "ntfy base URL, e.g. https://ntfy.sh (required)");
const OPT_NTFY_TOPIC: DefaultStringConfigOption = ConfigOption::new_str_with_default(
    "ntfy-topic",
    "cln-alerts",
    "ntfy topic to publish notifications to",
);
const OPT_NTFY_USERNAME: StringConfigOption =
    ConfigOption::new_str_no_default("ntfy-username", "ntfy username (basic auth)");
const OPT_NTFY_PASSWORD: StringConfigOption =
    ConfigOption::new_str_no_default("ntfy-password", "ntfy password (basic auth)");
const OPT_NTFY_TOKEN: StringConfigOption = ConfigOption::new_str_no_default(
    "ntfy-token",
    "nty access token (mutually exclusive with username/password)",
);
const OPT_NTFY_PROXY: StringConfigOption = ConfigOption::new_str_no_default(
    "ntfy-proxy",
    "ntfy proxy URL, e.g. socks5h://127.0.0.1:9050 (needed for .onion URLs)",
);
const OPT_NTFY_EVENTS: DefaultStringConfigOption = ConfigOption::new_str_with_default(
    "ntfy-events",
    "all",
    "comma-separated list of CLN events to forward, or `all`",
);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    unsafe {
        std::env::set_var("CLN_PLUGIN_LOG", "cln_plugin=info,cln_ntfy=debug,info");
    };

    let Some(configured_plugin) = Builder::new(stdin(), stdout())
        .option(OPT_NTFY_URL)
        .option(OPT_NTFY_TOPIC)
        .option(OPT_NTFY_USERNAME)
        .option(OPT_NTFY_PASSWORD)
        .option(OPT_NTFY_TOKEN)
        .option(OPT_NTFY_PROXY)
        .option(OPT_NTFY_EVENTS)
        // Subscribe to every supported event up front; the per-event filter
        // against `ntfy-events` happens in the dispatch handlers. This works
        // with `.dynamic()` since subscriptions must be registered before the
        // options are known.
        .subscribe("channel_opened", channel_opened_handler)
        .subscribe("channel_state_changed", channel_state_changed_handler)
        .subscribe("connect", connect_handler)
        .subscribe("disconnect", disconnect_handler)
        .subscribe("invoice_creation", invoice_creation_handler)
        .subscribe("invoice_payment", invoice_payment_handler)
        .subscribe("forward_event", forward_event_handler)
        .subscribe("sendpay_success", sendpay_success_handler)
        .subscribe("sendpay_failure", sendpay_failure_handler)
        .subscribe("coin_movement", coin_movement_handler)
        .subscribe("warning", warning_handler)
        .subscribe("block_added", block_added_handler)
        .dynamic()
        .configure()
        .await?
    else {
        return Ok(());
    };

    let state = match get_startup_options(&configured_plugin) {
        Ok(s) => s,
        Err(e) => {
            return configured_plugin
                .disable(&format!("Error parsing ntfy options: {e}"))
                .await;
        }
    };

    log::info!(
        "Starting cln-ntfy. URL forwarding {} event(s) to topic `{}`.",
        state.events.len(),
        state.topic
    );

    let plugin = configured_plugin.start(state).await?;
    plugin.join().await
}
