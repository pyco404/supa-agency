# syntax=docker/dockerfile:1
FROM rust:1.93-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/domain/Cargo.toml crates/domain/Cargo.toml
COPY crates/domain/src crates/domain/src
COPY crates/persistence/Cargo.toml crates/persistence/Cargo.toml
COPY crates/persistence/src crates/persistence/src
COPY crates/providers/Cargo.toml crates/providers/Cargo.toml
COPY crates/providers/src crates/providers/src
COPY crates/capabilities/Cargo.toml crates/capabilities/Cargo.toml
COPY crates/capabilities/src crates/capabilities/src
COPY crates/runtime/Cargo.toml crates/runtime/Cargo.toml
COPY crates/runtime/src crates/runtime/src
COPY crates/api/Cargo.toml crates/api/Cargo.toml
COPY crates/api/src crates/api/src
COPY migrations migrations
RUN cargo build --release --locked -p supa-api --bin supa-api --bin supa-staging-schema

# Debian's signed Trixie security archive supplies the maintained zlib fix.
# Download only: no build tools or package manager enter the final runtime.
FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2 AS zlib-security
RUN test "$(dpkg --print-architecture)" = amd64 \
    && apt-get update \
    && mkdir /tmp/zlib-security \
    && cd /tmp/zlib-security \
    && apt-get download zlib1g=1:1.3.dfsg+really1.3.1-1+deb13u1 \
    && set -- zlib1g_*_amd64.deb \
    && test "$#" -eq 1 \
    && mv -- "$1" zlib1g.deb \
    && echo '8c27c342230503b73dfde629642c3b352e87b554e06d60cf58772f236a305ceb  zlib1g.deb' | sha256sum --check --strict \
    && dpkg-deb --extract zlib1g.deb /overlay \
    && dpkg-deb --control zlib1g.deb /control \
    && mkdir -p /overlay/var/lib/dpkg/status.d \
    && cp /control/control /overlay/var/lib/dpkg/status.d/zlib1g \
    && cp /control/md5sums /overlay/var/lib/dpkg/status.d/zlib1g.md5sums

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1 AS runtime
# Official minimal glibc runtime: no package manager, shell or account utilities.
# Runtime ELF/library, CA, non-root and vulnerability checks remain mandatory in CI.
COPY --from=zlib-security /overlay/ /
ENV SUPA_PRIVATE_STAGING=true SUPA_BIND=[::]:8080 PORT=8080 SUPA_PROVIDER=offline SUPA_API_SCHEDULER_ENABLED=false SUPA_WALLET_ENABLED=false
COPY --from=build /app/target/release/supa-api /usr/local/bin/supa-api
COPY --from=build /app/target/release/supa-staging-schema /usr/local/bin/supa-staging-schema
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/supa-api"]
