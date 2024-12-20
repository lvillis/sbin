FROM alpine:latest AS downloader

RUN apk add --no-cache curl jq tar

RUN EZA_VERSION=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/hengyoush/kyanos/releases/latest | jq -r .tag_name) && \
    echo "Latest version: $EZA_VERSION" && \
    ASSET_NAME="kyanos_${EZA_VERSION#v}_linux_amd64.tar.gz" && \
    ASSET_URL=$(curl -s -H "User-Agent: docker-build" https://api.github.com/repos/hengyoush/kyanos/releases/latest | \
    jq -r --arg NAME "$ASSET_NAME" '.assets[] | select(.name == $NAME) | .browser_download_url') && \
    echo "Download url: $ASSET_URL" && \
    curl -L "$ASSET_URL" -o kyanos.tar.gz && \
    tar -zxvf kyanos.tar.gz -C /usr/local/bin/ && \
    mv -v /usr/local/bin/kyanos /usr/local/bin/kyanos && \
    chmod +x /usr/local/bin/kyanos && \
    rm kyanos.tar.gz

FROM scratch

COPY --from=downloader /usr/local/bin/kyanos /usr/local/bin/kyanos

ENTRYPOINT ["/usr/local/bin/kyanos"]
