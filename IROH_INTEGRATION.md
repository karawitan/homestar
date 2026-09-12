# Iroh Integration — Homestar

Tracking document for integrating [Iroh] networking into Homestar as an
alternative/supplement to the libp2p transport. This work is scaffolded on the
`feat/iroh-integration` branch of the `karawitan/homestar` fork.

## Background

IPVM (Interplanetary Virtual Machine) names Iroh as a peer node implementation in
the "Everywhere Computer" network. The Homestar roadmap (PR #306) lists Iroh
integration under Phase 1 and Phase 2:

- **Phase 1 — Networking / Multi-node testing**
  - Integration testing and simulation for libp2p swarm, gossipsub, Iroh peers
- **Phase 2 — Networking**
  - Iroh integration, esp. NAT traversal
- **Phase 2 — Observability**
  - iroh metrics

As of this branch, none of these items were implemented upstream. This fork
exists to prototype and test that integration.

## Scope of this branch

The goal is exploratory: stand up an Iroh transport alongside the existing
libp2p swarm, validate NAT traversal, and build the multi-node test harness
called out in Phase 1. It is **not** intended to replace libp2p wholesale.

### In scope
- `iroh` crate as an optional dependency behind the `iroh` feature flag
- `network::iroh` module with an `IrohEndpoint` wrapper around `iroh::Endpoint`
- Smoke test for endpoint bring-up
- Multi-node test harness using Iroh peers (Phase 1)
- NAT traversal validation (Phase 2)
- Iroh metrics emission (Phase 2)

### Out of scope (for now)
- Replacing the libp2p gossipsub / Kademlia DHT with Iroh equivalents
- Migrating the receipt/workflow DHT to Iroh blobs
- Any change to the default feature set (Iroh stays opt-in)

## MSRV note

`homestar-runtime` declares `rust-version = "1.75.0"`. Iroh 1.x requires
MSRV 1.89+. The `stable` toolchain (>= 1.89) compiles both fine. If/when this
work is proposed upstream, the MSRV discrepancy needs a decision: either bump
the crate's MSRV or pin to an older Iroh release line. See the comment in
`homestar-runtime/Cargo.toml` next to the `iroh` dependency.

## How to build and test

```sh
# Default build (no iroh) — unchanged behavior
cargo build -p homestar-runtime

# Build with the iroh feature enabled
cargo build -p homestar-runtime --features iroh

# Run the iroh module smoke test
cargo test -p homestar-runtime --features iroh --lib network::iroh
```

## Roadmap for this branch

- [x] Fork `ipvm-wg/homestar` → `karawitan/homestar`
- [x] Create `feat/iroh-integration` branch
- [x] Add `iroh` optional dependency + `iroh` feature flag
- [x] Add `network::iroh` module skeleton with `IrohEndpoint`
- [x] Wire module into `network/mod.rs`
- [x] Multi-node test harness with Iroh peers (Phase 1)
- [x] NAT traversal validation against Iroh relay (Phase 2)
- [x] Iroh metrics wired into the runtime metrics pipeline (Phase 2)
- [ ] Blob transfer prototype (Iroh blobs ↔ Homestar receipts/workflows)
- [ ] Open upstream tracking issue / discussion

## References

- Homestar roadmap PR: https://github.com/ipvm-wg/homestar/pull/306
- IPVM working group: https://github.com/ipvm-wg
- Iroh crate: https://crates.io/crates/iroh (1.0.3)
- Iroh docs: https://docs.rs/iroh
- Iroh repo: https://github.com/n0-computer/iroh
- Iroh discussion "What problem IPVM solves":
  https://github.com/n0-computer/iroh/discussions
