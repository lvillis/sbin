FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN EZA_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/libbpf/bpftool/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $EZA_VERSION" && \
    ASSET_NAME="bpftool-v7.5.0-amd64.tar.gz" && \
    ASSET_URL=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/libbpf/bpftool/releases/latest | \
    jq -r --arg NAME "$ASSET_NAME" '.assets[] | select(.name == $NAME) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o bpftool.tar.gz && \
    tar -zxvf bpftool.tar.gz -C /usr/local/bin/ && \
    chmod +x /usr/local/bin/bpftool && \
    rm bpftool.tar.gz

FROM scratch

COPY --from=downloader /usr/local/bin/bpftool /usr/local/bin/bpftool

ENTRYPOINT ["/usr/local/bin/bpftool"]
