//! Cross-network Iroh echo test binary.
//!
//! Runs as either a server (bind + accept + echo) or a client (connect +
//! send + read echo). Used to validate real NAT traversal between machines.
//!
//! # Usage
//!
//! Server (on the public-IP host, e.g. telour):
//! ```sh
//! iroh_echo --server
//! # prints: SERVER_ADDR=<endpoint-addr>
//! # prints: SERVER_ID=<node-id>
//! ```
//!
//! Client (on the NAT'd host, e.g. Mac):
//! ```sh
//! iroh_echo --client "<endpoint-addr>" --message "hello from behind NAT"
//! ```
//!
//! Both sides print their node ID and connection status. The server stays
//! running until Ctrl-C. The client connects, sends a message over a QUIC
//! bi-stream, reads the echo, and exits.

use anyhow::{Context, Result};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, Watcher};
use std::time::Duration;
/// ALPN for the echo protocol (mirrors homestar-runtime's HOMESTAR_ALPN).
const ECHO_ALPN: &[u8] = b"/homestar-echo/1";

/// Max bytes to read from a stream.
const READ_LIMIT: usize = 10 * 1024 * 1024;

#[derive(Debug, Clone)]
struct Args {
    mode: Mode,
    message: String,
    wait_online: bool,
}

#[derive(Debug, Clone)]
enum Mode {
    Server,
    Client { server_addr: String },
}

fn parse_args() -> Result<Args> {
    let args: Vec<String> = std::env::args().collect();
    let mut mode: Option<Mode> = None;
    let mut message = "hello from iroh echo".to_string();
    let mut wait_online = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--server" => mode = Some(Mode::Server),
            "--client" => {
                i += 1;
                let addr = args
                    .get(i)
                    .context("--client requires an endpoint address argument")?;
                mode = Some(Mode::Client {
                    server_addr: addr.clone(),
                });
            }
            "--message" | "-m" => {
                i += 1;
                message = args
                    .get(i)
                    .context("--message requires a string argument")?
                    .clone();
            }
            "--online" => wait_online = true,
            "--help" | "-h" => {
                eprintln!("Usage:");
                eprintln!("  iroh_echo --server [--online]");
                eprintln!("  iroh_echo --client <endpoint-addr> [--message <msg>] [--online]");
                std::process::exit(0);
            }
            other => {
                anyhow::bail!("unknown argument: {other}");
            }
        }
        i += 1;
    }

    let mode = mode.context("must specify --server or --client <addr>")?;
    Ok(Args {
        mode,
        message,
        wait_online,
    })
}

async fn build_endpoint() -> Result<Endpoint> {
    let builder = Endpoint::builder(presets::N0).alpns(vec![ECHO_ALPN.to_vec()]);
    builder.bind().await.context("failed to bind endpoint")
}

async fn run_server(args: Args) -> Result<()> {
    let endpoint = build_endpoint().await?;
    let node_id = endpoint.id();
    let addr = endpoint.watch_addr().get();

    eprintln!("[server] node_id: {node_id}");
    println!("SERVER_ID={node_id}");
    let addr_json = serde_json::to_string(&addr).context("failed to serialize addr")?;
    println!("SERVER_ADDR={addr_json}");

    if args.wait_online {
        eprintln!("[server] waiting for relay connection...");
        tokio::time::timeout(Duration::from_secs(30), endpoint.online())
            .await
            .context("timed out waiting for relay connection")?;
        eprintln!("[server] online (relay connected)");
        let addr_after = endpoint.watch_addr().get();
        let addr_after_json =
            serde_json::to_string(&addr_after).context("failed to serialize addr")?;
        println!("SERVER_ADDR_ONLINE={addr_after_json}");
    }

    eprintln!("[server] accepting connections on ALPN {}...", String::from_utf8_lossy(ECHO_ALPN));

    loop {
        match endpoint.accept().await {
            Some(incoming) => {
                let ep = endpoint.clone();
                tokio::spawn(async move {
                    match incoming.await {
                        Ok(conn) => {
                            eprintln!("[server] connection from {:?}", conn.remote_id());
                            match echo_connection(&conn).await {
                                Ok(n) => eprintln!("[server] echoed {n} bytes"),
                                Err(e) => eprintln!("[server] echo error: {e}"),
                            }
                            conn.closed().await;
                        }
                        Err(e) => eprintln!("[server] accept error: {e}"),
                    }
                    let _ = ep;
                });
            }
            None => {
                eprintln!("[server] endpoint closed, shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn echo_connection(conn: &iroh::endpoint::Connection) -> Result<usize> {
    let (mut send, mut recv) = conn.accept_bi().await.context("accept_bi failed")?;
    let buf = recv.read_to_end(READ_LIMIT).await.context("read_to_end failed")?;
    send.write_all(&buf).await.context("write_all failed")?;
    send.finish().context("finish failed")?;
    Ok(buf.len())
}

async fn run_client(args: Args) -> Result<()> {
    let endpoint = build_endpoint().await?;
    let node_id = endpoint.id();
    eprintln!("[client] node_id: {node_id}");

    if args.wait_online {
        eprintln!("[client] waiting for relay connection...");
        tokio::time::timeout(Duration::from_secs(30), endpoint.online())
            .await
            .context("timed out waiting for relay connection")?;
        eprintln!("[client] online (relay connected)");
    }

    let server_addr: EndpointAddr = match &args.mode {
        Mode::Client { server_addr } => serde_json::from_str(server_addr)
            .with_context(|| format!("failed to parse endpoint address JSON: {server_addr}"))?,
        _ => unreachable!(),
    };

    eprintln!("[client] connecting to server...");
    let conn = tokio::time::timeout(
        Duration::from_secs(30),
        endpoint.connect(server_addr, ECHO_ALPN),
    )
    .await
    .context("connect timed out")?
    .context("connect failed")?;

    eprintln!("[client] connected, opening bi-stream...");
    let (mut send, mut recv) = conn.open_bi().await.context("open_bi failed")?;

    let message = args.message.as_bytes();
    eprintln!("[client] sending {} bytes: {:?}", message.len(), args.message);
    send.write_all(message).await.context("write_all failed")?;
    send.finish().context("finish failed")?;

    eprintln!("[client] reading echo...");
    let echo = tokio::time::timeout(
        Duration::from_secs(10),
        recv.read_to_end(READ_LIMIT),
    )
    .await
    .context("read timed out")?
    .context("read_to_end failed")?;

    if echo == message {
        eprintln!("[client] ✓ echo matches ({} bytes)", echo.len());
        println!("ECHO_OK=true");
        println!("ECHO_BYTES={}", echo.len());
    } else {
        eprintln!("[client] ✗ echo mismatch: sent {} bytes, got {} bytes", message.len(), echo.len());
        println!("ECHO_OK=false");
        println!("ECHO_BYTES={}", echo.len());
    }

    conn.close(0u32.into(), b"done");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;

    match args.mode {
        Mode::Server => run_server(args).await,
        Mode::Client { .. } => run_client(args).await,
    }
}
