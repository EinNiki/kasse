#!/bin/sh
# Exports the root certificate of the local certificate authority so devices can trust the HTTPS
# address of the cash register. Install the file once on every tablet/phone/PC (see README).
set -eu
cd "$(dirname "$0")/.."
docker compose cp proxy:/data/caddy/pki/authorities/local/root.crt ./kasse-root-ca.crt
echo "Written: kasse-root-ca.crt  (only the public certificate, safe to share)"
