FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN EZA_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/eza-community/eza/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $EZA_VERSION" && \
    ASSET_URL=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/eza-community/eza/releases/latest | \
    jq -r --arg NAME "eza_x86_64-unknown-linux-musl.tar.gz" '.assets[] | select(.name == $NAME) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o eza.tar.gz && \
    tar -zxvf eza.tar.gz -C /usr/local/bin/ && \
    chmod +x /usr/local/bin/eza && \
    rm eza.tar.gz

FROM scratch

COPY --from=downloader /usr/local/bin/eza /usr/local/bin/eza

ENTRYPOINT ["/usr/local/bin/eza"]
