# build stage：原生 arm64 執行，輸出 x86_64 binary，並鎖定 glibc 2.34（= Alma 9）
FROM --platform=$BUILDPLATFORM docker.io/library/rust:1-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends python3-pip \
 && pip3 install --break-system-packages ziglang \
 && cargo install --locked cargo-zigbuild \
 && rustup target add x86_64-unknown-linux-gnu
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo zigbuild --release --locked --bins --target x86_64-unknown-linux-gnu.2.34

# runtime stage：與生產環境相同的 OS
FROM docker.io/almalinux/9-minimal
RUN microdnf update -y && microdnf clean all
COPY --from=build /src/target/x86_64-unknown-linux-gnu/release/feed \
                  /src/target/x86_64-unknown-linux-gnu/release/sink /usr/local/bin/
USER 10001:10001
