---
name: rust-axum-stack
description: Use when writing Rust backend code: routes, handlers, database access, templates, or error handling.
---

# Rust-Konventionen

## Struktur
- `src/main.rs`: nur Start, Config, Router-Zusammenbau
- `src/routes/`: ein Modul pro Bereich (auth, stock, sales, stats)
- `src/db/`: alle sqlx-Abfragen, Handler rufen nur Funktionen aus hier auf
- `src/templates.rs` (Template-Structs) und `templates/` (askama-HTML)
- `src/error.rs`: ein `AppError`-Typ, der `IntoResponse` implementiert

## Regeln
- Kein `unwrap()` und kein `expect()` außerhalb von Tests und Startup-Konfiguration
- Preise als `i64` in Cent, nie `f64`
- sqlx 0.9 mit Compile-Zeit-Prüfung (`query!`/`query_as!`), Migrationen unter `migrations/`. Nach jeder SQL-Änderung `cargo sqlx prepare -- --all-targets` und `.sqlx/` einchecken. Dynamisches SQL nur mit `sqlx::AssertSqlSafe` und Begründung im Kommentar
- `rand` 0.10 (`rand::fill`, `rand::rng()`), argon2 0.6 (`password_hash::phc`)
- Passwörter mit argon2, nie im Klartext loggen
- Session-Cookie: HttpOnly, SameSite=Lax, Secure in Produktion
- Formulare mit CSRF-Schutz bei allen POST-Routen
- Auth-Middleware für alles außer `/login`
- Logging mit `tracing`, keine `println!`

## Fehler
- Handler geben `Result<T, AppError>` zurück
- Datenbankfehler werden geloggt, dem Nutzer wird eine kurze deutsche Meldung gezeigt
