//  Copyright (c) 2026 Metaform Systems, Inc
//
//  This program and the accompanying materials are made available under the
//  terms of the Apache License, Version 2.0 which is available at
//  https://www.apache.org/licenses/LICENSE-2.0
//
//  SPDX-License-Identifier: Apache-2.0
//
//  Contributors:
//       Metaform Systems, Inc. - initial API and implementation
//

//! Siglet against a real Vault, once per supported signing key type.
//!
//! Each case configures `vault.signing_key_type` (and optionally `token.signing_algorithm`),
//! assembles the full runtime with [`assemble_memory`], which creates Siglet's transit key in
//! Vault, and then:
//! - checks Vault created the key with the configured type,
//! - issues an access token and checks the header `alg`/`kid`,
//! - fetches the public `/keys` JWKS endpoint and verifies the token with plain `jsonwebtoken`
//!   using the published JWK (the path a data plane takes),
//! - verifies the token through `/tokens/verify` and `TokenManager::validate_token`,
//! - checks that the same token with its header `alg` swapped to another algorithm is rejected.
//!
//! Requires a Docker-compatible runtime (via `dsdk-facet-testcontainers`).

#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use dsdk_facet_core::context::ParticipantContext;
use dsdk_facet_testcontainers::{
    keycloak::setup_keycloak_container, utils::create_network, vault::setup_vault_container,
};
use serde_json::{Value, json};
use siglet::assembly::{DEFAULT_TOKEN_ISSUER, assemble_memory};
use siglet::config::{
    KeyType, ManagementApiAuthConfig, SigletConfig, SignalingAuthConfig, SigningAlgorithm, TokenApiAuthConfig,
    TokenConfig, VaultAuth, VaultConfig,
};
use siglet::server::auth::AuthLayer;
use std::collections::HashMap;
use tower::ServiceExt;

/// Siglet's own transit key (`signing-{SIGLET_PC_ID}`), which the access-token generator uses.
const SIGNING_KEY_NAME: &str = "signing-siglet";
const CONSUMER_PC_ID: &str = "consumer-pc";
const CONSUMER_AUDIENCE: &str = "did:web:consumer.example.com";

struct Case {
    key_type: KeyType,
    /// `token.signing_algorithm`; `None` means the key type's default.
    configured: Option<SigningAlgorithm>,
    expected: SigningAlgorithm,
}

const CASES: &[Case] = &[
    Case {
        key_type: KeyType::Ed25519,
        configured: None,
        expected: SigningAlgorithm::EdDSA,
    },
    Case {
        key_type: KeyType::EcdsaP256,
        configured: None,
        expected: SigningAlgorithm::ES256,
    },
    Case {
        key_type: KeyType::EcdsaP384,
        configured: None,
        expected: SigningAlgorithm::ES384,
    },
    Case {
        key_type: KeyType::Rsa2048,
        configured: None,
        expected: SigningAlgorithm::RS256,
    },
    Case {
        key_type: KeyType::Rsa2048,
        configured: Some(SigningAlgorithm::PS256),
        expected: SigningAlgorithm::PS256,
    },
    Case {
        key_type: KeyType::Rsa3072,
        configured: Some(SigningAlgorithm::PS384),
        expected: SigningAlgorithm::PS384,
    },
];

#[tokio::test]
async fn siglet_signs_and_publishes_every_vault_key_type() {
    let network = create_network().await;
    let (keycloak_setup, _keycloak_container) = setup_keycloak_container(&network).await;
    let jwks_url = format!(
        "{}/realms/master/protocol/openid-connect/certs",
        keycloak_setup.keycloak_internal_url
    );
    let (vault_url, root_token, _vault_container) =
        setup_vault_container(&network, &jwks_url, &keycloak_setup.keycloak_container_id).await;

    // Siglet reads the Vault token from a file; use the dev root token.
    let token_file = std::env::temp_dir().join(format!("siglet-vault-alg-test-{}", uuid::Uuid::new_v4()));
    std::fs::write(&token_file, &root_token).unwrap();

    let http = reqwest::Client::new();

    for case in CASES {
        let label = format!("{} / {}", case.key_type, case.expected);

        // The generator always signs with `signing-siglet`, so each case starts without that key
        // and lets Siglet create it with the configured type.
        delete_transit_key(&http, &vault_url, &root_token, SIGNING_KEY_NAME).await;

        let cfg = SigletConfig {
            vault: VaultConfig {
                url: Some(vault_url.clone()),
                auth: Some(VaultAuth::KubernetesServiceAccount {
                    token: None,
                    token_file: Some(token_file.to_string_lossy().into_owned()),
                }),
                signing_key_type: case.key_type,
                ..VaultConfig::default()
            },
            token: TokenConfig {
                signing_algorithm: case.configured,
                ..TokenConfig::default()
            },
            // The API auth layers are not under test here; the handlers are driven directly.
            signaling_auth: SignalingAuthConfig::Disabled,
            token_api_auth: TokenApiAuthConfig::Disabled,
            management_api_auth: ManagementApiAuthConfig::Disabled,
            ..SigletConfig::default()
        };
        cfg.validate()
            .unwrap_or_else(|e| panic!("{label}: config rejected: {e:?}"));

        let runtime = assemble_memory(&cfg, reqwest::Client::new())
            .await
            .unwrap_or_else(|e| panic!("{label}: assembly failed: {e}"));

        // Siglet created the key with the configured type.
        let key_info = read_transit_key(&http, &vault_url, &root_token, SIGNING_KEY_NAME).await;
        assert_eq!(
            key_info["data"]["type"],
            case.key_type.as_str(),
            "{label}: Vault key type"
        );

        // Issue an access token.
        let consumer = ParticipantContext::builder()
            .id(CONSUMER_PC_ID)
            .identifier(CONSUMER_AUDIENCE)
            .audience(CONSUMER_AUDIENCE)
            .build();
        let pair = runtime
            .token_manager
            .generate_pair(&consumer, CONSUMER_AUDIENCE, HashMap::new(), format!("flow-{}", label))
            .await
            .unwrap_or_else(|e| panic!("{label}: token issuance failed: {e}"));
        let token = pair.token;

        let header = decode_segment(&token, 0);
        assert_eq!(header["alg"], case.expected.as_str(), "{label}: header alg");
        assert_eq!(header["kid"], format!("{}-1", SIGNING_KEY_NAME), "{label}: header kid");
        let kid = header["kid"].as_str().unwrap().to_string();

        let router = runtime.token_api_handler.router(AuthLayer::Disabled);

        // The public JWKS publishes the key with the algorithm Siglet signs with.
        let response = router
            .clone()
            .oneshot(Request::builder().uri("/keys").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{label}: GET /keys");
        let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_slice(&body_bytes(response).await)
            .unwrap_or_else(|e| panic!("{label}: /keys is not a valid JWK set: {e}"));
        let jwk = jwks
            .find(&kid)
            .unwrap_or_else(|| panic!("{label}: kid {kid} missing from /keys"));
        assert_eq!(
            jwk.common.key_algorithm.as_ref().map(|a| a.to_string()).as_deref(),
            Some(case.expected.as_str()),
            "{label}: JWK alg"
        );

        // An external verifier using only the published JWK accepts the token.
        let decoding_key = jsonwebtoken::DecodingKey::from_jwk(jwk).unwrap();
        let mut validation = jsonwebtoken::Validation::new(case.expected.into());
        validation.set_audience(&[CONSUMER_AUDIENCE]);
        validation.set_issuer(&[DEFAULT_TOKEN_ISSUER]);
        let external = jsonwebtoken::decode::<Value>(&token, &decoding_key, &validation)
            .unwrap_or_else(|e| panic!("{label}: jsonwebtoken rejected the token: {e}"));
        assert_eq!(external.claims["sub"], CONSUMER_AUDIENCE, "{label}: sub");

        // Siglet's own verification path accepts it.
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/tokens/verify")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({ "token": token, "audience": CONSUMER_AUDIENCE }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body = body_bytes(response).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "{label}: /tokens/verify: {}",
            String::from_utf8_lossy(&body)
        );
        let claims: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(claims["sub"], CONSUMER_AUDIENCE, "{label}: verified sub");

        runtime
            .token_manager
            .validate_token(Some(CONSUMER_PC_ID), CONSUMER_AUDIENCE, &token)
            .await
            .unwrap_or_else(|e| panic!("{label}: validate_token rejected the token: {e}"));

        // Algorithm confusion: the same signature under a different header `alg` must fail.
        for other in SigningAlgorithm::ALL.iter().filter(|a| **a != case.expected) {
            let forged = with_header_alg(&token, other.as_str());
            assert!(
                runtime
                    .token_manager
                    .validate_token(None, CONSUMER_AUDIENCE, &forged)
                    .await
                    .is_err(),
                "{label}: token accepted with header alg {other}"
            );
        }
        let forged = with_header_alg(&token, "HS256");
        assert!(
            runtime
                .token_manager
                .validate_token(None, CONSUMER_AUDIENCE, &forged)
                .await
                .is_err(),
            "{label}: token accepted with header alg HS256"
        );
    }

    let _ = std::fs::remove_file(&token_file);
}

// ============================================================================
// Helpers
// ============================================================================

async fn body_bytes(response: axum::response::Response) -> Vec<u8> {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

fn decode_segment(token: &str, index: usize) -> Value {
    let segment = token.split('.').nth(index).unwrap();
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segment).unwrap()).unwrap()
}

/// Returns `token` with its header `alg` replaced, keeping payload and signature.
fn with_header_alg(token: &str, alg: &str) -> String {
    let mut header = decode_segment(token, 0);
    header["alg"] = Value::String(alg.to_string());
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
    let rest = token.split_once('.').unwrap().1;
    format!("{}.{}", header, rest)
}

async fn read_transit_key(http: &reqwest::Client, vault_url: &str, token: &str, name: &str) -> Value {
    let response = http
        .get(format!("{}/v1/transit/keys/{}", vault_url, name))
        .header("X-Vault-Token", token)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "reading transit key {name} failed");
    response.json().await.unwrap()
}

/// Deletes a transit key if it exists. Vault requires `deletion_allowed` before a delete.
async fn delete_transit_key(http: &reqwest::Client, vault_url: &str, token: &str, name: &str) {
    let key_url = format!("{}/v1/transit/keys/{}", vault_url, name);
    let exists = http
        .get(&key_url)
        .header("X-Vault-Token", token)
        .send()
        .await
        .unwrap()
        .status()
        .is_success();
    if !exists {
        return;
    }
    let response = http
        .post(format!("{}/config", key_url))
        .header("X-Vault-Token", token)
        .json(&json!({ "deletion_allowed": true }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "enabling deletion of {name} failed");
    let response = http
        .delete(&key_url)
        .header("X-Vault-Token", token)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success(), "deleting transit key {name} failed");
}
