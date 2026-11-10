# syntax=docker/dockerfile:1
FROM rust:1.93-bookworm@sha256:1d33950f982ca6411f5e0ee4850be46e03f066f1a9efaeb41922a0e59497c9c2 AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
RUN CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo build --release --locked -p supa-runtime --bin supa-x-autonomy --bin supa-x-mock-worker

# glibc-compatible runtime: no inherited Debian tools, gosu or Go runtime.
# BusyBox /bin/sh is required by PostgreSQL initdb/pg_ctl; no Bash is installed.
FROM cgr.dev/chainguard/wolfi-base@sha256:d13acb327aa77acf2752acea8c08456511a5b8142455cc2fe0f966b960bf64c6 AS base
RUN apk upgrade --no-cache \
    && apk add --no-cache postgresql-18-base=18.6-r7 postgresql-18-client=18.6-r7 \
        python-3.14-runtime=3.14.10_git20261104-r0 tini=0.19.0-r26 ca-certificates-bundle=20261020-r0 busybox=1.38.0-r3 \
    && addgroup -g 10001 supa-x && adduser -D -H -u 10001 -G supa-x -s /sbin/nologin supa-x \
    && mkdir -p /usr/lib/postgresql/18 \
    && ln -s /usr/libexec/postgresql18 /usr/lib/postgresql/18/bin \
    && ln -s /usr/libexec/postgresql18/postgres /usr/bin/postgres \
    && install -d -o 10001 -g 10001 -m 0700 /etc/supa-agent/x-worker /run/credentials/supa-x-autonomy.service \
    && install -d -m 0755 /opt/supa-x/bin /opt/supa-x/sql \
    && apk del --no-cache wolfi-base apk-tools
COPY deployment/x-worker-entrypoint.py /opt/supa-x/entrypoint.py
ENV SUPA_X_WORKER_MODE=paused PORT=8080
USER 10001:10001
WORKDIR /opt/supa-x
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --start-period=60s --retries=3 CMD ["python3", "-c", "import os,urllib.request; urllib.request.urlopen('http://127.0.0.1:'+os.environ.get('PORT','8080')+'/healthz',timeout=2).read()"]
ENTRYPOINT ["/usr/bin/tini", "--", "python3", "/opt/supa-x/entrypoint.py"]
STOPSIGNAL SIGTERM

# Separately selected live target; never copies mock SQL or a mock executable.
FROM base AS live
COPY --from=build /app/target/release/supa-x-autonomy /opt/supa-x/bin/supa-x-autonomy
COPY deployment/x-worker-live.mode /opt/supa-x/bundle-mode

# Default/rehearsal target has no live dispatcher or real account authorization.
FROM base AS mock
COPY --from=build /app/target/release/supa-x-mock-worker /opt/supa-x/bin/supa-x-mock-worker
COPY deployment/x-worker-mock.mode /opt/supa-x/bundle-mode
COPY deployment/x-posting-foundation.sql deployment/ecosystem-social.sql deployment/social-review.sql deployment/social-autonomy.sql deployment/social-first-post.sql deployment/x-worker-mock.sql /opt/supa-x/sql/
