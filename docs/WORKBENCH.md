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

## Browser workbench

Run `cargo build --locked --bins`, then `cargo run --locked --bin omni-workbench`.
Open `http://127.0.0.1:9092`. The Chinese interface includes overview metrics,
managed proxy recording, flow search/selection and header/body inspection, JSONL
import, scenario library, visual step editing/full JSON, runtime variables,
fault controls, historical run comparison and JSON/JUnit downloads. It supports
light/dark themes, keyboard focus, Ctrl/Cmd+S and narrow layouts. No frontend build
is needed: HTML/CSS/JS assets are embedded in the executable.

The default workspace is `.omni-workbench` (ignored by Git). Scenario JSON and
reports persist across restarts. A built-in lab scenario is refreshed with the
current local port at startup; duplicate it to retain custom edits. The visual
editor preserves custom HTTP methods, and a new recording resets prior selections.
The UI only binds loopback, checks Host/Origin,
rejects cross-site browser requests and serves a restrictive CSP without external
assets. Document IDs cannot address arbitrary filesystem paths. It has no remote
authentication or multi-user deployment support.

The recorder starts only a sibling `omni_proxy` executable, checks readiness and
manages only its own process. It does not edit OS proxy settings or trust stores.
Stop test traffic before stopping a recording: in-flight requests may otherwise
remain incomplete. Rotated segments can contain orphaned responses at boundaries;
import only a complete, focused recording. The UI CA endpoint serves the public
certificate, never its private key. Raw local recordings contain sensitive data;
masking is best-effort in previews and generated JSON, not encryption of the log.

`录制示例流量` sends three real HTTP requests through the managed proxy. Selecting
them and generating a scenario links exact prior JSON string values used in later
paths, query values, headers (including Bearer tokens) and JSON bodies. Cookie
headers inherited from earlier Set-Cookie responses are removed so a fresh cookie
jar can establish its own session. Inference considers bounded, nontrivial string
values; encoded, numeric, transformed, ambiguous or application-specific flows
still need review and manual extraction. Unlinked secrets become required null
input variables. Non-JSON bodies are preserved as base64 and need manual review.

A fault path that no request reaches produces an explicit failed coverage check,
so an unused fault configuration cannot masquerade as a successful experiment.

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
