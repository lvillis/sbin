FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN EZA_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/ClementTsang/bottom/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $EZA_VERSION" && \
    ASSET_NAME="bottom_x86_64-unknown-linux-musl.tar.gz" && \
    ASSET_URL=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/ClementTsang/bottom/releases/latest | \
    jq -r --arg NAME "$ASSET_NAME" '.assets[] | select(.name == $NAME) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o bottom.tar.gz && \
    tar -zxvf bottom.tar.gz -C /usr/local/bin/ && \
    chmod +x /usr/local/bin/btm && \
    rm bottom.tar.gz

FROM scratch

COPY --from=downloader /usr/local/bin/btm /usr/local/bin/btm

ENTRYPOINT ["/usr/local/bin/bottom"]
