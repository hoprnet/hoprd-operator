FROM debian:stable-slim
LABEL name="hoprd metrics" \
      maintainer="tech@hoprnet.org" \
      vendor="HOPR" \
      summary="Hoprd metrics wrapper" \
      description="Wraps the hoprd metrics script in a lighttpd server"

# musl toolchain for static binaries
RUN apt update && \
    apt install --no-install-recommends -y curl lighttpd && \
    apt-get autoremove -y && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*
ENV HOPRD_API_TOKEN=MY_SECRET_API_TOKEN
EXPOSE 8080

RUN mkdir -p /app/hoprd-metrics /var/www/cgi-bin/
COPY ./metrics.sh /var/www/cgi-bin/
COPY ./lighttpd.conf /etc/lighttpd/lighttpd.conf
COPY ./entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /var/www/cgi-bin/metrics.sh /usr/local/bin/entrypoint.sh
RUN chown -R www-data:www-data /var/www/cgi-bin

ENTRYPOINT [ "/usr/local/bin/entrypoint.sh" ]