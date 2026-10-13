---
name: docker-setup
description: Use when creating or changing Dockerfile, docker-compose, environment config, or database startup.
---

# Docker-Regeln

## Ziel
`docker compose up -d` startet alles. Keine manuellen Schritte außer `.env` anlegen.

## Dockerfile
- Multi-stage: Build-Stage mit Rust (Ziel `x86_64-unknown-linux-musl`), Runtime-Stage `FROM scratch`.
- Nur das statische Binary und `static/` ins Runtime-Image (ca. 11 MB).
- Nicht als root laufen (`USER 10001:10001`).
- Nur x86_64, kein arm64.
- Dependency-Caching: erst `Cargo.toml` und `Cargo.lock` kopieren, dann bauen.

## docker-compose.yml
- Service `db`: `postgres:16-alpine`, Volume für Daten, Healthcheck mit `pg_isready`.
- Service `app`: `depends_on` mit `condition: service_healthy`.
- Kein eingebauter Proxy: die App veröffentlicht `${APP_PORT:-8080}`, HTTPS macht der Reverse Proxy des Nutzers (Nginx Proxy Manager).
- Keine Ports der Datenbank nach außen.
- Image-Zeile im Immich-Stil: `ghcr.io/einniki/kasse:${KASSE_VERSION:-dev}`.
- Zugangsdaten aus `.env`, nie hart im Compose-File.

## Konfiguration
- `.env.example` im Repo mit allen Variablen, `.env` in `.gitignore`.
- Erster Admin-Benutzer über `ADMIN_USER` und `ADMIN_PASSWORD` beim ersten Start, nur wenn noch kein User existiert.

## Migrationen
- Beim App-Start automatisch ausführen (`sqlx::migrate!`).
