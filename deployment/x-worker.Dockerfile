# syntax=docker/dockerfile:1
FROM rust:1.93-bookworm@sha256:1d33950f982ca6411f5e0ee4850be46e03f066f1a9efaeb41922a0e59497c9c2 AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
RUN CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo build --release --locked -p supa-runtime --bin supa-x-autonomy --bin supa-x-mock-worker

FROM postgres:18-bookworm@sha256:02fb3245e038f33536cf13bc6c4e59f21d07b0cea3c1fe1c07c6e8aa8f083bcc AS base
RUN apt-get update && apt-get install -y --no-install-recommends python3 tini ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 supa-x && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin supa-x \
    && install -d -o 10001 -g 10001 -m 0700 /etc/supa-agent/x-worker /run/credentials/supa-x-autonomy.service \
    && install -d -m 0755 /opt/supa-x/bin /opt/supa-x/sql
COPY deployment/x-worker-entrypoint.py /opt/supa-x/entrypoint.py
ENV SUPA_X_WORKER_MODE=paused PORT=8080
USER 10001:10001
WORKDIR /opt/supa-x
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --start-period=60s --retries=3 CMD ["python3", "-c", "import os,urllib.request; urllib.request.urlopen('http://127.0.0.1:'+os.environ.get('PORT','8080')+'/healthz',timeout=2).read()"]
ENTRYPOINT ["/usr/bin/tini", "--", "python3", "/opt/supa-x/entrypoint.py"]

# Separately selected live target; never copies mock SQL or a mock executable.
FROM base AS live
COPY --from=build /app/target/release/supa-x-autonomy /opt/supa-x/bin/supa-x-autonomy
COPY deployment/x-worker-live.mode /opt/supa-x/bundle-mode

# Default/rehearsal target has no live dispatcher or real account authorization.
FROM base AS mock
COPY --from=build /app/target/release/supa-x-mock-worker /opt/supa-x/bin/supa-x-mock-worker
COPY deployment/x-worker-mock.mode /opt/supa-x/bundle-mode
COPY deployment/x-posting-foundation.sql deployment/ecosystem-social.sql deployment/social-review.sql deployment/social-autonomy.sql deployment/social-first-post.sql deployment/x-worker-mock.sql /opt/supa-x/sql/
