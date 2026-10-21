# Reproduction Workbench

## Baseline and acceptance

The unchanged `748acae` checkout compiled and passed all 17 existing tests on
Windows x64 with Rust 1.99.0 (`cargo test --locked --all-targets -j 1`).
It already provides HTTP/HTTPS interception, JSONL recording, TUI, request replay,
rules and Wasm hooks. Missing capabilities were ordered business scenarios,
semantic assertions, failure reports and a browser workbench.

Capture integrity is now explicit: malformed records, lost-event markers,
truncated or unavailable bodies, duplicate IDs, ambiguous and missing responses
are surfaced. Explicit unknown IDs never consume a different request's response.
The legacy replay command refuses incomplete request bodies. Concurrent responses
without echoed IDs are deliberately left unpaired. This is a diagnostic boundary,
not a claim of complete HTTP/2 correlation.

This document records the verified scope of the new workbench as it is delivered.

## Scenario runner

`omni-scenario` runs ordered, portable JSON scenarios. Each run has a fresh cookie
jar. `extract` maps variable names to RFC 6901 JSON Pointers; `{{variable}}` can be
used in paths, headers and JSON. A whole JSON placeholder preserves the variable's
type. Missing/null required inputs fail before that request is sent. JSON query
variables are URL-encoded. The runner uses a single HTTP(S) origin, bounded 2 MiB
request/response bodies, 50..30000 ms request timeouts and at most 100 steps.
There are no implicit redirects, environment proxies or automatic scenario retries.

Each step requires an assertion: status, semantic JSON baseline, selected ignored
JSON Pointers, equals/exists/contains/type assertions, response headers, body SHA-256
or elapsed time. JSON key ordering does not matter; types and array ordering do.
Ignoring a field does not hide its disappearance. Failure skips dependent steps.
Reports contain a scenario SHA-256, original target, fault configuration, precise
failed checks and a bounded response preview. Extracted variable values are omitted.
Common secret keys are masked, not all possible application secrets or personal data.

```sh
cargo build --locked --bins
omni-scenario validate examples/scenarios/checkout.json
# In another terminal start omni-workbench, then:
omni-scenario run examples/scenarios/checkout.json --report report.json --junit report.xml
omni-scenario run examples/scenarios/checkout.json --fault examples/scenarios/fault-drift.json --report drift.json
```

Exit code is nonzero on invalid input, transport errors or failed assertions.
JSON/JUnit output files are written even when assertions fail. The library and
HTTP API intentionally return structured failed reports instead of transport errors.

## Reproducible HTTP faults

Each faulted run gets an ephemeral loopback gateway and fresh counters/state.
The saved baseline is unchanged. The exact path (without query) matches every
request to that path. Available fault kinds:

| Kind | Behavior |
| --- | --- |
| `status` | Return the configured HTTP status and an error JSON, without forwarding |
| `delay` | Wait `delay_ms` before forwarding |
| `json` | Forward, then replace an existing response JSON Pointer with `value` |
| `invalid_json` | Return HTTP 200 with invalid JSON, without forwarding |
| `disconnect` | Interrupt the response body so the HTTP client sees a transport failure |

This is HTTP fault reproduction, not packet loss, DNS, QUIC, TCP timing simulation,
or a retry/resilience assessment of the target application. Requests really execute;
there is no rollback of upstream writes. Use disposable test data. The gateway
rewrites the origin: domain-scoped/secure cookies, absolute redirects, signed Host
headers, mTLS and origin-bound authentication may need an application-specific
adapter. Gateway redirects are not followed. TLS verification stays enabled.

The integration suite exercises real local sockets: dynamic token/ID extraction,
cookie continuity, repeated 503 reproduction, semantic drift with HTTP 201,
invalid JSON, interrupted bodies, timeouts, missing inputs and redirect handling.
