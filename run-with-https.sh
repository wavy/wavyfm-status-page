#!/bin/bash

export PORT=443
export HTTPS=1

certbot certonly --standalone --preferred-challenges http -d status.wavy.fm \
  --non-interactive --agree-tos -m aram.peres@wavy.fm --post-hook "/opt/status-page/wavy-status-page"
