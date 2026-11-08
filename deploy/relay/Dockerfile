# Multi-stage build for the GrinMessenger relay.
# Pin the builder image by digest for a reproducible production build.
FROM rust:1.99.1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p flint-gateway --bin flint-relay

FROM debian:bookworm-slim
RUN useradd --system --home /data --shell /usr/sbin/nologin flint \
    && install -d -o flint -g flint -m 0750 /data
COPY --from=build /src/target/release/flint-relay /usr/local/bin/flint-relay
USER flint
EXPOSE 18080
VOLUME ["/data"]
ENTRYPOINT ["/usr/local/bin/flint-relay"]
CMD ["--listen", "127.0.0.1:18080", "--store", "/data"]
