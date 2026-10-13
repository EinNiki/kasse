---
name: stock-rules
description: Use when implementing sales, stock movements, prices, or statistics. Applies to any code that changes product stock or money.
---

# Bestands- und Verkaufsregeln

## Abbuchen
- Immer in einer Datenbanktransaktion.
- Betroffene Produkte mit `SELECT ... FOR UPDATE` sperren.
- Prüfen: Bestand reicht für alle Positionen. Falls nicht, Fehler zurückgeben und nichts buchen.
- Bestand reduzieren, `sales` und `sale_items` schreiben, `stock_movements` mit negativem Delta eintragen. Alles oder nichts.

## Unbegrenzte Produkte
- `products.unlimited`: kein Bestandscheck, der Bestand ändert sich beim Verkauf nicht. Der Verkauf steht trotzdem in `sale_items` und als Bewegung (Delta negativ, Grund „Verkauf“).
- Wareneingang und Korrektur sind dafür gesperrt (`StockError::Unlimited`). Im Warenkorb gilt eine Obergrenze von 999.
- Unbegrenzte Produkte zählen nicht als „ausverkauft“.

## Wareneingang und Korrektur
- Jede Änderung erzeugt einen Eintrag in `stock_movements` mit `delta`, `reason` und optionalem `comment`.
- Bestand darf nie negativ werden, auch nicht durch Korrektur.

## Preise
- Immer `i64` in Cent.
- `sale_items` speichert `name_snapshot` und `unit_price_cents` zum Verkaufszeitpunkt.
- Preisänderungen wirken nur auf neue Verkäufe.

## Rückgeld
- `change_cents = paid_cents - total_cents`.
- Bei `paid_cents < total_cents` nicht abbuchen, Hinweis anzeigen.

## Tests (Pflicht)
- Abbuchen mit ausreichendem Bestand: Bestand sinkt korrekt.
- Abbuchen mit zu wenig Bestand: Fehler, Bestand unverändert.
- Zwei parallele Abbuchungen auf den letzten Artikel: nur eine gelingt.
- Rückgeld-Berechnung inklusive Grenzfall exakt gleicher Betrag.
