FROM debian:buster

RUN apt-get update && apt-get install -y "libssl1.1" ca-certificates

WORKDIR /opt/status-page

COPY target/release/wavy-status-page .
COPY index.html .
RUN mkdir status

CMD ["/opt/status-page/wavy-status-page"]
