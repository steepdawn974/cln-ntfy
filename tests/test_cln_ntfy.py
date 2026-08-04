#!/usr/bin/python

import logging
import time

import pytest
from pyln.testing.fixtures import *  # noqa: F403
from pyln.testing.utils import wait_for, TIMEOUT  # noqa: F401
from util import get_plugin  # noqa: F401

LOGGER = logging.getLogger(__name__)


def _start_node(node_factory, get_plugin, mock_ntfy, extra=None):
    opts = {
        "plugin": get_plugin,
        "ntfy-url": mock_ntfy.url,
        "ntfy-topic": "cln-alerts",
    }
    if extra:
        opts.update(extra)
    return node_factory.get_node(options=opts)


def test_startup_validates_options(node_factory, get_plugin):
    """A missing required ntfy-url should disable the plugin, not panic."""
    node = node_factory.get_node(
        options={"plugin": get_plugin, "ntfy-topic": "cln-alerts"},
        allow_broken=True,
    )
    # The plugin should have logged a disable message about ntfy-url.
    wait_for(
        lambda: node.daemon.is_in_log("ntfy-url"),
        timeout=TIMEOUT,
    )


def test_invoice_payment_payload(node_factory, get_plugin, mock_ntfy):
    """An invoice payment produces a payload with the right title/tags."""
    l1, l2 = node_factory.line_graph(
        2,
        opts=[
            {},
            {
                "plugin": get_plugin,
                "ntfy-url": mock_ntfy.url,
                "ntfy-topic": "cln-alerts",
                "ntfy-events": "invoice_payment",
            },
        ],
    )
    wait_for(lambda: l2.daemon.is_in_log("Starting cln-ntfy."))

    mock_ntfy.reset()
    inv = l2.rpc.invoice(10000, "lbl", "desc")
    l1.rpc.pay(inv["bolt11"])
    wait_for(lambda: len(mock_ntfy.received) >= 1, timeout=TIMEOUT)

    req = mock_ntfy.received[-1]
    assert req["path"] == "/cln-alerts"
    # The body is JSON published by the ntfy crate.
    import json

    payload = json.loads(req["body"])
    assert payload["title"] == "New payment received"
    assert "zap" in payload["tags"]
    assert "10 sat" in payload["message"]


def test_event_filter_suppresses_unselected(node_factory, get_plugin, mock_ntfy):
    """invoice_creation should be suppressed when not in ntfy-events."""
    l1, l2 = node_factory.line_graph(
        2,
        opts=[
            {},
            {
                "plugin": get_plugin,
                "ntfy-url": mock_ntfy.url,
                "ntfy-topic": "cln-alerts",
                "ntfy-events": "invoice_payment",
            },
        ],
    )
    wait_for(lambda: l2.daemon.is_in_log("Starting cln-ntfy."))

    mock_ntfy.reset()
    inv = l2.rpc.invoice(10000, "lbl2", "desc")
    l1.rpc.pay(inv["bolt11"])
    # Wait for the payment notification (the only one we expect).
    wait_for(lambda: len(mock_ntfy.received) >= 1, timeout=TIMEOUT)

    # Give a little extra time for any suppressed creation event to arrive.
    time.sleep(2)
    titles = []
    for req in mock_ntfy.received:
        import json

        payload = json.loads(req["body"])
        titles.append(payload.get("title"))
    assert "New invoice created" not in titles
    assert "New payment received" in titles


def test_token_auth_header(node_factory, get_plugin, mock_ntfy):
    """ntfy-token results in a Bearer Authorization header."""
    node = _start_node(
        node_factory,
        get_plugin,
        mock_ntfy,
        extra={"ntfy-token": "tk_testtoken123", "ntfy-events": "invoice_payment"},
    )
    wait_for(lambda: node.daemon.is_in_log("Starting cln-ntfy."))

    mock_ntfy.reset()
    inv = node.rpc.invoice(10000, "lbl3", "desc")
    # No peer to pay, but we can still trigger an invoice_creation event by
    # simply creating the invoice — except we filtered to invoice_payment.
    # Instead, force a payment by adding a second node.
    l1 = node_factory.get_node()
    node_factory.join_nodes([l1, node], fundchannel=False)
    l1.rpc.pay(inv["bolt11"])
    wait_for(lambda: len(mock_ntfy.received) >= 1, timeout=TIMEOUT)

    req = mock_ntfy.received[-1]
    auth = req["headers"].get("authorization", "")
    assert auth == "Bearer tk_testtoken123"


def test_basic_auth_header(node_factory, get_plugin, mock_ntfy):
    """ntfy-username/ntfy-password results in a Basic Authorization header."""
    import base64

    l1, l2 = node_factory.line_graph(
        2,
        opts=[
            {},
            {
                "plugin": get_plugin,
                "ntfy-url": mock_ntfy.url,
                "ntfy-topic": "cln-alerts",
                "ntfy-username": "alice",
                "ntfy-password": "secret",
                "ntfy-events": "invoice_payment",
            },
        ],
    )
    wait_for(lambda: l2.daemon.is_in_log("Starting cln-ntfy."))

    mock_ntfy.reset()
    inv = l2.rpc.invoice(10000, "lbl4", "desc")
    l1.rpc.pay(inv["bolt11"])
    wait_for(lambda: len(mock_ntfy.received) >= 1, timeout=TIMEOUT)

    req = mock_ntfy.received[-1]
    auth = req["headers"].get("authorization", "")
    expected = "Basic " + base64.b64encode(b"alice:secret").decode()
    assert auth == expected


def test_unknown_event_disables_plugin(node_factory, get_plugin, mock_ntfy):
    """An unknown ntfy-events value disables the plugin with a clear error."""
    node = node_factory.get_node(
        options={
            "plugin": get_plugin,
            "ntfy-url": mock_ntfy.url,
            "ntfy-topic": "cln-alerts",
            "ntfy-events": "bogus_event",
        },
        allow_broken=True,
    )
    wait_for(
        lambda: node.daemon.is_in_log("Unknown event") or node.daemon.is_in_log("bogus_event"),
        timeout=TIMEOUT,
    )
