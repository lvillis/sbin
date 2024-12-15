FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN EZA_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/bootandy/dust/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $EZA_VERSION" && \
    ASSET_NAME="dust-${EZA_VERSION}-x86_64-unknown-linux-musl.tar.gz" && \
    ASSET_URL=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/bootandy/dust/releases/latest | \
    jq -r --arg NAME "$ASSET_NAME" '.assets[] | select(.name == $NAME) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o dust.tar.gz && \
    tar -zxvf dust.tar.gz -C /usr/local/bin/ && \
    mv -v /usr/local/bin/dust-*-x86_64-unknown-linux-musl/dust /usr/local/bin/dust && \
    chmod +x /usr/local/bin/dust && \
    rm dust.tar.gz

FROM scratch

COPY --from=downloader /usr/local/bin/dust /usr/local/bin/dust

ENTRYPOINT ["/usr/local/bin/dust"]
