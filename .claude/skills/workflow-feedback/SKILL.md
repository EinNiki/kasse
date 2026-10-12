---
name: workflow-feedback
description: Use at the start of any multi-step task in this project, and at each milestone. Defines how to plan, test, and pause for user feedback.
---

# Arbeitsweise

## Ablauf
1. Plan zeigen: Projektstruktur, Routen, Datenmodell. Auf Freigabe warten.
2. Pro Meilenstein umsetzen (z. B. Login, Lager, Buchen, Statistik).
3. Nach jedem Meilenstein:
   - Tests laufen lassen (`cargo test`)
   - App per `docker compose up -d` starten und die Route im Browser-Ablauf prüfen
   - Kurz zusammenfassen: was geändert, was getestet, was offen ist
   - Stoppen und auf Feedback warten
4. Erst nach Freigabe mit dem nächsten Meilenstein weitermachen.

## Regeln
- Keine Annahmen über Steuern, Bonpflicht oder Rechte treffen. Nachfragen.
- Sudo nur für Installationen nutzen, die wirklich nötig sind (z. B. Docker). Das Passwort nie in Dateien, Code oder Git schreiben.
- Keine Secrets committen.
- Fehler melden statt still zu umgehen.
