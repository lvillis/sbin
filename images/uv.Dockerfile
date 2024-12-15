FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq

RUN COMPOSE_VERSION=$(curl -s https://api.github.com/repos/docker/compose/releases/latest | jq -r .tag_name) && \
    echo "Latest uv version: $COMPOSE_VERSION" && \
    curl -L "https://github.com/docker/compose/releases/download/${COMPOSE_VERSION}/uv-linux-x86_64" -o /uv && \
    chmod +x /uv

FROM scratch

COPY --from=downloader /uv /usr/local/bin/uv

ENTRYPOINT ["/usr/local/bin/uv"]