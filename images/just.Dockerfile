FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN JUST_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/casey/just/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $JUST_VERSION" && \
    ASSET_URL=$(curl -s https://api.github.com/repos/casey/just/releases/latest | \
    jq -r '.assets[] | select(.name | test("x86_64-unknown-linux-musl\\.tar\\.gz$")) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o just.tar.gz && \
    tar -zxvf just.tar.gz -C /usr/local/bin/ && \
    chmod +x /usr/local/bin/just && \
    rm just.tar.gz

FROM scratch

COPY --from=downloader /usr/local/bin/just /usr/local/bin/just

ENTRYPOINT ["/usr/local/bin/just"]
