# 一個 image 放兩個 binary（feed / sink），保證兩邊用同一份 ring layout 編譯
FROM docker.io/library/rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# --locked：依賴版本必須與 commit 了的 Cargo.lock 完全一致，防止 build 時偷偷拉到新版本
RUN cargo build --release --locked --bins

# runtime 與 build 用同一個 Debian 版本，避免 glibc 版本不合
FROM docker.io/library/debian:bookworm-slim
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin app
COPY --from=build /src/target/release/feed /src/target/release/sink /usr/local/bin/
# 固定數字 UID：shared volume 的擁有者要與它一致（見 compose.yaml 的 uid=10001）
USER 10001:10001
