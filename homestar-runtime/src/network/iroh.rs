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
use iroh::endpoint::{Accept, Connection};
use iroh::{Endpoint, EndpointAddr, EndpointId, Watcher};
use tracing::info;

/// ALPN protocol identifier for Homestar's internal Iroh transport.
///
/// Both peers must agree on this to establish a connection. Future work will
/// likely make this configurable.
pub(crate) const HOMESTAR_ALPN: &[u8] = b"/homestar/1";

/// Iroh endpoint wrapper, holding a bound [Endpoint] and the local node's
/// [EndpointId].
///
/// All methods are currently only exercised by the test suite; they will be
/// wired into the runtime as the Iroh integration progresses.
///
/// [Endpoint]: iroh::Endpoint
/// [EndpointId]: iroh::EndpointId
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct IrohEndpoint {
    endpoint: Endpoint,
    node_id: EndpointId,
}

impl IrohEndpoint {
    /// Build and start a new iroh [Endpoint] using the `N0` preset (relay-backed
    /// NAT traversal + DNS address lookup), binding to an ephemeral port with no
    /// ALPNs configured (cannot accept incoming connections).
    ///
    /// Use [`IrohEndpoint::bind_with_alpn`] to accept incoming connections.
    ///
    /// [Endpoint]: iroh::Endpoint
    pub(crate) async fn bind() -> Result<Self> {
        Self::bind_with_alpn(&[]).await
    }

    /// Build and start a new iroh [Endpoint] using the `N0` preset, configuring
    /// the given ALPN protocols for accepting incoming connections.
    ///
    /// The `N0` preset enables the public n0 relay servers and Pkarr-based DNS
    /// address lookup, which is the default configuration for peers that need
    /// to be reachable across NATs.
    ///
    /// [Endpoint]: iroh::Endpoint
    pub(crate) async fn bind_with_alpn(alpns: &[&[u8]]) -> Result<Self> {
        let mut builder = Endpoint::builder(presets::N0);
        if !alpns.is_empty() {
            builder = builder.alpns(alpns.iter().map(|a| a.to_vec()).collect());
        }
        let endpoint = builder
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
    pub(crate) fn node_id(&self) -> &EndpointId {
        &self.node_id
    }

    /// The full [EndpointAddr] of this node, suitable for handing to
    /// [`IrohEndpoint::connect`] on a remote peer.
    ///
    /// [EndpointAddr]: iroh::EndpointAddr
    pub(crate) fn addr(&self) -> EndpointAddr {
        self.endpoint.watch_addr().get()
    }

    /// Borrow the underlying iroh [Endpoint].
    ///
    /// [Endpoint]: iroh::Endpoint
    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Connect to a remote peer at the given [EndpointAddr], negotiating the
    /// given ALPN. Returns a [Connection] that can be used to open bi/uni
    /// streams.
    ///
    /// [EndpointAddr]: iroh::EndpointAddr
    /// [Connection]: iroh::endpoint::Connection
    pub(crate) async fn connect(
        &self,
        addr: impl Into<EndpointAddr>,
        alpn: &[u8],
    ) -> Result<Connection> {
        self.endpoint
            .connect(addr, alpn)
            .await
            .context("failed to connect to iroh peer")
    }

    /// Returns an [Accept] future that yields incoming connections. The caller
    /// is responsible for spawning an accept loop.
    ///
    /// [Accept]: iroh::endpoint::Accept
    pub(crate) fn accept(&self) -> Accept<'_> {
        self.endpoint.accept()
    }

    /// Wait until the endpoint has established a connection to at least one
    /// relay server. This is the precondition for NAT traversal: hole-punching
    /// is coordinated through the relay, so until `online()` returns, the
    /// endpoint may not be reachable from behind a NAT.
    ///
    /// See [`Endpoint::online`] in the iroh docs for details.
    ///
    /// [`Endpoint::online`]: iroh::Endpoint::online
    pub(crate) async fn online(&self) {
        self.endpoint.online().await;
    }

    /// Returns the iroh [EndpointMetrics] for this endpoint.
    ///
    /// The `metrics` feature is always enabled on our iroh dependency, so this
    /// is always available when the `iroh` feature flag is on.
    ///
    /// [EndpointMetrics]: iroh::metrics::EndpointMetrics
    pub(crate) fn metrics(&self) -> &iroh::metrics::EndpointMetrics {
        self.endpoint.metrics()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Maximum bytes to read from a single stream in tests.
    const READ_LIMIT: usize = 1024 * 1024;

    /// Smoke test: ensure we can bring up an iroh endpoint with the N0
    /// preset.
    #[tokio::test]
    async fn bind_endpoint() {
        let ep = IrohEndpoint::bind().await;
        assert!(ep.is_ok(), "failed to bind iroh endpoint: {:?}", ep.err());
    }

    /// Multi-node test (Phase 1): bring up two Iroh endpoints, connect from
    /// a client to a server, exchange a message over a bidirectional QUIC
    /// stream, and verify the round-trip.
    ///
    /// This validates the core Iroh peer connectivity primitive that Homestar
    /// would use as an alternative to the libp2p transport.
    #[tokio::test]
    async fn two_node_bi_stream_roundtrip() {
        // Server: accept connections on the Homestar ALPN.
        let server = IrohEndpoint::bind_with_alpn(&[HOMESTAR_ALPN])
            .await
            .expect("failed to bind server endpoint");
        let server_addr = server.addr();
        let server_id = *server.node_id();

        // Client: no ALPNs needed (outgoing only).
        let client = IrohEndpoint::bind()
            .await
            .expect("failed to bind client endpoint");

        // Spawn the server accept loop: echo back any bytes received on a
        // bidirectional stream.
        let server_ep = server.endpoint().clone();
        let server_task = tokio::spawn(async move {
            let incoming = server_ep.accept().await.expect("accept failed");
            let conn = incoming.await.expect("connection failed");
            let (mut send, mut recv) = conn.accept_bi().await.expect("accept_bi failed");

            // Echo: read all bytes, write them back.
            let buf = recv.read_to_end(READ_LIMIT).await.expect("read_to_end failed");
            send.write_all(&buf).await.expect("write_all failed");
            send.finish().expect("finish failed");

            // Keep the connection alive until the client closes it, otherwise
            // dropping `conn` here would abort the QUIC connection and the
            // client's read would fail with ConnectionLost.
            conn.closed().await;
        });

        // Give the server a moment to start accepting.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Client connects to the server.
        let conn = client
            .connect(server_addr, HOMESTAR_ALPN)
            .await
            .expect("failed to connect");

        // Open a bidirectional stream, send a message, read the echo.
        let (mut send, mut recv) = conn.open_bi().await.expect("open_bi failed");
        let message = b"hello from homestar iroh multi-node test";
        send.write_all(message).await.expect("write_all failed");
        send.finish().expect("finish failed");

        let echo = recv.read_to_end(READ_LIMIT).await.expect("read_to_end failed");

        // Verify the round-trip.
        assert_eq!(echo, message, "echoed message does not match");

        // Close the connection so the server's conn.closed().await resolves.
        conn.close(0u32.into(), b"done");

        // Wait for the server task to finish.
        server_task.await.expect("server task panicked");

        // Sanity: the server's node id should differ from the client's.
        assert_ne!(
            *client.node_id(),
            server_id,
            "client and server should have different node ids"
        );
    }

    /// Multi-node test (Phase 1): verify that three endpoints can coexist and
    /// that a single client can connect to two different servers concurrently.
    #[tokio::test]
    async fn one_client_two_servers() {
        // Two servers with the Homestar ALPN.
        let server_a = IrohEndpoint::bind_with_alpn(&[HOMESTAR_ALPN])
            .await
            .expect("failed to bind server A");
        let server_b = IrohEndpoint::bind_with_alpn(&[HOMESTAR_ALPN])
            .await
            .expect("failed to bind server B");
        let addr_a = server_a.addr();
        let addr_b = server_b.addr();

        // One client.
        let client = IrohEndpoint::bind()
            .await
            .expect("failed to bind client");

        // Spawn accept loops for both servers.
        let ep_a = server_a.endpoint().clone();
        let task_a = tokio::spawn(async move {
            let incoming = ep_a.accept().await.expect("accept A failed");
            let conn = incoming.await.expect("conn A failed");
            let (mut send, mut recv) = conn.accept_bi().await.expect("accept_bi A failed");
            let buf = recv.read_to_end(READ_LIMIT).await.expect("read A failed");
            send.write_all(&buf).await.expect("write A failed");
            send.finish().expect("finish A failed");
            conn.closed().await;
        });

        let ep_b = server_b.endpoint().clone();
        let task_b = tokio::spawn(async move {
            let incoming = ep_b.accept().await.expect("accept B failed");
            let conn = incoming.await.expect("conn B failed");
            let (mut send, mut recv) = conn.accept_bi().await.expect("accept_bi B failed");
            let buf = recv.read_to_end(READ_LIMIT).await.expect("read B failed");
            send.write_all(&buf).await.expect("write B failed");
            send.finish().expect("finish B failed");
            conn.closed().await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Connect to both servers and exchange distinct messages.
        let msg_a = b"ping-A";
        let msg_b = b"ping-B-different";

        let conn_a = client
            .connect(addr_a, HOMESTAR_ALPN)
            .await
            .expect("connect A failed");
        let conn_b = client
            .connect(addr_b, HOMESTAR_ALPN)
            .await
            .expect("connect B failed");

        let (mut send_a, mut recv_a) = conn_a.open_bi().await.expect("open_bi A failed");
        let (mut send_b, mut recv_b) = conn_b.open_bi().await.expect("open_bi B failed");

        send_a.write_all(msg_a).await.expect("write A failed");
        send_a.finish().expect("finish A failed");
        send_b.write_all(msg_b).await.expect("write B failed");
        send_b.finish().expect("finish B failed");

        let echo_a = recv_a.read_to_end(READ_LIMIT).await.expect("read echo A failed");
        let echo_b = recv_b.read_to_end(READ_LIMIT).await.expect("read echo B failed");

        assert_eq!(echo_a, msg_a, "server A echo mismatch");
        assert_eq!(echo_b, msg_b, "server B echo mismatch");

        // Close both connections so the servers' conn.closed().await resolve.
        conn_a.close(0u32.into(), b"done");
        conn_b.close(0u32.into(), b"done");

        task_a.await.expect("task A panicked");
        task_b.await.expect("task B panicked");
    }

    /// NAT traversal test (Phase 2): verify that two endpoints can connect
    /// through the Iroh relay after both have come `online()`.
    ///
    /// The `N0` preset uses public n0 relay servers for NAT traversal. This
    /// test waits for both endpoints to establish a relay connection (via
    /// `online()`), then verifies that a bi-stream round-trip succeeds
    /// through the relay-assisted path.
    ///
    /// This test requires network access to the public n0 relay servers. It
    /// is expected to take a few seconds for relay connection establishment.
    #[tokio::test]
    #[ignore = "requires network access to public n0 relay servers"]
    async fn nat_traversal_via_relay() {
        // Server: accept on the Homestar ALPN.
        let server = IrohEndpoint::bind_with_alpn(&[HOMESTAR_ALPN])
            .await
            .expect("failed to bind server endpoint");

        // Client: outgoing only.
        let client = IrohEndpoint::bind()
            .await
            .expect("failed to bind client endpoint");

        // Wait for both endpoints to connect to a relay. This is the
        // precondition for NAT traversal: hole-punching is coordinated
        // through the relay.
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            futures::future::join(server.online(), client.online()),
        )
        .await
        .expect("timed out waiting for endpoints to come online");

        let server_addr = server.addr();

        // Spawn the server accept loop (same echo pattern as Phase 1 tests).
        let server_ep = server.endpoint().clone();
        let server_task = tokio::spawn(async move {
            let incoming = server_ep.accept().await.expect("accept failed");
            let conn = incoming.await.expect("connection failed");
            let (mut send, mut recv) = conn.accept_bi().await.expect("accept_bi failed");
            let buf = recv.read_to_end(READ_LIMIT).await.expect("read_to_end failed");
            send.write_all(&buf).await.expect("write_all failed");
            send.finish().expect("finish failed");
            conn.closed().await;
        });

        // Client connects to the server through the relay.
        let conn = client
            .connect(server_addr, HOMESTAR_ALPN)
            .await
            .expect("failed to connect via relay");

        let (mut send, mut recv) = conn.open_bi().await.expect("open_bi failed");
        let message = b"nat traversal echo via iroh relay";
        send.write_all(message).await.expect("write_all failed");
        send.finish().expect("finish failed");

        let echo = recv.read_to_end(READ_LIMIT).await.expect("read_to_end failed");
        assert_eq!(echo, message, "echoed message does not match");

        conn.close(0u32.into(), b"done");
        server_task.await.expect("server task panicked");
    }

    /// Metrics test (Phase 2): verify that iroh endpoint metrics are
    /// accessible and populated after binding and connecting.
    ///
    /// After an endpoint comes online and exchanges data, the socket metrics
    /// counters (bytes sent/received, relay connection counters) should be
    /// non-zero. This validates that the iroh metrics pipeline is wired up
    /// and can be scraped for observability.
    #[tokio::test]
    #[ignore = "requires network access to public n0 relay servers"]
    async fn metrics_populated_after_connection() {
        let server = IrohEndpoint::bind_with_alpn(&[HOMESTAR_ALPN])
            .await
            .expect("failed to bind server endpoint");
        let client = IrohEndpoint::bind()
            .await
            .expect("failed to bind client endpoint");

        // Come online so relay metrics are populated.
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            futures::future::join(server.online(), client.online()),
        )
        .await
        .expect("timed out waiting for endpoints to come online");

        let server_addr = server.addr();

        // Quick echo exchange to generate traffic.
        let server_ep = server.endpoint().clone();
        let server_task = tokio::spawn(async move {
            let incoming = server_ep.accept().await.expect("accept failed");
            let conn = incoming.await.expect("connection failed");
            let (mut send, mut recv) = conn.accept_bi().await.expect("accept_bi failed");
            let buf = recv.read_to_end(READ_LIMIT).await.expect("read_to_end failed");
            send.write_all(&buf).await.expect("write_all failed");
            send.finish().expect("finish failed");
            conn.closed().await;
        });

        let conn = client
            .connect(server_addr, HOMESTAR_ALPN)
            .await
            .expect("failed to connect");

        let (mut send, mut recv) = conn.open_bi().await.expect("open_bi failed");
        let message = b"metrics test payload";
        send.write_all(message).await.expect("write_all failed");
        send.finish().expect("finish failed");
        let _echo = recv.read_to_end(READ_LIMIT).await.expect("read_to_end failed");

        conn.close(0u32.into(), b"done");
        server_task.await.expect("server task panicked");

        // Verify that iroh metrics are accessible and that at least one
        // counter has been incremented. The relay_conns_success counter
        // should be >= 1 since both endpoints came online via the relay.
        let client_metrics = client.metrics();
        let relay_conns = client_metrics.socket.relay_conns_success.get();

        assert!(
            relay_conns >= 1,
            "expected relay_conns_success >= 1 after coming online, got {relay_conns}"
        );

        // The net_report reports counter should also be >= 1 since net
        // reports run automatically when the endpoint comes online.
        let net_reports = client_metrics.net_report.reports.get();
        assert!(
            net_reports >= 1,
            "expected net_report reports >= 1, got {net_reports}"
        );
    }
}
