FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN EZA_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/Netflix/bpftop/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $EZA_VERSION" && \
    ASSET_URL=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/Netflix/bpftop/releases/latest | \
    jq -r --arg NAME "bpftop-x86_64-unknown-linux-gnu" '.assets[] | select(.name == $NAME) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o /usr/local/bin/bpftop && \
    chmod +x /usr/local/bin/bpftop

FROM scratch

COPY --from=downloader /usr/local/bin/bpftop /usr/local/bin/bpftop

ENTRYPOINT ["/usr/local/bin/bpftop"]
