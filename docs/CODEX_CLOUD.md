# Developing OmniProxy with Codex Cloud and GitHub

GitHub is the source of truth. Codex Cloud can work on a branch, build the Rust
binaries, run unit/integration/browser tests and prepare a pull request. It does
not need access to your desktop's browser, system proxy or private traffic.

## Environment setup

Use a Linux environment with a current stable Rust toolchain (including rustfmt),
a C/C++ compiler, CMake, pkg-config and Git. Node.js 24/npm are needed only for
the browser test suite. The workbench's frontend is embedded into the Rust binary.
The initial build includes Wasmtime/Cranelift and AWS-LC: allow several GB of RAM,
disk space for Cargo caches, and network access for crate/npm/browser downloads.
Low-memory environments should set:

```sh
export CARGO_BUILD_JOBS=1
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
```

Permit dependency registries during setup; keep application tests on loopback.
No production credentials are required for the bundled lab. The GitHub-connected
Codex environment should be granted access to this repository through the product's
normal repository setup. Never put GitHub tokens, CA private keys, recordings or
authentication variables into repository files.

## Acceptance commands

```sh
cargo fmt --all --check
cargo check --locked --all-targets
cargo test --locked --all-targets --all-features
cargo build --locked --bins
cargo run --locked --bin omni-scenario -- validate examples/scenarios/checkout.json
cd web
npm ci --ignore-scripts --no-audit --no-fund
npm run check
npx playwright install --with-deps chromium
npm test
```

The browser tests start an isolated workbench on port 19092 and a managed core on
19090. They use synthetic lab data and cover normal/fault runs, history comparison,
actual recording → inferred scenario → fresh successful replay, validation, theme
switching and a narrow viewport. Evidence is saved as a GitHub Actions artifact.

## Maintenance workflow

1. Make one user-visible capability on a branch, including acceptance evidence.
2. Update the core plan, roadmap and affected Chinese documentation.
3. Commit and push that milestone with the GitHub privacy email.
4. Inspect Actions, repair failed checks, then open/update a pull request.

The workflow builds Linux amd64/arm64/armv7, Windows x64 and both macOS targets,
and packages a Debian artifact. Cross-compiling is not validation on physical
ARM devices. HTTPS certificate installation, transparent capture and embedded
network integration still need acceptance on the relevant OS or hardware.

This guide prepares a repeatable cloud workflow. It does not claim that a Codex
Cloud account/environment was provisioned or that an embedded board was tested.
