---
name: frontend-kasse
description: Use when creating or changing HTML templates, CSS, or any UI of the cash register app.
---

# Frontend-Regeln Kasse

## Grundsatz
Design-System "Botanical / Organic Serif": ruhig, warm, handwerklich, erdige Farben, weiche Formen.
Auf der Kasse zählen trotzdem Tempo und Lesbarkeit: Dekoration darf nie die Bedienung (Tippen, Lesen aus Abstand) behindern.

## Design-Tokens (alle als CSS-Variablen in `:root`, einzige Quelle)
- background `#EBE7DC`, foreground `#2D3A31` (Waldgrün, statt Schwarz), card `#FFFFFF`, card-soft `#E4DFD2`
- primary (Salbei) `#8C9A84` für Flächen, Rahmen, Icons. Für **Text** auf hellem Grund `#54634C` (Kontrast AA)
- secondary (Ton) `#D3C4B4`, border `#D6D0C0`
- interactive (Terrakotta) `#C27B66` für Hover und einzelne Akzente
- Schatten weich, grün getönt (`rgba(45,58,49,…)`), als Variablen sm/md/xl
- Radien: Karten 24 px, Buttons und Inputs Pille (`999px`), Bögen mit `border-radius: 200px 200px … …`

## Typografie
- Überschriften: `"Playfair Display"` 600/700, einzelne Schlüsselwörter *kursiv* (Klasse `.em`, Farbe Terrakotta)
- Fließtext/UI: `"Source Sans 3"` 400/500/600
- Schriften nur lokal aus `static/fonts/` per `@font-face` mit `font-display: swap`. **Keine Google Fonts, keine CDN.**
- Zahlen immer mit `font-variant-numeric: tabular-nums`
- Buttons: Großbuchstaben, `letter-spacing: .1em`, klein (ca. 14 px)

## Erlaubt und erwünscht
- Hintergrundzeichnung `static/bg-shapes.svg` (ruhige Kreis- und Bogenformen in den Ecken, fixiert, nicht klickbar, hinter allem), darüber Papierkorn
- Papierkorn: fester SVG-Rauschen-Overlay (`body::after`, `opacity: .015`, `pointer-events: none`) als Data-URI
- Weiche Schatten, Hover-Lift (`translateY(-4px)`) mit Schattenwachstum
- Invertierte Bereiche (Waldgrün mit hellem Text), Bögen, keine versetzten Karten
- `backdrop-filter: blur(4px)` nur auf rein dekorativen Overlays, nie auf Bedienelementen
- Icons: handgezeichnetes Inline-SVG, Strichstärke 1,5, in blassen Kreisen
- Seitenwechsel: ruhig wie das Öffnen einer App auf iOS. Die alte Seite blendet in 180 ms aus, die neue setzt sich in 460 ms (`--ease-apple`) von etwas kleiner und tiefer ein. Nur bei echten Seitenwechseln (View-Transition-Typ `page`), die Kopfzeile bleibt stehen. Keine Verwandlung von Kacheln in Seiten. Auch beim Öffnen der Buchungsseite
- Live-Updates (htmx + Idiomorph, `hx-swap="morph:…"`) ändern Elemente an Ort und Stelle: unveränderte Elemente werden nie angefasst, nur geänderte Texte und Klassen aktualisieren sich (CSS-Übergänge animieren den Unterschied). Kein Neuladen, kein Ausblenden, kein Springen. Klicks beim Buchen laufen per `hx-boost` ohne Seitenwechsel und werden mit `hx-sync="queue all"` der Reihe nach abgearbeitet, damit kein Tippen verloren geht
- Reiter (Lager) tauschen nur `#lager-body` aus. Der neue Inhalt gleitet 22 px seitlich ein und blendet dabei ein (380 ms): vorwärts (Produkte → Bewegungen → Kategorien) von rechts, rückwärts von links. Die Richtung setzt `static/app.js` als Klasse `slide-fwd`/`slide-back` nach dem Austausch. Die Linie unter dem aktiven Reiter ist fest, keine View-Transitions für Teil-Updates. `html { scrollbar-gutter: stable }` verhindert, dass die Seite beim Wechsel seitlich springt
- Suche ist sofort: kein Debounce, `hx-sync="this:replace"` (der neueste Tastendruck gewinnt), Antwort in ca. 10 ms. Kein Redis oder anderer Cache nötig
- Nach jedem Live-Update wird die Scroll-Position wiederhergestellt (`static/app.js`), nichts springt
- Die Kopfzeile ist deckend. Die Leiste mit Suche und Kategorien beim Buchen ist **kein sichtbarer Kasten**: sie malt dasselbe wie die Seite dahinter (`bg-shapes.svg` mit `background-attachment: fixed` auf der Hintergrundfarbe), kein Rand, kein Schatten; auf dem Handy, wo die Formen fehlen, ist sie einfarbig. Die Hintergrundformen bleiben auch auf der Buchungsseite sichtbar
- Abbuchen: Schnellbeträge und Eingabe aktualisieren nur `#pay` an Ort und Stelle (Morph), die Seite lädt nicht neu
- Keine Dauer-Animationen (nichts bewegt sich von allein)
- Bei `prefers-reduced-motion: reduce` entfallen alle Animationen, Übergänge und Hover-Bewegungen

## Verboten
- Emojis als Icons, Icon-Bibliotheken (auch Lucide), Icon-Fonts
- CSS-Frameworks (Tailwind, Bootstrap), Animationsbibliotheken, CDN-Skripte, externe Fonts
- Stock-Hero-Bilder oder Dekorbilder
- Harte Schlagschatten, grelle Farben, Verläufe als Button-Füllung
- Hart codierte Werte, die als Variable existieren

- Die Admin-Kachel sieht nur, wer Admin ist (zusätzlich prüft jede Admin-Route die Rolle serverseitig)
- Startseite: Überschrift ohne Label, zufälliger launiger Spruch mit einem kursiven Wort (`HEADLINES` in `src/routes/home.rs`)

## Layout Tablet quer
- Warenkorb fest rechts, Produktkacheln links im Grid
- Hauptaktionen immer sichtbar, keine versteckten Menüs
- Touch-Ziele mindestens 48 px hoch, 8-px-Raster
- Responsiv: Dekor auf kleinen Bildschirmen ausblenden (unter 900 px)

## Performance
- Eine CSS-Datei, kein JavaScript außer nötigem htmx (lokal ausgeliefert)
- Bilder nur, wenn nötig, mit `width` und `height`
- Keine externen Requests beim Laden

## Responsive (Pflicht für jede Seite)
- Breakpoints: 1100 px (Tablet quer), 900 px (eine Spalte), 640 px (Handy). Nirgends horizontales Scrollen.
- Tabellen bekommen `class="responsive"` und an jedem `<td>` ein `data-label`; auf dem Handy werden sie zu gestapelten Karten.
- Buchen auf dem Handy: Warenkorb unter den Kacheln, feste Leiste „Abbuchen · Summe“ unten.
- Suchfelder sind live (htmx, `input changed`, ohne Verzögerung), funktionieren ohne JavaScript weiter als normales GET-Formular.
- Handy (bis 640 px): Basisschrift 16 px, kompakte Abstände, Kacheln 2 pro Reihe, Touch-Ziele mindestens 44 px
- Statistik: Von, Bis und Zeitraum-Buttons liegen im selben 3-Spalten-Raster wie die Kennzahl-Kästen darüber und darunter
- Alle Steuerelemente in einer Filterzeile haben dieselbe Höhe (56 px) und dieselbe Unterkante.
- Jeder Button hat sichtbaren Hover (Anheben + Tönung) und `:active`-Rückmeldung, auch innerhalb von Tabellenzeilen.

## htmx und Sicherheitsrichtlinie (CSP)
- Die CSP erlaubt nur eigene Dateien: kein Inline-JS, kein Inline-CSS (`style=`), kein `eval`. Konfiguration von htmx steht in `<meta name="htmx-config">` (`allowEval:false`).
- Deshalb in `hx-swap` nur Schlüsselwörter ohne Code verwenden, zum Beispiel `morph:outerHTML` oder `outerHTML transition:true`. **Nie** `morph:{...}`, `hx-on`, `hx-vals='js:...'` oder `hx-trigger` mit Filterausdrücken (sie werden ausgewertet und scheitern lautlos; ein Test prüft das).
- Einstellungen der Idiomorph-Bibliothek gehören in `static/app.js` (nur Konfiguration, keine eigene Logik).
- Nach jedem Umbau von Live-Updates prüfen, dass jede ID nur einmal im Dokument vorkommt (verschachtelte Kopien sind das Zeichen eines kaputten Swaps).

## Bedienung per Touch
- Kopfleiste ist fest (`position: sticky`), ebenso Suche und Kategorien beim Buchen. htmx scrollt nach einem Klick nie von selbst (`scrollIntoViewOnBoost:false`).
- Klickflächen von Kacheln, Chips und +/- reichen halb in die Lücke zum Nachbarn (`::after` mit negativem `inset`, 2 px Überlappung), damit ein knapp danebenliegender Tipp trotzdem trifft.
- Hover-Effekte, die hängen bleiben könnten, stehen in `@media (hover: none)` zurück.
- Das Layout darf sich beim Ändern des Warenkorbs nie verschieben (gleiche Spaltenbreiten vorher und nachher).
