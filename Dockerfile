# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /app
ENV SQLX_OFFLINE=true

# Dependency caching: build with dummy sources first, only manifests are copied.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release && rm -rf src

COPY .sqlx .sqlx
COPY migrations migrations
COPY templates templates
COPY src src
RUN touch src/main.rs src/lib.rs && cargo build --release

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home kasse
WORKDIR /app
COPY --from=build /app/target/release/kasse /app/kasse
COPY static /app/static
USER kasse
EXPOSE 8080
CMD ["/app/kasse"]
