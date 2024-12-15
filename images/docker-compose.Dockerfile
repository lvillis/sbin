FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq

RUN COMPOSE_VERSION=$(curl -s https://api.github.com/repos/docker/compose/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $COMPOSE_VERSION" && \
    curl -L "https://github.com/docker/compose/releases/download/${COMPOSE_VERSION}/docker-compose-linux-x86_64" -o /docker-compose && \
    chmod +x /docker-compose

FROM scratch

COPY --from=downloader /docker-compose /usr/local/bin/docker-compose

ENTRYPOINT ["/usr/local/bin/docker-compose"]