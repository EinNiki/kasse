# Kasse

Kassen- und Lagerverwaltung für Theke/Kiosk. Rust (Axum), PostgreSQL (sqlx, Migrationen), serverseitige
askama-Templates, eine handgeschriebene CSS-Datei, htmx (mit Idiomorph) für Live-Suche, Warenkorb-Klicks und Umschalten ohne Neuladen. Oberfläche auf Deutsch,
ausgelegt für Tablets im Querformat. Als Datenbank wird ausschließlich PostgreSQL verwendet.

## Fertige Installation ohne Quellcode

Für den Einsatz reicht **eine Datei**: `deploy/docker-compose.yml` (oder die aus dem jeweiligen GitHub-Release). Das Image ist nur etwa 11 MB groß (statisches
Binary ohne Betriebssystem), wird bei jedem Push automatisch gebaut (GitHub Actions) und liegt unter `ghcr.io/einniki/kasse`. Es gibt keinen
Setup-Befehl und keinen eingebauten Proxy: HTTPS übernimmt dein Reverse Proxy (zum Beispiel Nginx Proxy Manager).

1. In der Datei die drei Passwörter ändern (`change-me-db` kommt zweimal vor, `change-me-app`, `change-me-admin`). Solange noch ein `change-me…` drinsteht, startet die App nicht.
2. `docker compose up -d`
3. Im Nginx Proxy Manager einen Proxy-Host anlegen: Weiterleitung an `http://<Rechner>:8080`, SSL-Zertifikat und „Force SSL“ aktivieren.
4. Adresse öffnen, Anmeldung mit `admin` und dem gesetzten Admin-Passwort.

Aktualisieren: `docker compose pull && docker compose up -d`. Das Image gibt es als `:latest` (Hauptzweig), `:dev` (Entwicklung) und als Versionen (`:1.0.0`).

## Automatischer Bau (GitHub Actions)

`.github/workflows/ci.yml` läuft bei jedem Push auf `dev` und `main`, bei Pull Requests und bei Versions-Tags (`v1.2.3`):

1. **Test:** `cargo fmt --check`, `cargo clippy -D warnings`, alle Tests gegen eine echte PostgreSQL-Datenbank. Die SQL-Abfragen werden gegen den
   eingecheckten Cache `.sqlx/` geprüft; vergessene `cargo sqlx prepare` lässt den Bau scheitern.
2. **Image:** Docker-Image bauen und mit der Fertig-Compose-Datei starten (Rauchtest: Login-Seite, Sicherheits-Header, eingeschränkte DB-Rolle).
3. **Veröffentlichen** (nicht bei Pull Requests): Image für x86_64 (amd64) nach `ghcr.io`.
4. **Release** (bei Tags `v*`): GitHub-Release mit `docker-compose.yml` (Image-Version fest eingetragen).

Dazu prüft ein Audit-Job die Abhängigkeiten (blockiert nicht), und Dependabot schlägt wöchentlich Updates vor.
Einmalig nötig: Unter *GitHub → Packages → kasse → Package settings* die Sichtbarkeit auf **Public** stellen, sonst braucht `docker compose pull` eine Anmeldung.

## Start (aus dem Quellcode)

```sh
cp .env.example .env      # ALLE Passwörter ändern
chmod 600 .env
docker compose up -d        # zieht das fertige Image von ghcr.io/einniki/kasse:dev
# eigenen Quellcode bauen statt das Image zu ziehen:
# docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

Danach ist die App unter **<http://localhost:8080>** erreichbar (Port über `APP_PORT` in `.env`).
Von anderen Geräten im Netz: `http://<IP-des-Rechners>:8080`.

Beim ersten Start passiert automatisch:
1. Migrationen laufen (`migrations/`).
2. Die Datenbankrolle der App (`APP_DB_USER`) wird mit eingeschränkten Rechten angelegt, siehe Abschnitt „Datenbank-Sicherheit“.
2. Der Admin wird aus `ADMIN_USER` / `ADMIN_PASSWORD` angelegt, aber nur, wenn noch kein Benutzer existiert. Platzhalter-Passwörter aus `.env.example` werden abgelehnt.
3. Die Getränkeliste (28 Produkte in den Kategorien „Alkohol“ und „Nicht-Alkohol“, Bestand 0) wird einmalig angelegt.

Stoppen: `docker compose down` (Daten bleiben im Volume `pgdata`). `docker compose down -v` löscht auch alle Daten.

## Verschlüsselung (HTTPS)

Die Entwicklungs-Compose-Datei im Hauptordner enthält einen Caddy-Proxy mit eigener Zertifizierungsstelle; die Fertig-Installation (oben) nutzt stattdessen deinen eigenen Proxy. Die App spricht nur über einen vorgeschalteten HTTPS-Proxy (Caddy, TLS 1.2/1.3). Die App selbst ist von außen nicht erreichbar, und die
Sitzungs-Cookies sind `Secure`. Passwörter und Buchungen laufen also nie unverschlüsselt durchs WLAN.

Weil im lokalen Netz keine öffentliche Zertifizierungsstelle ein Zertifikat ausstellt, erzeugt der Proxy eine eigene. Damit die Browser
keine Warnung mehr zeigen, muss jedes Gerät dieser Zertifizierungsstelle **einmalig** vertrauen:

```sh
./scripts/export-ca.sh        # schreibt kasse-root-ca.crt (nur das öffentliche Zertifikat)
```

- **iPhone/iPad:** Datei übertragen (AirDrop/Mail), öffnen, „Profil laden“, dann *Einstellungen → Allgemein → Info → Zertifikatsvertrauenseinstellungen* aktivieren.
- **Android:** *Einstellungen → Sicherheit → Weitere Einstellungen → Verschlüsselung & Anmeldedaten → Zertifikat installieren → CA-Zertifikat*.
- **Windows:** Doppelklick, *Zertifikat installieren → Lokaler Computer → Vertrauenswürdige Stammzertifizierungsstellen*.
- **macOS:** Doppelklick, im Schlüsselbund auf „Immer vertrauen“ stellen.
- **Firefox** hat einen eigenen Speicher: *Einstellungen → Datenschutz & Sicherheit → Zertifikate → Zertifikate anzeigen → Importieren*.

Ändert sich die Adresse (neue IP), `SITE_ADDRESS` in `.env` anpassen und `docker compose up -d` ausführen. Die Zertifizierungsstelle bleibt im Volume
`caddy_data` erhalten, die Geräte müssen nichts neu installieren. Mit `docker compose down -v` geht sie verloren.

## Bedienung

| Bereich | Was geht |
|---|---|
| **Buchen / Bestellen** | Kategorie wählen (oder „Alle“), Suche und Kategorie-Filter, Die Suche ist live: schon beim Tippen erscheinen die Treffer, der beste (Name beginnt mit dem Suchbegriff) zuerst. Produktkachel antippen legt sie in den Warenkorb (rechts). Warenkorb mit +/−, Entfernen und Summe. Ausverkaufte Produkte sind sichtbar, aber gesperrt. „Abbuchen“: Betrag unter „Gegeben“ eintippen (oder Schnellbetrag wählen), das Rückgeld erscheint sofort. Danach Bestätigung mit Summe und Rückgeld. |
| **Lager** | Produkte mit Live-Suche und Kategorie-Filter. Anlegen, Bearbeiten, Löschen (Soft-Delete, Verkäufe bleiben erhalten). Bestand „Hinzufügen“ (Wareneingang) und „Entfernen“ (Korrektur) mit Menge und Kommentar. Reiter „Bewegungen“ zeigt das Protokoll, Reiter „Kategorien“ legt Kategorien an, benennt um und löscht (nur leere). |
| **Admin** (nur für Admins sichtbar) | Benutzer anlegen (optional als Admin), Passwort setzen, Admin-Rechte vergeben oder entziehen, Benutzer deaktivieren oder wieder aktivieren (Benutzer mit Verkäufen werden nie gelöscht, damit die Historie erhalten bleibt). Man kann sich weder selbst deaktivieren noch die eigenen Admin-Rechte entziehen, damit immer ein Admin übrig bleibt. |
| **Statistik** | Umsatz heute, Woche, Monat. Für einen frei wählbaren Zeitraum: Umsatz, Anzahl Verkäufe, Ø je Verkauf, Top-Produkte nach Menge und Umsatz, Umsatz pro Kategorie, letzte Verkäufe mit Uhrzeit. Tagesgrenzen gelten in deutscher Zeit (Europe/Berlin). |

Regeln: Der Bestand wird nie negativ (weder beim Verkauf noch bei Korrektur, zusätzlich per CHECK-Constraint in der DB).
Abbuchen läuft in einer Transaktion mit Zeilensperre; reicht der Bestand nicht oder das Geld nicht, wird nichts gebucht.
Preise werden als Cent (`i64`) gespeichert, jede Verkaufsposition merkt sich Name und Preis zum Verkaufszeitpunkt.

## Benutzer

Weitere Benutzer legst du im Bereich **Admin** an. Alternativ per Kommandozeile (Passwort über stdin, mindestens 8 Zeichen):

```sh
echo 'EinSicheresPasswort' | docker compose run --rm -T app /app/kasse create-user anna          # normaler Benutzer
echo 'EinSicheresPasswort' | docker compose run --rm -T app /app/kasse create-user chef --admin  # Admin
```

Rollen: **Admin** (alles, inklusive Benutzerverwaltung) und **Benutzer** (Buchen, Lager, Statistik). Der erste Benutzer ist Admin.

## Migrationen

Migrationen liegen in `migrations/` und laufen beim App-Start automatisch (`sqlx::migrate!`).
Neue Migration anlegen: `sqlx migrate add <name>` (benötigt `sqlx-cli`).

## Entwicklung und Tests

Die SQL-Abfragen werden zur Compile-Zeit gegen eine Datenbank geprüft. Der Docker-Build nutzt den
eingecheckten Cache in `.sqlx/` (`SQLX_OFFLINE=true`).

```sh
# Wegwerf-Datenbank für Entwicklung und Tests (nur auf localhost)
docker run -d --rm --name kasse-devdb -e POSTGRES_PASSWORD=dev -e POSTGRES_DB=kasse \
  -p 127.0.0.1:5433:5432 postgres:16-alpine
export DATABASE_URL=postgres://postgres:dev@127.0.0.1:5433/kasse
sqlx migrate run
cargo test                # Tests legen pro Test eine eigene Datenbank an
cargo sqlx prepare -- --all-targets   # nach Änderungen an SQL-Abfragen, .sqlx/ mit einchecken
```

Die Tests decken unter anderem ab: Abbuchen mit und ohne ausreichenden Bestand, zwei parallele Abbuchungen auf den
letzten Artikel, Rückgeld inklusive exakt gleichem Betrag, Wareneingang und Korrektur mit Protokoll, Preis-Snapshot,
Soft-Delete, Kategorie-Regeln, Seed, Statistik sowie den kompletten Ablauf über HTTP inklusive CSRF-Schutz.

## Datenbank-Sicherheit

- **Eingeschränkte Rolle:** Migrationen laufen mit dem Datenbankbesitzer (`POSTGRES_USER`). Die laufende App verbindet sich als `APP_DB_USER`
  (kein Superuser, kann das Schema nicht ändern, kann `_sqlx_migrations` nicht lesen). Rechte pro Tabelle stehen in `src/db/roles.rs`;
  ein Test stellt sicher, dass jede neue Tabelle dort eingetragen wird.
- **Verlauf unveränderlich:** `sales`, `sale_items` und `stock_movements` sind append-only. Die App-Rolle darf dort nur lesen und einfügen,
  und zusätzlich verbieten Trigger UPDATE, DELETE und TRUNCATE selbst für den Besitzer. Korrekturen sind neue Zeilen.
- **Datenbank nicht erreichbar von außen:** Kein veröffentlichter Port, das Netz `backend` ist `internal` (kein Internetzugang, nur die App kommt heran).
  Anmeldung per `scram-sha-256`.
- **Integrität in der DB:** CHECK-Constraints (Bestand ≥ 0, Preise ≥ 0, Menge > 0, Gegeben ≥ Summe), Fremdschlüssel, Benutzer- und Kategorienamen
  eindeutig ohne Beachtung der Groß-/Kleinschreibung.
- **Container:** App läuft ohne Root, mit schreibgeschütztem Dateisystem, ohne Linux-Capabilities und `no-new-privileges`.
- **Verbindung:** Nur HTTPS, HSTS, Cookies `Secure`/`HttpOnly`/`SameSite=Lax`. Antworten tragen Content-Security-Policy (nur eigene Dateien, keine Inline-Skripte), `X-Frame-Options: DENY`, `nosniff`; dynamische Seiten sind `no-store`.
- **Zugangsdaten:** Nur in `.env` (`chmod 600`, nicht im Repo). Die Platzhalter aus `.env.example` werden von App und Compose abgelehnt bzw. verlangt.
- **Sitzungen** liegen nur als SHA-256-Hash in der DB, Passwörter als argon2. Nach 5 falschen Anmeldungen wird der Benutzername 5 Minuten gesperrt.
- **Backup:** `./scripts/backup.sh` schreibt einen komprimierten Dump nach `backups/` (nicht im Repo). Zurückspielen: siehe Kopf des Skripts.
  Ein Backup gehört auf ein anderes Medium als den Rechner der Kasse.
- **Nicht abgedeckt:** Verschlüsselung der Festplatte (die Datenbank liegt unverschlüsselt im Volume). Wer das braucht, verschlüsselt das Laufwerk des Kassenrechners.

## Sicherheit (App)

- Passwörter mit argon2, Session-Cookie (HttpOnly, SameSite=Lax), in der DB nur als Hash gespeichert.
- CSRF-Token bei allen POST-Formularen, alle Seiten außer `/login` verlangen eine Anmeldung.
- Schriften und htmx liegen lokal in `static/` (Lizenzen daneben), es gibt keine externen Requests.
