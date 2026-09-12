//! Iroh-based networking for [Homestar].
//!
//! This module provides an alternative/supplement to the libp2p transport using
//! [iroh] for direct QUIC peer connectivity with NAT traversal. It is gated
//! behind the `iroh` feature flag.
//!
//! # Status
//!
//! This is scaffolding for the Iroh integration work tracked in
//! `IROH_INTEGRATION.md` and on the Homestar roadmap (PR #306, Phase 1 & 2):
//!   - Phase 1: multi-node testing with Iroh peers
//!   - Phase 2: Iroh integration, esp. NAT traversal; iroh metrics
//!
//! [Homestar]: crate
//! [iroh]: iroh

use anyhow::{Context, Result};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointId};
use tracing::info;

/// Iroh endpoint wrapper, holding a connected [Endpoint] and the local node's
/// [EndpointId].
///
/// [Endpoint]: iroh::Endpoint
/// [EndpointId]: iroh::EndpointId
#[derive(Debug)]
pub(crate) struct IrohEndpoint {
    endpoint: Endpoint,
    node_id: EndpointId,
}

impl IrohEndpoint {
    /// Build and start a new iroh [Endpoint] using the `N0` preset (relay-backed
    /// NAT traversal + DNS address lookup), binding to an ephemeral port.
    ///
    /// The `N0` preset enables the public n0 relay servers and Pkarr-based DNS
    /// address lookup, which is the default configuration for peers that need
    /// to be reachable across NATs. For local-only / test scenarios, callers
    /// can switch to the `Minimal` preset in future work.
    ///
    /// [Endpoint]: iroh::Endpoint
    pub(crate) async fn bind() -> Result<Self> {
        let endpoint = Endpoint::builder(presets::N0)
            .bind()
            .await
            .context("failed to bind iroh endpoint")?;

        let node_id = endpoint.id();

        info!(
            subject = "iroh.endpoint.init",
            category = "iroh",
            node_id = %node_id,
            "iroh endpoint started"
        );

        Ok(Self { endpoint, node_id })
    }

    /// The local node's iroh [EndpointId] (public key / node id).
    ///
    /// [EndpointId]: iroh::EndpointId
    #[allow(dead_code)]
    pub(crate) fn node_id(&self) -> &EndpointId {
        &self.node_id
    }

    /// Borrow the underlying iroh [Endpoint].
    ///
    /// [Endpoint]: iroh::Endpoint
    #[allow(dead_code)]
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bind_endpoint() {
        // Smoke test: ensure we can bring up an iroh endpoint with the N0
        // preset. If the feature is disabled this test is not compiled.
        let ep = IrohEndpoint::bind().await;
        assert!(ep.is_ok(), "failed to bind iroh endpoint: {:?}", ep.err());
    }
}
