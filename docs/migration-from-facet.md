# Migration from dsdk-facet-rs

Siglet was developed inside the [dsdk-facet-rs](https://github.com/eclipse-dataplane-core/dsdk-facet-rs) workspace up
to `siglet-v0.1.6`. It was moved to this repository on 2026-11-06 so it can build, test, and release on its own,
against the published facet crates. This note records what moved, what changed during the move, and how the result was
verified.

## Summary

|                     | Before (dsdk-facet-rs)                               | After (this repo)                                   |
|---------------------|------------------------------------------------------|-----------------------------------------------------|
| Facet dependencies  | `path = "../crates/…"`                               | crates.io `0.1.1`                                   |
| Release tags        | `siglet-vX.Y.Z` (last: `siglet-v0.1.6`)              | `vX.Y.Z` (first: `v0.1.7`)                          |
| Container image     | `ghcr.io/eclipse-dataplane-core/dsdk-facet-rs/siglet` | `ghcr.io/eclipse-dataplane-core/siglet`             |
| E2E test package    | `dsdk-facet-e2e-tests`                               | `siglet-e2e-tests`                                  |
| Release workflow    | `.github/workflows/release-siglet.yml`               | `.github/workflows/release.yml`                     |
| `DEPENDENCIES.txt`  | Refreshed by release-plz's prepare workflow          | Refreshed by `.github/workflows/dependencies.yml`   |

Images and GitHub Releases for `0.1.6` and earlier remain in dsdk-facet-rs and were not moved.

## What moved

Imported with full history from dsdk-facet-rs `main` at `5b0a2e0`:

- `siglet/`, `siglet-tck/`, `e2e/`
- `docs/siglet.md`, `docs/security-assessment-siglet.md`
- `.github/workflows/release-siglet.yml`, `.github/workflows/docker-publish.yaml`

The history was extracted with `git filter-branch` (index filter over the paths above, empty commits pruned) and merged
into this repository with `--allow-unrelated-histories` (merge commit `b927984`). The extraction was checked against
the source:

- the 52 extracted commits match the source's log for those paths (subject, author, date);
- the trees of all seven paths are identical at the source `HEAD`;
- the trees at every `siglet-v0.1.0`…`siglet-v0.1.6` tag are identical to the originals.

Also copied from dsdk-facet-rs: `LICENSE`, `rustfmt.toml`, `.gitignore`, `.dockerignore`, `.config/nextest.toml`,
`.github/dependabot.yml`, `.github/workflows/rust.yml`, `scripts/check-dependencies.sh`, `scripts/dash-common.sh`,
`scripts/request-dependency-review.sh`, and `Cargo.lock`.

## What stayed in dsdk-facet-rs

The Vault sidecar e2e suite tests `dsdk-facet-hashicorp-vault` only, not siglet, so it was dropped here:

- `e2e/src/tests/vault_sidecar.rs`
- `e2e/test-client/` (the `vault-test` binary and its Dockerfile)
- `e2e/scripts/build-and-load-image.sh`
- `e2e/manifests/test-pod.yaml`

To make the move possible, `dsdk-facet-testcontainers` was made publishable and released to crates.io as `0.1.1`
(dsdk-facet-rs #128), because siglet's tests depend on it.

## Changes made during the move

**Build**

- New root `Cargo.toml`: members `siglet`, `siglet-tck`, `e2e`; workspace version `0.1.7`; only the 19 workspace
  dependencies these crates use, at the same versions as in dsdk-facet-rs.
- Facet path dependencies replaced with crates.io `0.1.1` in `siglet/Cargo.toml` and `e2e/Cargo.toml`.
- `Cargo.lock` carried over. No third-party crate changed version; only the facet crates switched from path to
  registry. In particular tokio stays at 1.53.1, because tokio ≥ 1.54 panics pingora 0.9.0, which is still pulled in
  through `dsdk-facet-core`.

**E2E**

- Package renamed to `siglet-e2e-tests` (Cargo.toml, Makefile, README, scripts, `lib.rs`, nextest config).
- References to the Vault sidecar suite removed (Makefile `build` target, `setup.sh`, README, `tests/mod.rs`).
- Two existing test bugs fixed. Both already failed in dsdk-facet-rs and were not caused by the move:
  - The prepare and start payloads used `processId`/`transferType`. `dataplane-sdk` 0.1.3 requires
    `dataFlowId`/`profile`, so the requests were rejected with `422`.
  - `/tokens/verify` was called as the consumer. Since fix F6 (see `security-assessment-siglet.md`), verify only
    returns claims for tokens issued for the caller's own participant context, so it returned `401`. It is now called
    as the provider. This also fixes the stale-token rotation check, which had been passing without actually testing
    revocation.

**CI and release**

- `docker-publish.yaml`: triggers on `v*` tags, and `GHCR_IMAGE` is now `ghcr.io/${{ github.repository }}`.
- `release.yml` (was `release-siglet.yml`): creates `v<version>` tags.
- New `dependencies.yml`: runs the Eclipse Dash License Tool and pushes a `dependencies` branch with the refreshed
  `DEPENDENCIES.txt`.
- `.config/nextest.toml`: facet-only test overrides removed.
- `.github/dependabot.yml`: ignores tokio ≥ 1.54 (see above).
- `scripts/dash-common.sh`: `ECLIPSE_PROJECT_REPO` points to this repository.
- release-plz workflows not copied: siglet is not published to crates.io.

**Docs**

- `docs/siglet.md`: issued tokens are Ed25519 (`EdDSA`) only, not "Ed25519 or RSA", and the transit key must be
  Ed25519. The `/tokens/verify` route table and admin-scope text now describe the F6 participant binding.
- `README.md` written for the standalone repository.

**`DEPENDENCIES.txt`**

The initial file was derived from dsdk-facet-rs's reviewed list: 497 of siglet's 501 runtime dependencies, all
approved. The remaining four are the facet crates themselves (`dsdk-facet-core`, `-hashicorp-vault`, `-postgres`,
`-testcontainers` at `0.1.1`). They were path dependencies before, which the Dash script skips. They are filled in by
the first run of `dependencies.yml`.

## Verification

**Content parity.** dsdk-facet-rs `main` at `fe80d64` was compared file by file (by blob hash) with this repository's
`main` at `1246572`. The only facet commit after the import snapshot (#128) touches none of the moved paths.

| Result                                       | Files |
|----------------------------------------------|-------|
| In dsdk-facet-rs                             | 86    |
| In this repository                           | 80    |
| Byte-identical                               | 69    |
| Only in dsdk-facet-rs (Vault sidecar suite)  | 6     |
| Different, only by the changes listed above  | 11    |

All of `siglet/src`, `siglet/tests`, both Dockerfiles, `siglet-tck/`, and the e2e fixtures and manifests are
byte-identical. Of the copied repository files, `rust.yml`, `.dockerignore`, `.gitignore`, `rustfmt.toml`, `LICENSE`,
`check-dependencies.sh` and `request-dependency-review.sh` are identical; the others differ only as listed above.
Nothing in this repository references files outside it (no `include_str!`/`include_bytes!`, `../crates`, or `assets/`
paths).

**Build and tests.**

- `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo check --workspace --all-targets --all-features` pass.
- 343 siglet unit tests and 33 container-free integration tests pass.
- E2E in Kind: all 4 tests pass (`test_siglet_deployment_and_health`, `test_pull_operations`,
  `test_signaling_auth_rejects_invalid_tokens`, `test_token_api_auth_rejects_invalid_tokens`).
- CI on `main` (`1246572`): `Rust` and `Build and Publish Docker Siglet Image` (amd64 + arm64) pass.

## Remaining steps

1. Run **Refresh Dependencies** and merge its PR, filing IP reviews for any flagged dependency.
2. Release `0.1.7` with **Release Siglet**.
3. Remove siglet from dsdk-facet-rs: `siglet/`, `siglet-tck/`, the siglet e2e suite, the siglet docs, and the siglet
   workflows. The Vault sidecar suite stays there. Add a pointer to this repository and the new image location in
   facet's README.
