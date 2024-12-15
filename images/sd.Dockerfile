FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN EZA_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/chmln/sd/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $EZA_VERSION" && \
    ASSET_NAME="sd-${EZA_VERSION}-x86_64-unknown-linux-musl.tar.gz" && \
    ASSET_URL=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/chmln/sd/releases/latest | \
    jq -r --arg NAME "$ASSET_NAME" '.assets[] | select(.name == $NAME) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o sd.tar.gz && \
    tar -zxvf sd.tar.gz -C /usr/local/bin/ && \
    mv -v /usr/local/bin/sd-*-x86_64-unknown-linux-musl/sd /usr/local/bin/sd && \
    chmod +x /usr/local/bin/sd && \
    rm sd.tar.gz

FROM scratch

COPY --from=downloader /usr/local/bin/sd /usr/local/bin/sd

ENTRYPOINT ["/usr/local/bin/sd"]
