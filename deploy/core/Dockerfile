# vyreon-core: the service and its admin commands in one image. Build from the repository root:
#   docker build -f deploy/core/Dockerfile -t vyreon-core:$(git rev-parse --short HEAD) .
# The seed files (the 64-member profile, its Intrinio ticker history, the engine's trading
# sessions) are in the image so a new database can be loaded from inside the cluster.
FROM rust:1.98-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo build --release --locked -p vyreon-core

FROM debian:bookworm-slim
RUN apt-get update -q && apt-get install -yq --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 vyreon && mkdir -p /var/spool/vyreon-core && chown vyreon /var/spool/vyreon-core
COPY --from=build /src/target/release/vyreon-core /usr/local/bin/vyreon-core
COPY crates/test-support/fixtures/engine-v1-PROFILE.json crates/test-support/fixtures/engine-v1-sessions.json /opt/vyreon-core/seed/
COPY crates/test-support/fixtures/intrinio-universe /opt/vyreon-core/seed/intrinio-universe
USER vyreon
ENV VYREON_CORE_LISTEN=0.0.0.0:8080 VYREON_CORE_SPOOL=/var/spool/vyreon-core
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/vyreon-core"]
CMD ["serve"]
