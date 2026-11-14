# Builds every Red Hive backend binary once; each compose service runs one of them.
#   docker build -f deployment/docker/Dockerfile -t redhive/backend .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
# default-members = every backend service, the module backends and the CLI (the desktop client is excluded)
RUN cargo build --release --locked \
 && mkdir /out && find target/release -maxdepth 1 -type f -name 'redhive-*' -perm -u+x -exec cp {} /out/ \;

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 redhive
COPY --from=build /out/ /usr/local/bin/
COPY modules /opt/redhive/modules
ENV REDHIVE__MODULES_DIR=/opt/redhive/modules \
    REDHIVE__LOG_FORMAT=json \
    REDHIVE__PORT=8080
USER redhive
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=3s --retries=5 CMD curl -fs http://localhost:8080/health/live || exit 1
