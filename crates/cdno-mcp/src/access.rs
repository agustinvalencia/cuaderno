//! Origin-side validation of the Cloudflare Access identity JWT
//! (GH #302).
//!
//! The remote deployment terminates OAuth 2.1 at Cloudflare Access
//! (Managed OAuth); after Access authenticates a request it injects a
//! signed identity assertion in the `Cf-Access-Jwt-Assertion` header
//! toward the origin. This module verifies that assertion on **every**
//! request — defence in depth per design decision D5: the tunnel is
//! never trusted alone, so a leaked tunnel hostname, a misconfigured
//! Access policy, or another local process pointed at the port all
//! still fail here.
//!
//! Verification is deliberately strict and fail-closed:
//!
//! - **RS256 only.** The algorithm comes from our `Validation`, never
//!   from the token header — `alg: none` and cross-algorithm
//!   confusion are structurally impossible.
//! - **`iss` must equal the team URL** and **`aud` must contain the
//!   Access application's AUD tag**; `exp` is required (all enforced
//!   by `jsonwebtoken`'s `Validation`).
//! - **Keys come from the team JWKS** (`{team}/cdn-cgi/access/certs`),
//!   fetched at startup — construction fails if the fetch does, so
//!   the server never starts with an empty trust store — and
//!   refreshed at most once per request on an unknown `kid` (key
//!   rotation), guarded so a flood of bad-`kid` tokens cannot stampede
//!   the JWKS endpoint.
//! - Every failure maps to plain **401** with the reason logged at
//!   debug on the server, never echoed to the client.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};

/// Header Cloudflare injects after Access authenticates the request.
pub const ACCESS_JWT_HEADER: &str = "Cf-Access-Jwt-Assertion";

/// Minimum spacing between JWKS refreshes triggered by unknown-`kid`
/// tokens. Real key rotation is rare (days); this only has to be
/// short enough that a rotation is picked up promptly and long
/// enough that forged tokens can't turn the verifier into a JWKS
/// query cannon.
const REFRESH_COOLDOWN: Duration = Duration::from_secs(30);

/// One entry of the `{team}/cdn-cgi/access/certs` document. Fields we
/// don't verify with are ignored by serde.
#[derive(serde::Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    n: String,
    e: String,
}

#[derive(serde::Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

/// Shared verifier state: the key cache plus the refresh stamp that
/// implements the anti-stampede cooldown.
struct Cache {
    keys: HashMap<String, DecodingKey>,
    last_refresh: Instant,
}

/// Validates `Cf-Access-Jwt-Assertion` headers against a Cloudflare
/// Access team's JWKS. Cheap to clone (everything shared).
#[derive(Clone)]
pub struct JwtVerifier {
    /// Team URL, e.g. `https://<team>.cloudflareaccess.com` — doubles
    /// as the expected `iss` claim.
    issuer: String,
    /// The Access application's AUD tag.
    audience: String,
    certs_url: String,
    client: reqwest::Client,
    cache: Arc<tokio::sync::RwLock<Cache>>,
}

impl JwtVerifier {
    /// Build a verifier and perform the initial JWKS fetch.
    ///
    /// Fail-closed by construction: if the JWKS cannot be fetched or
    /// contains no usable RSA keys, this errors and the server must
    /// not start.
    pub async fn new(team_url: &str, audience: &str) -> anyhow::Result<Self> {
        let issuer = team_url.trim_end_matches('/').to_string();
        let verifier = Self {
            certs_url: format!("{issuer}/cdn-cgi/access/certs"),
            issuer,
            audience: audience.to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
            cache: Arc::new(tokio::sync::RwLock::new(Cache {
                keys: HashMap::new(),
                last_refresh: Instant::now(),
            })),
        };
        let count = verifier.refresh_keys().await?;
        anyhow::ensure!(
            count > 0,
            "JWKS at {} contained no usable RSA keys",
            verifier.certs_url
        );
        tracing::info!(jwks = %verifier.certs_url, keys = count, "Access JWT verification active");
        Ok(verifier)
    }

    /// Fetch the JWKS and replace the key cache. Returns how many
    /// usable keys were installed.
    async fn refresh_keys(&self) -> anyhow::Result<usize> {
        let jwks: Jwks = self
            .client
            .get(&self.certs_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut keys = HashMap::new();
        for jwk in jwks.keys {
            if jwk.kty != "RSA" {
                continue;
            }
            match DecodingKey::from_rsa_components(&jwk.n, &jwk.e) {
                Ok(key) => {
                    keys.insert(jwk.kid, key);
                }
                Err(e) => {
                    tracing::warn!(kid = %jwk.kid, error = %e, "skipping malformed JWK")
                }
            }
        }
        let count = keys.len();
        let mut cache = self.cache.write().await;
        cache.keys = keys;
        cache.last_refresh = Instant::now();
        Ok(count)
    }

    /// Verify one token: RS256 signature against a cached JWKS key
    /// (refreshing once on unknown `kid`, subject to the cooldown),
    /// then `iss`, `aud`, and `exp`.
    pub async fn verify(&self, token: &str) -> Result<(), VerifyError> {
        let kid = decode_header(token)
            .map_err(VerifyError::Header)?
            .kid
            .ok_or(VerifyError::MissingKid)?;

        let key = match self.cached_key(&kid).await {
            Some(key) => key,
            None => {
                // Unknown kid: allow one JWKS refresh (key rotation),
                // but never more often than the cooldown — a stream of
                // forged kids must not hammer the JWKS endpoint.
                let due = {
                    let cache = self.cache.read().await;
                    cache.last_refresh.elapsed() >= REFRESH_COOLDOWN
                };
                if due {
                    self.refresh_keys().await.map_err(VerifyError::Refresh)?;
                }
                self.cached_key(&kid).await.ok_or(VerifyError::UnknownKid)?
            }
        };

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[&self.issuer]);
        validation.set_audience(&[&self.audience]);
        decode::<serde_json::Value>(token, &key, &validation).map_err(VerifyError::Invalid)?;
        Ok(())
    }

    async fn cached_key(&self, kid: &str) -> Option<DecodingKey> {
        self.cache.read().await.keys.get(kid).cloned()
    }
}

/// Why a token was rejected. Logged at debug server-side; the client
/// only ever sees a bare 401.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("malformed token header: {0}")]
    Header(jsonwebtoken::errors::Error),
    #[error("token header carries no kid")]
    MissingKid,
    #[error("kid not present in the team JWKS")]
    UnknownKid,
    #[error("JWKS refresh failed: {0}")]
    Refresh(anyhow::Error),
    #[error("signature/claim validation failed: {0}")]
    Invalid(jsonwebtoken::errors::Error),
}

/// axum middleware: reject any request whose `Cf-Access-Jwt-Assertion`
/// is absent or fails [`JwtVerifier::verify`]. Applied outermost, so
/// unauthenticated requests never reach body buffering, the
/// concurrency budget, or rmcp.
pub async fn require_access_jwt(
    State(verifier): State<JwtVerifier>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = request
        .headers()
        .get(ACCESS_JWT_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            tracing::debug!("request rejected: no {ACCESS_JWT_HEADER} header");
            StatusCode::UNAUTHORIZED
        })?;

    verifier.verify(token).await.map_err(|e| {
        tracing::debug!(error = %e, "request rejected: Access JWT failed verification");
        StatusCode::UNAUTHORIZED
    })?;

    Ok(next.run(request).await)
}
