# Pinned to bookworm to match the runtime stage: a binary linked against a newer glibc
# will not start on bookworm.
FROM rust:1.94-slim-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# Cache mounts keep the registry and compiled dependencies between builds on the VPS,
# so a code change doesn't recompile every crate. The binary is copied out of the
# cached target dir because cache mounts are not part of the image.
# The VPS (2 cores, 4 GB) also serves other apps: build at lowest priority with one job,
# which leaves a core free and keeps peak memory down.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    nice -n 19 cargo build --release --locked -j1 \
    && cp target/release/discord-market-bot /usr/local/bin/discord-market-bot

FROM debian:bookworm-slim
# sqlite3 is for online backups: see README.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates sqlite3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home marketbot \
    && mkdir /data && chown marketbot:marketbot /data
COPY --from=build /usr/local/bin/discord-market-bot /usr/local/bin/discord-market-bot
USER marketbot
ENV DATABASE_PATH=/data/market.db
VOLUME /data
CMD ["discord-market-bot"]
