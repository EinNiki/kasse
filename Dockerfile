# syntax=docker/dockerfile:1
# Static musl binary in an empty image: no OS, no shell, nothing but the app (a few MB).
FROM rust:1-bookworm AS build
WORKDIR /app
ENV SQLX_OFFLINE=true
RUN rustup target add x86_64-unknown-linux-musl

# Dependency caching: build with dummy sources first, only manifests are copied.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release --target x86_64-unknown-linux-musl && rm -rf src

COPY .sqlx .sqlx
COPY migrations migrations
COPY templates templates
COPY src src
RUN touch src/main.rs src/lib.rs && cargo build --release --target x86_64-unknown-linux-musl

FROM scratch
WORKDIR /app
COPY --from=build /app/target/x86_64-unknown-linux-musl/release/kasse /app/kasse
COPY static /app/static
USER 10001:10001
EXPOSE 8080
CMD ["/app/kasse"]
