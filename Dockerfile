FROM node:24-slim AS web
RUN npm install -g pnpm@11
WORKDIR /build/web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ .
RUN pnpm build

FROM rust:1-bookworm AS server
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY migrations migrations
COPY .sqlx .sqlx
COPY src src
COPY --from=web /build/web/dist web/dist
ENV SQLX_OFFLINE=true
RUN cargo build --release

FROM debian:bookworm-slim
# the preview worker drives this; unset PREVIEW_CHROMIUM to turn previews off
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      chromium fonts-liberation fonts-noto-color-emoji ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=server /build/target/release/plan-env-md /usr/local/bin/plan-env-md
ENV BIND=0.0.0.0:3000
ENV DATABASE_URL=sqlite:///data/plan-env-md.db
ENV PREVIEW_CHROMIUM=/usr/bin/chromium
VOLUME /data
EXPOSE 3000
CMD ["plan-env-md"]
