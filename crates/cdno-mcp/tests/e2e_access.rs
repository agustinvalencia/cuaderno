//! End-to-end tests for the Access-JWT origin middleware (GH #302).
//!
//! Spawns a mock Cloudflare-Access JWKS endpoint in-process, then the
//! real `cdno-mcp-server` binary in `--smoke` mode with the
//! `CDNO_ACCESS_*` env set, and drives it over HTTP with tokens signed
//! by a **test-only** RSA key (generated for this file; it protects
//! nothing). Asserts the fail-closed properties: no header → 401,
//! tampered/expired/wrong-audience/unknown-kid tokens → 401, valid
//! token → 200 — and that configuring auth is exactly what lifts the
//! non-loopback bind interlock.

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Test-only RSA keypair. Never reuse anywhere: it is public by
/// definition of being committed.
const TEST_RSA_PEM: &str = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEAtOg0ZjWqDyOKbjK9iBOqKEAh8oMoKbpm8Huwik071NCbwQ8x
1NeUHRCAzsd3/bKdF3w6QLr6J7ImVHyVG9+dmwkxGBfxYM04GEWMKBhOMGBIJ4+/
nBEBrL/O6cbIWNeu3ZFIgYY8/BZkduYi8ifICKKXKbdd+9wVUNsWKzsoDgQa8mmN
i26wbwQwBkkg7ezP74tinteJs/Tl1w1RRd/tolotPCPhUtuyjDQ83h5AS8XEb2ba
LIQlp1n1cNMvFWis7xAdFJYffhjh0XJd1M/5w4I8xD6Iyp9mr3+ac7hx64jX/3+I
RjHAzAq5SkwuTRTJM3LX4G2ZpauiiSY+M+I4JQIDAQABAoIBAA1d3t6dISHvOVTn
xjNIvgOCrD9z9hgR4CCYCF+fp0e+hij0oMpI3MvCTWd55jbGPMSLIZlozKpO3SAp
owbP4KBKRJNdfttCg940kqTfxDSKLf/7p2e2UKOypGzc4C7C+PZWvBRolmxLPJnA
arKZF+GeHyyDkkaAE3R0S6YJapiREJy7tHgihREn+Y4PWrCuzfnmkqkpn6iDqEi4
zyt/gL7vB0t1Rn43bbNk1PfbXukVgVLGXRsYlaTe/+tyQiuqkybDhcRciZGfr/PF
p0wU6OUcTgy/eIl11tZ0GvcwfblwFm3oH6TyGoZW353d2LWpeZqT/XBN/S9h0Pu0
ibjgBCECgYEA2xbaEHD+MFbJy/gGrIIZO0v4O7eqI7mCICJEFgbgOuUnKn7dZ4Bg
NCOIOPzTaXRji9hu2Tu73wKik8s6bFD8a7w9ZNSpr1vs95mXhvJxHQlkeE/xYWnX
CnlA/jxN8Sef4QKO+zgpWTTdFGzVCT5So8BQERvCiEggbsRudRIS8/kCgYEA02KV
eDZ1py439ORbwY1jd7N1cjwnqbYcGTvWwOU67oI2JyyeU26savaTAew8S6LLUR9z
7i8F80h/71egQMDk7LL0dMMfonjXpkuByjpQFAri+IIJc4ck7qxnzofEeD1kEGHV
ay5Uj6z0wuhHVw+GJU5Tm2tV5AVliqL/FKzGmI0CgYBSnX5jXshrX/6+fGu/11s+
YfpcQnjU+doY1fMIv1UEwG6Rdr90jRM59gAjRStPg8UZ8eZy4jSI9sxpoOQJ/kwB
MD2SbSMDbk2gXHmoOHnw8h7Bw5uJGUkuuOSKOiFGA6QlTDqwftAQxH9teVCoKKku
+JD4spgbnd8lBcuFN+iPuQKBgQDMLfJgfoIgbNVh993lVDPa4H42TIKnPB9iBFnI
UuMclKvIJSH9Ru7GFswi1FPdXy7yeeYaEFO4DbR9tG83fNrjA2x7CCqbXgw3NcH1
W2QUJ/vavIhyjfyPifpvFNciqXHpHQbvk33cldyKE6EtJ/KUQFcjzYbWTJwrUIwB
JW5i1QKBgQCPpk4ePS9UmH5U4U/HlPSl4TmDsOhils4HAPn9Jmh6lgHLlKv9gqQ9
RHRJpw/ggYFHLlo+q+QIwDIxvKAXH3TPoqq3fRSj47qrdWVX9kWNGY1kkOwQm1RU
ev6hSq5s3GviUNr7fePYzT9EzXu8QwgmS2xv0b3II+Gb9OfOHh9G2A==
-----END RSA PRIVATE KEY-----"#;

/// base64url modulus of the key above, as the mock JWKS serves it.
const TEST_RSA_N: &str = "tOg0ZjWqDyOKbjK9iBOqKEAh8oMoKbpm8Huwik071NCbwQ8x1NeUHRCAzsd3_bKdF3w6QLr6J7ImVHyVG9-dmwkxGBfxYM04GEWMKBhOMGBIJ4-_nBEBrL_O6cbIWNeu3ZFIgYY8_BZkduYi8ifICKKXKbdd-9wVUNsWKzsoDgQa8mmNi26wbwQwBkkg7ezP74tinteJs_Tl1w1RRd_tolotPCPhUtuyjDQ83h5AS8XEb2baLIQlp1n1cNMvFWis7xAdFJYffhjh0XJd1M_5w4I8xD6Iyp9mr3-ac7hx64jX_3-IRjHAzAq5SkwuTRTJM3LX4G2ZpauiiSY-M-I4JQ";
const TEST_RSA_E: &str = "AQAB";
const TEST_KID: &str = "test-kid-1";
const TEST_AUD: &str = "test-aud-tag";

/// Serve a mock `{team}/cdn-cgi/access/certs` on an OS-assigned port;
/// returns the team URL. The axum task lives for the process — fine
/// at test scale.
async fn mock_jwks_server() -> String {
    let jwks = json!({
        "keys": [
            { "kid": TEST_KID, "kty": "RSA", "alg": "RS256", "use": "sig",
              "n": TEST_RSA_N, "e": TEST_RSA_E },
            // A non-RSA entry the verifier must skip, as Cloudflare's
            // real document carries entries we don't use.
            { "kid": "ec-ignored", "kty": "EC", "n": "", "e": "" }
        ]
    });
    let app = axum::Router::new().route(
        "/cdn-cgi/access/certs",
        axum::routing::get(move || {
            let jwks = jwks.clone();
            async move { axum::Json(jwks) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock JWKS");
    let team_url = format!("http://{}", listener.local_addr().expect("addr"));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    team_url
}

/// Sign a token with the test key, expiring `exp_offset_secs` from
/// now (negative = already expired).
fn make_token(iss: &str, aud: &str, kid: &str, exp_offset_secs: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let claims = json!({
        "iss": iss,
        "aud": [aud],
        "email": "e2e@example.test",
        "iat": now,
        "exp": now + exp_offset_secs,
    });
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(kid.to_string());
    let key =
        jsonwebtoken::EncodingKey::from_rsa_pem(TEST_RSA_PEM.as_bytes()).expect("test key parses");
    jsonwebtoken::encode(&header, &claims, &key).expect("sign test token")
}

struct HttpServer {
    child: Child,
    port: u16,
}

impl HttpServer {
    /// Spawn `cdno-mcp-server --smoke` with Access auth configured
    /// against the mock JWKS. `bind_all` exercises the lifted
    /// interlock (0.0.0.0) — connections still arrive via loopback.
    fn spawn_with_auth(team_url: &str, bind_all: bool) -> Self {
        let port = free_port();
        let host = if bind_all { "0.0.0.0" } else { "127.0.0.1" };
        let bin = env!("CARGO_BIN_EXE_cdno-mcp-server");
        let child = Command::new(bin)
            .args(["--smoke", "--bind", &format!("{host}:{port}")])
            .env("RUST_LOG", "off")
            .env_remove("CUADERNO_VAULT_PATH")
            .env("CDNO_ACCESS_TEAM_URL", team_url)
            .env("CDNO_ACCESS_AUD", TEST_AUD)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn cdno-mcp-server");

        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "cdno-mcp-server did not start listening on port {port} within 15s"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        Self { child, port }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind :0");
    listener.local_addr().expect("local addr").port()
}

/// POST tools/list with an optional Access JWT; return the status.
async fn post_with_token(port: u16, token: Option<&str>) -> (u16, Value) {
    let client = reqwest::Client::new();
    let mut req = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }));
    if let Some(token) = token {
        req = req.header("Cf-Access-Jwt-Assertion", token);
    }
    let resp = req.send().await.expect("POST /mcp");
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("body");
    let value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, value)
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

// multi_thread: the mock JWKS runs as an in-process tokio task, and
// the spawn-readiness poll blocks the test thread — a current-thread
// runtime would starve the JWKS during the child's startup fetch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn valid_token_passes_and_everything_else_is_401() {
    let team_url = mock_jwks_server().await;
    let server = HttpServer::spawn_with_auth(&team_url, false);

    // No header → 401 before anything else runs.
    let (status, _) = post_with_token(server.port, None).await;
    assert_eq!(status, 401, "missing header must 401");

    // Valid token → the request reaches rmcp and lists the echo tool.
    let token = make_token(&team_url, TEST_AUD, TEST_KID, 600);
    let (status, resp) = post_with_token(server.port, Some(&token)).await;
    assert_eq!(status, 200, "valid token must pass: {resp}");
    assert_eq!(resp["result"]["tools"][0]["name"], json!("echo"), "{resp}");

    // Tampered signature → 401. (Flip a character near the end of
    // the signature segment; keep it valid base64url.)
    let mut tampered = token.clone();
    let last = tampered.pop().expect("nonempty");
    tampered.push(if last == 'A' { 'B' } else { 'A' });
    let (status, _) = post_with_token(server.port, Some(&tampered)).await;
    assert_eq!(status, 401, "tampered signature must 401");

    // Expired → 401 (offset far past jsonwebtoken's default leeway).
    let expired = make_token(&team_url, TEST_AUD, TEST_KID, -7200);
    let (status, _) = post_with_token(server.port, Some(&expired)).await;
    assert_eq!(status, 401, "expired token must 401");

    // Wrong audience → 401.
    let wrong_aud = make_token(&team_url, "some-other-app", TEST_KID, 600);
    let (status, _) = post_with_token(server.port, Some(&wrong_aud)).await;
    assert_eq!(status, 401, "wrong aud must 401");

    // Wrong issuer → 401.
    let wrong_iss = make_token("https://not-the-team.example", TEST_AUD, TEST_KID, 600);
    let (status, _) = post_with_token(server.port, Some(&wrong_iss)).await;
    assert_eq!(status, 401, "wrong iss must 401");

    // Unknown kid → one JWKS refresh, still unknown → 401.
    let unknown_kid = make_token(&team_url, TEST_AUD, "rotated-away", 600);
    let (status, _) = post_with_token(server.port, Some(&unknown_kid)).await;
    assert_eq!(status, 401, "unknown kid must 401");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_auth_lifts_the_non_loopback_interlock() {
    // Same 0.0.0.0 bind that refuses without auth (covered in
    // e2e_http.rs) must start once the verifier is configured —
    // and still demand a valid token.
    let team_url = mock_jwks_server().await;
    let server = HttpServer::spawn_with_auth(&team_url, true);

    let (status, _) = post_with_token(server.port, None).await;
    assert_eq!(status, 401);

    let token = make_token(&team_url, TEST_AUD, TEST_KID, 600);
    let (status, resp) = post_with_token(server.port, Some(&token)).await;
    assert_eq!(status, 200, "{resp}");
}

#[test]
fn refuses_to_start_when_jwks_is_unreachable() {
    // Fail-closed: auth configured but the team URL is dead → the
    // server must exit with an error, not serve unauthenticated.
    let port = free_port();
    let bin = env!("CARGO_BIN_EXE_cdno-mcp-server");
    let output = Command::new(bin)
        .args(["--smoke", "--bind", &format!("127.0.0.1:{port}")])
        .env("RUST_LOG", "off")
        // Reserved TEST-NET-1 address: nothing answers.
        .env("CDNO_ACCESS_TEAM_URL", "http://192.0.2.1:9")
        .env("CDNO_ACCESS_AUD", TEST_AUD)
        .output()
        .expect("run cdno-mcp-server");
    assert!(
        !output.status.success(),
        "an unreachable JWKS must abort startup (fail closed)"
    );
}
