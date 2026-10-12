---
name: docker-setup
description: Use when creating or changing Dockerfile, docker-compose, environment config, or database startup.
---

# Docker-Regeln

## Ziel
`docker compose up -d` startet alles. Keine manuellen Schritte außer `.env` anlegen.

## Dockerfile
- Multi-stage: Build-Stage mit Rust, Runtime-Stage mit `debian:bookworm-slim`.
- Nur das fertige Binary und die nötigen Assets ins Runtime-Image.
- Nicht als root laufen (eigener User im Image).
- Dependency-Caching: erst `Cargo.toml` und `Cargo.lock` kopieren, dann bauen.

## docker-compose.yml
- Service `db`: `postgres:16-alpine`, Volume für Daten, Healthcheck mit `pg_isready`.
- Service `app`: `depends_on` mit `condition: service_healthy`.
- Keine Ports der Datenbank nach außen, nur die App auf einem Port.
- Zugangsdaten aus `.env`, nie hart im Compose-File.

## Konfiguration
- `.env.example` im Repo mit allen Variablen, `.env` in `.gitignore`.
- Erster Admin-Benutzer über `ADMIN_USER` und `ADMIN_PASSWORD` beim ersten Start, nur wenn noch kein User existiert.

## Migrationen
- Beim App-Start automatisch ausführen (`sqlx::migrate!`).
