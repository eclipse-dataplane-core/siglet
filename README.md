# siglet

Siglet is a Data Plane built on the [Eclipse Data Plane SDK for Rust](https://github.com/eclipse-dataplane-core/dataplane-sdk-rust).
It implements Data Plane Signaling and issues Ed25519-signed access tokens backed by HashiCorp Vault, with PostgreSQL for
persistence. It is built from the [Facet-RS](https://github.com/eclipse-dataplane-core/dsdk-facet-rs) building blocks.

See [docs/siglet.md](docs/siglet.md) for the runtime reference: architecture, token shape, API authentication,
configuration, and Vault setup.

## Repository layout

| Path          | Contents                                                                 |
|---------------|--------------------------------------------------------------------------|
| `siglet/`     | The siglet binary and library                                            |
| `siglet-tck/` | Runs the Data Plane Signaling (DPS) TCK against an in-process siglet     |
| `e2e/`        | End-to-end tests against siglet deployed in a Kind cluster with Vault    |
| `docs/`       | Runtime documentation and security assessment                            |

## Container image

Images are published to `ghcr.io/eclipse-dataplane-core/siglet`:

- `latest`, `X.Y.Z`, `X.Y` for releases
- `dev` for the current `main`

Images for `0.1.6` and earlier were published from the Facet-RS repository as
`ghcr.io/eclipse-dataplane-core/dsdk-facet-rs/siglet`. See [docs/migration-from-facet.md](docs/migration-from-facet.md)
for what changed in the move.

```bash
docker build -f siglet/Dockerfile -t siglet:local .
```

## Building

`cmake` is required (a transitive dependency builds native code).

```bash
cargo build --release --bin siglet
```

## Running tests

```bash
cargo test --all-features
```

Integration tests use [testcontainers](https://testcontainers.com/), so a running Docker daemon is required.
[cargo-nextest](https://nexte.st/) is supported and recommended; `.config/nextest.toml` tunes it for container-based
tests:

```bash
cargo install cargo-nextest --locked
cargo nextest run --all-features
```

### E2E tests

E2E tests require Docker, Kind, and kubectl. See [e2e/README.md](e2e/README.md) for details.

```bash
cd e2e
make test       # bootstraps the cluster on first run, builds the image, runs the tests
make cleanup    # tear down the Kind cluster
```

## Releasing

Releases are cut manually with the **Release Siglet** workflow (`.github/workflows/release.yml`), giving a
`MAJOR.MINOR.PATCH` version. It runs the tests and the Eclipse Dash license check, then builds and pushes the
multi-arch image, and only then creates the `vX.Y.Z` tag and GitHub Release.

`DEPENDENCIES.txt` is refreshed with the **Refresh Dependencies** workflow, which pushes a `dependencies` branch to open
a PR from. Dependencies that need an Eclipse IP review can be filed with `scripts/request-dependency-review.sh`.

## License

[Apache-2.0](LICENSE)
