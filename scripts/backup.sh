#!/bin/sh
# Writes a compressed PostgreSQL dump to backups/. Usage: ./scripts/backup.sh
# Restore:  gunzip -c backups/<file>.sql.gz | docker compose exec -T db sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"'
set -eu
cd "$(dirname "$0")/.."
mkdir -p backups
umask 077
file="backups/kasse-$(date +%Y%m%d-%H%M%S).sql.gz"
docker compose exec -T db sh -c 'pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB" --clean --if-exists' | gzip > "$file"
echo "Backup written: $file"
