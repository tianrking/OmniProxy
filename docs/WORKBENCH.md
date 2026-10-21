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
