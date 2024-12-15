FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN EZA_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/sharkdp/bat/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $EZA_VERSION" && \
    ASSET_NAME="bat-${EZA_VERSION}-x86_64-unknown-linux-musl.tar.gz" && \
    ASSET_URL=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/sharkdp/bat/releases/latest | \
    jq -r --arg NAME "$ASSET_NAME" '.assets[] | select(.name == $NAME) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o bat.tar.gz && \
    tar -zxvf bat.tar.gz -C /usr/local/bin/ && \
    mv -v /usr/local/bin/bat-*-x86_64-unknown-linux-musl/bat /usr/local/bin/bat && \
    chmod +x /usr/local/bin/bat && \
    rm bat.tar.gz

FROM scratch

COPY --from=downloader /usr/local/bin/bat /usr/local/bin/bat

ENTRYPOINT ["/usr/local/bin/bat"]
