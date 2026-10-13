# syntax=docker/dockerfile:1
# The build always runs natively on the build machine and cross-compiles for the target, so an
# arm64 image (Raspberry Pi) does not need slow CPU emulation. All dependencies are pure Rust.
FROM --platform=$BUILDPLATFORM rust:1-bookworm AS build
ARG TARGETARCH
WORKDIR /app
ENV SQLX_OFFLINE=true

RUN case "$TARGETARCH" in \
      arm64) echo aarch64-unknown-linux-gnu > /rust-target ;; \
      *)     echo x86_64-unknown-linux-gnu  > /rust-target ;; \
    esac \
    && rustup target add "$(cat /rust-target)" \
    && if [ "$TARGETARCH" = arm64 ]; then \
         apt-get update && apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
         && rm -rf /var/lib/apt/lists/*; \
       fi
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc

# Dependency caching: build with dummy sources first, only manifests are copied.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && echo '' > src/lib.rs \
    && cargo build --release --target "$(cat /rust-target)" && rm -rf src

COPY .sqlx .sqlx
COPY migrations migrations
COPY templates templates
COPY src src
RUN touch src/main.rs src/lib.rs && cargo build --release --target "$(cat /rust-target)" \
    && cp "target/$(cat /rust-target)/release/kasse" /kasse

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home kasse
WORKDIR /app
COPY --from=build /kasse /app/kasse
COPY static /app/static
USER kasse
EXPOSE 8080
CMD ["/app/kasse"]
