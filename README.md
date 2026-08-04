# cln-ntfy

A [Core Lightning](https://github.com/ElementsProject/lightning) plugin that forwards node notifications to a [ntfy](https://ntfy.sh) topic — your own self-hosted server, the public `ntfy.sh`, or a `.onion` endpoint.

## Features

* Forward CLN notification events to any ntfy-compatible server (custom domain, self-hosted, or `.onion` via proxy).
* Pick exactly which CLN events to forward with `ntfy-events`.
* Authenticate with **basic auth** (username + password) **or** an **access token**.
* Graceful failure: if the ntfy server is unreachable the plugin logs a warning and keeps running — it never breaks CLN's notification dispatch.
* Validated startup: bad URLs, partial credentials, or unknown event names disable the plugin with a clear error instead of panicking.

## Installation

For general plugin installation instructions see the plugins repo [README.md](https://github.com/lightningd/plugins/blob/master/README.md#Installation).

### Prebuilt binaries

Release binaries for `x86_64-linux`, `armv7-linux` and `aarch64-linux` can be found on the [release](https://github.com/yukibtc/cln-ntfy/releases) page. They require `glibc>=2.31` (`ldd --version`).

### Building from source

```
git clone https://github.com/yukibtc/cln-ntfy.git
cd cln-ntfy
cargo build --release
```

The binary will be at `target/release/cln-ntfy`.

Release binaries are built with the `optimized` profile:

```
cargo build --release --profile optimized
```

## Configuration

Add the plugin to your `~/.lightning/config` (or pass the options on the command line):

```
plugin=/path/to/cln-ntfy
ntfy-url=https://ntfy.sh
```

### Options

| Option | Default | Description |
| --- | --- | --- |
| `ntfy-url` | _(none, required)_ | ntfy base URL, e.g. `https://ntfy.sh` or `https://ntfy.example.com`. Any ntfy-compatible server works. |
| `ntfy-topic` | `cln-alerts` | ntfy topic to publish all notifications to. Must be non-empty. |
| `ntfy-username` | _(none)_ | Username for basic auth. Must be set together with `ntfy-password`. |
| `ntfy-password` | _(none)_ | Password for basic auth. Must be set together with `ntfy-username`. |
| `ntfy-token` | _(none)_ | Access token for token auth. Mutually exclusive with `ntfy-username`/`ntfy-password`. |
| `ntfy-proxy` | _(none)_ | Proxy URL, e.g. `socks5h://127.0.0.1:9050`. Required to reach a `.onion` ntfy URL. |
| `ntfy-events` | `all` | Comma-separated list of CLN events to forward, `all`, or `none` (publishes nothing). See the table below. |

### Supported events

| Event name | Description |
| --- | --- |
| `channel_opened` | A new channel was opened. |
| `channel_state_changed` | A channel changed state. |
| `connect` | A peer connected. |
| `disconnect` | A peer disconnected. |
| `invoice_creation` | An invoice was created. |
| `invoice_payment` | An invoice was paid (payment received). |
| `forward_event` | A forwarding event occurred. |
| `sendpay_success` | A payment was sent successfully. |
| `sendpay_failure` | A payment send failed. |
| `coin_movement` | A coin movement was recorded. |
| `warning` | A warning was emitted. |
| `block_added` | A new block was added. |

Example — only forward payments and channel opens:

```
ntfy-events=invoice_payment,channel_opened
```

Disable all forwarding (the plugin stays loaded but sends nothing):

```
ntfy-events=none
```

### Auth examples

Basic auth:

```
ntfy-username=alice
ntfy-password=secret
```

Access token (see the [ntfy docs](https://docs.ntfy.sh/publish/#access-tokens)):

```
ntfy-token=tk_abc123...
```

Do **not** set both basic-auth and token options — the plugin will refuse to start.

### Self-hosted / custom domain

```
ntfy-url=https://ntfy.example.com
ntfy-topic=my-node-alerts
```

### `.onion` endpoint

```
ntfy-url=http://ntfyxyzexample.onion
ntfy-topic=my-node-alerts
ntfy-proxy=socks5h://127.0.0.1:9050
```

## Behavior on misconfiguration

If a required option is missing, credentials are only half-set, the URL is invalid, or `ntfy-events` contains an unknown name, the plugin disables itself at startup and logs a descriptive error via `lightningd`. It does **not** panic. Fix the option and restart `lightningd`.

## Building

```
make            # cargo build --release
make precommit  # cargo fmt && cargo clippy
```

## Tests

Unit tests:

```
cargo test
```

Integration tests (require `lightningd` in `PATH` and a working Python/pyln environment):

```
pytest tests/
```

## License

This project is distributed under the MIT software license — see the [LICENSE](LICENSE) file for details.
