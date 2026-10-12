#!/bin/sh
# Creates .env with strong random passwords and asks for the address the tablets use.
# Usage: ./setup.sh [address]     e.g. ./setup.sh 192.168.1.50
set -eu
cd "$(dirname "$0")"

if [ -e .env ]; then
  echo ".env exists already - not touching it. Delete it first if you want to start over." >&2
  exit 1
fi

random() { head -c 48 /dev/urandom | base64 | tr -dc 'A-Za-z0-9' | head -c "$1"; }

address="${1:-}"
if [ -z "$address" ]; then
  guess=$(hostname -I 2>/dev/null | awk '{print $1}' || true)
  printf "Address the tablets use (IP or name) [%s]: " "${guess:-localhost}"
  read -r address || true
  address="${address:-${guess:-localhost}}"
fi

# Other names that should work as well (without repeating the main address)
aliases=""
for name in localhost 127.0.0.1; do
  [ "$name" = "$address" ] || aliases="${aliases:+$aliases }$name"
done

admin_password=$(random 16)
umask 077
sed -e "s|^POSTGRES_PASSWORD=.*|POSTGRES_PASSWORD=$(random 32)|" \
    -e "s|^APP_DB_PASSWORD=.*|APP_DB_PASSWORD=$(random 32)|" \
    -e "s|^ADMIN_PASSWORD=.*|ADMIN_PASSWORD=${admin_password}|" \
    -e "s|^SITE_ADDRESS=.*|SITE_ADDRESS=${address}|" \
    -e "s|^SITE_ALIASES=.*|SITE_ALIASES=${aliases}|" \
    .env.example > .env

echo
echo ".env written (readable only by you)."
echo "  Address:  https://${address}:8443"
echo "  Login:    admin / ${admin_password}"
echo "Write the login down now, it is only stored in .env. Then start with:  docker compose up -d"
