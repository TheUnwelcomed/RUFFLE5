//! TLS: rustls 0.23 with RustCrypto's pure-Rust algorithms (rustls-rustcrypto) and Mozilla's root
//! certificates compiled in (webpki-roots), so nothing of the console's own TLS is needed and the
//! same code runs in the host check. TLS 1.3 and 1.2; X25519, P-256 and P-384 key exchange;
//! AES-GCM and ChaCha20-Poly1305; RSA, ECDSA and Ed25519 certificates. Random numbers come from
//! getrandom, which on the console is the title's arc4random_buf backend (src/random.rs).
// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::TcpStream;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

/// The client configuration: Mozilla's roots, and `extra` (a test proxy's CA).
pub fn config(extra: &[CertificateDer<'static>]) -> Result<Arc<ClientConfig>, String> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for c in extra {
        roots.add(c.clone()).map_err(|e| format!("an extra root certificate: {e}"))?;
    }
    let provider = Arc::new(rustls_rustcrypto::provider());
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS versions: {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(cfg))
}

/// The handshake with `host` over `tcp`, done here so that its failure is said as one.
pub fn connect(cfg: Arc<ClientConfig>, host: &str, mut tcp: TcpStream) -> Result<StreamOwned<ClientConnection, TcpStream>, String> {
    let name = ServerName::try_from(host.to_string()).map_err(|e| format!("{host}: not a name TLS accepts ({e})"))?;
    let mut conn = ClientConnection::new(cfg, name).map_err(|e| format!("TLS with {host}: {e}"))?;
    while conn.is_handshaking() {
        if let Err(e) = conn.complete_io(&mut tcp) {
            return Err(format!("TLS with {host}: {}", crate::http::describe(&e)));
        }
    }
    Ok(StreamOwned::new(conn, tcp))
}

/// "TLSv1_3, TLS13_AES_256_GCM_SHA384" for the log.
pub fn describe_session(conn: &ClientConnection) -> String {
    let version = conn.protocol_version().map(|v| format!("{v:?}")).unwrap_or_else(|| "?".into());
    let suite = conn.negotiated_cipher_suite().map(|s| format!("{:?}", s.suite())).unwrap_or_else(|| "?".into());
    let group = conn.negotiated_key_exchange_group().map(|g| format!("{:?}", g.name())).unwrap_or_else(|| "?".into());
    format!("{version}, {suite}, {group}")
}
