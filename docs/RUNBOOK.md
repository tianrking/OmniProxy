# OmniProxy v1.0 Runbook

## 1. Bootstrap

```bash
cargo run --bin omni_proxy -- --bootstrap
```

Expected:
1. CA files created.
2. Rule and flow files initialized.
3. Plugin directory exists.

## 2. CA Health Check

```bash
cargo run --bin omni_proxy -- --diagnose-ca
```

Gate:
1. `pair_parse_ok=true`

## 3. Rules Validation

```bash
cargo run --bin omni_proxy -- --check-rules --rule-file ./examples/rules-ci.txt
```

Gate:
1. parse success
2. expected counters printed

## 4. Core Build & Tests

```bash
cargo fmt --all --check
cargo check --all-targets
cargo test --all-targets --all-features
```

Gate:
1. all commands pass

## 5. Concurrency Convergence

```bash
cargo run --bin omni-converge -- --url https://example.com --requests 4000 --concurrency 256 --proxy http://127.0.0.1:9090
```

Gate recommendation:
1. `error_rate` low and stable over repeated runs
2. p95/p99 no escalating trend

## 6. Replay Validation

```bash
cargo run --bin omni-replay -- --list
cargo run --bin omni-replay -- --index 0 --dry-run --print-curl
```

Gate:
1. can list
2. can produce curl
3. replay result prints status/bytes/hash diffs

## 7. Release Workflow

Workflow: `.github/workflows/release.yml`

Artifacts expected:
1. binaries
2. `SHA256SUMS`
3. `PROVENANCE.json`
4. `SHA256SUMS.sig`
5. `SHA256SUMS.pem`

## 8. Final Go/No-Go Checklist

1. Build and tests green.
2. CA diagnostics healthy.
3. Rule preflight success.
4. Convergence run acceptable.
5. Replay path verified.
6. Release bundle + signatures generated.
