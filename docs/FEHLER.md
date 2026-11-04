# Fehlerliste

Funde aus den Prüfrunden und aus dem Gebrauch. Neueste Prüfung oben.

Einstufung: **A** stürzt ab / Daten in Gefahr · **B** falsch oder unbedienbar ·
**C** langsam oder umständlich · **D** Schönheitsfehler

## Aus dem Gebrauch

| Nr | Stufe | Fund | Lösung | Stand |
|---|---|---|---|---|
| F11 | B | Wolke, Vieleck und Streckenzug ließen sich kaum zeichnen: Die Form endete nur mit einem Doppelklick innerhalb von 0,3 s (Windows erlaubt üblicherweise 0,5 s), und ein Klick, bei dem die Maus ein paar Pixel verrutschte, setzte keinen Punkt. Gemeldet von Mr. M | Ein Klick auf den letzten Punkt beendet die Form, egal wie schnell; bei geschlossenen Formen auch ein Klick auf den ersten Punkt. Verrutschte Klicks (Ziehen) setzen ebenfalls Punkte. Ein Maustest prüft langsamen Doppelklick, verrutschte Klicks und Schließen am ersten Punkt (alter Stand: alle 8 Fälle scheitern, neuer: alle gelingen) | behoben 2026-11-04 |
| F12 | B | Dialog-Vorschauen blieben beim ersten Bild stehen (Hintergrundfarbe, Wasserzeichen, Kopf-/Fußzeilen, Paginierung, Drucken, Zuschneiden, Seitenlayout …): Jede Änderung baut ein neues Probedokument, der Bildspeicher erkannte es aber nur an der Versionsnummer, und die war immer gleich. Dadurch wirkte z. B. eine gewählte Hintergrundfarbe „nicht eingefügt“. Gemeldet von Mr. M | Jedes Dokument hat eine eigene Kennung; die Vorschau wird neu gezeichnet, sobald sich das Probedokument ändert. Maustest: Farbe, Wasserzeichen-Text und Kopfzeilen-Text erscheinen in der Vorschau | behoben 2026-11-04 |
| F13 | C | Kopf- und Fußzeilen waren in der Vorschau nur etwa 2 Pixel hoch; Vorschauen ließen sich nicht vergrößern. Gemeldet von Mr. M | Alle Vorschauen zoomen mit dem Mausrad (an der Mausposition) und mit „−“, „+“ und „Ganze Seite“ in der Ecke; Ziehen verschiebt (in Vorschauen mit Griffen mit der rechten Maustaste), Doppelklick stellt die Startansicht her. Kopf-/Fußzeilen und Paginierung zeigen oben und unten der Seite in voller Breite nebeneinander | behoben 2026-11-04 |
| F14 | B | „Per E-Mail senden“ öffnete unter Windows den Webbrowser statt des E-Mail-Programms, und die Datei musste man selbst anhängen. Gemeldet von Mr. M | Windows: neue Nachricht im Standard-E-Mail-Programm über Simple MAPI, das Dokument (bei ungespeicherten Änderungen eine Kopie des aktuellen Stands) ist angehängt; gesendet wird nichts ohne den Benutzer. Ohne MAPI-fähiges Programm (z. B. „neues Outlook“) öffnet Windows die mailto-Adresse mit dem dafür eingetragenen Programm und der Explorer zeigt die Datei. Gilt auch für „Alle Dokumente per E-Mail“ und „Formulardaten per E-Mail“ | behoben 2026-11-04, auf Windows noch von Mr. M zu prüfen |
| F9 | B | Formen und Textfelder ließen sich nur verschieben, nicht in der Größe ändern; bei Linien, Pfeilen, Polygonen und Messungen ließen sich die Punkte nicht einzeln verschieben. Beim Maus-Rundgang gefunden | Eine ausgewählte Anmerkung zeigt Griffe: acht Quadrate am Rahmen (Rechteck, Oval, Wolke, Textfeld, Schreibmaschine, Beschreibung, Stempel, Stift) bzw. Kreise an jedem Punkt (Linie, Pfeil, Polylinie, Polygon, Messungen). Ziehen an einem Griff ändert Größe bzw. Punkt; Messwerte werden neu berechnet. Rückgängig geht in einem Schritt | behoben 2026-11-04 |
| F10 | D | „Suchen und schwärzen“, „Lesezeichen umbenennen/hinzufügen“ und „Text zu Lesezeichen hinzufügen“: OK mit leerem Feld tat stillschweigend nichts | OK ist grau, solange nichts eingegeben ist (auch bei „Verweis erstellen“ und „Herunterladen“ mit ungültiger Adresse) | behoben 2026-11-04 |
| F8 | B | Drucken mit „Keine Größenanpassung“: Die Seite wurde an die Ecke des **bedruckbaren Bereichs** gelegt statt an die Papierecke, also um den Druckerrand nach rechts und unten verschoben; rechts und unten fehlte etwas, selbst bei A4 auf A4. Bei Letter auf A4 (6 mm breiter) fehlte deutlich mehr. Die Vorschau zeigte das nicht erkennbar (die abgeschnittene Kante sah aus wie ein Seitenrand). Gemeldet von Mr. M | Seiten sitzen an der Papierecke (bzw. mittig); nur der unbedruckbare Druckerrand fällt weg. Die Vorschau zeichnet den bedruckbaren Bereich gestrichelt ein. Ragt eine Seite über das Papier hinaus, warnt der Dialog („Seite 1 ist größer als das Papier … es fehlen bis zu 6 mm in der Breite“) und bietet „Auf Druckbereich reduzieren“ an | behoben 2026-11-04; von Mr. M mit echtem Windows-Drucker bestätigt |

## Maus-Prüfung vom 2026-11-04

Jeder Knopf aller zwölf Menüreiter wurde mit der Maus angeklickt, jeder
Eintrag jedes Aufklappmenüs ebenso, jedes Mal in einem frischen Fenster
(`tests/mouse.rs`, 428 Klickfolgen). Dialoge wurden mit OK bestätigt,
Werkzeuge auf der Seite benutzt. Für jede Anmerkungsart zusätzlich:
anlegen, anklicken, verschieben, Größe/Punkt ändern, Entf, Strg+Z; für
Textkorrektur und „Inhalt hervorheben“ Text markieren bzw. anklicken.

Ergebnis: keine Abstürze, keine Fehlermeldungen; Funde F9 und F10. Alle
Klicks „ohne sichtbare Wirkung“ ließen sich erklären (Zustand schon
eingestellt, Dokument ohne Felder/Verweise, Dateiauswahl des Systems).
Nicht geklickt: Drucken, E-Mail, Weblinks, Dateizuordnung, Vorlesen,
Umbenennen/Verschieben/Papierkorb, Beenden.

## Prüfung vom 2026-11-04

### Was geprüft wurde

| Runde | Inhalt | Ergebnis |
|---|---|---|
| 1 | Alle 260 Befehle der Befehlssuche, jeder in einem frischen Fenster mit Beispieldokument (`tests/tour.rs`), zweimal: Fenster 1280×820 und 1024×680 | 0 Abstürze, 0 Fehlermeldungen, nichts langsamer als 2 s; 86 Dialoge fotografiert und vermessen |
| 2 | Sechs echte PDFs (1,5 bis 274 MB, 9 bis 1476 Seiten): drehen, Anmerkung, löschen, einfügen, umkehren, herausziehen, alles rückgängig, speichern; nach jedem Schritt neu geöffnet und verglichen (`tests/real_files.rs`) | 5 von 6 Dateien fehlerfrei; bei 274 MB siehe F4 |
| 3 | 300 beschädigte Mini-PDFs und 200 beschädigte Varianten einer echten Datei: öffnen, zeichnen, Text, bearbeiten, speichern (`tests/robustness.rs`) | 0 Abstürze, nichts hängt; beschädigte Dateien werden geöffnet oder sauber abgelehnt |

Nicht automatisch geprüft, weil sie außerhalb von MrPDF wirken: E-Mail
senden, Datei umbenennen/verschieben/löschen, Weblinks (Handbuch, Support,
Startseite, Update), Dateizuordnung in Windows, Vorlesen. Ebenso wenig das
echte Drucken und das Arbeiten mit der Maus in jedem Werkzeug.

### Behoben

| Nr | Stufe | Fund | Lösung |
|---|---|---|---|
| F1 | B | „Verweise aus Lesezeichen erstellen“ war viel breiter als das Fenster; Titelleiste und Schließen-Knopf lagen außerhalb, der Dialog war kaum bedienbar | Die zwei unteren Bereiche stehen jetzt in zwei gleich breiten Spalten, die Einstellungen untereinander |
| F2 | B | Hohe Dialoge („Paginierung hinzufügen“, „Kopf- und Fußzeilen“) waren höher als das Fenster; Titel oben abgeschnitten, OK unten am Rand oder unerreichbar | Kein Dialog wird mehr höher als das Fenster, sonst erscheint ein sichtbarer Rollbalken; die Vorschau dieser Dialoge passt sich der Fensterhöhe an |
| F3 | D | Die Hilfe zur Kommandozeile kannte `--new-window` und `--opengl` nicht | Ergänzt |

### Nachträglich behoben (2026-11-04)

| Nr | Stufe | Fund | Lösung |
|---|---|---|---|
| F4 | C | Bei sehr großen Dateien blieb nur **ein** Rückgängig-Schritt: Jeder Schritt speichert die ganze Datei, der Verlauf durfte 512 MB belegen, ältere Schritte wurden ohne Hinweis verworfen | Ältere Schritte werden in temporäre Dateien ausgelagert statt verworfen (höchstens 4 GB, wie bisher höchstens 50 Schritte). Die Dateien verschwinden mit dem Schritt bzw. beim Schließen; Reste nach einem Absturz räumt der nächste Start auf. Mit 274 MB: sechs Änderungen und „alles rückgängig“ ergeben wieder genau den Anfangszustand |
| F5 | C | Änderungen der Seitenstruktur dauerten bei 274 MB / 1476 Seiten 1,5 bis 2 s, weil danach jede Seite geladen wurde, nur um ihre Größe zu lesen | Rahmen und Drehung werden direkt aus der Datei gelesen und mit PDFiums schneller Größenabfrage abgeglichen; nur abweichende Seiten werden geladen. Drehen 0,40 s, Seite löschen 0,25 s; bei 50 MB 20 bis 80 ms |
| F6 | C | Öffnen der 274-MB-Datei dauerte 1,6 s | Gleiche Lösung: 0,18 s |
| F7 | B | Bei Seiten, deren Seitenrahmen nur vom übergeordneten Seitenknoten geerbt wird, nahm MrPDF einen Rahmen am Nullpunkt an; Klickpositionen konnten dort leicht versetzt sein. Beim Bau von F5 gefunden | Der geerbte Rahmen wird jetzt gelesen; ein Test vergleicht gedrehte, zugeschnittene und geerbte Seiten mit PDFium |

### Offen

Keine offenen Punkte aus dieser Prüfung.
## Werkzeuge der Prüfung

* `cargo test --release --test tour -- --ignored --nocapture` – Befehls-Rundgang,
  Bericht und Dialogfotos in `target/tour/`; `MRPDF_TOUR_SIZE=1024x680` prüft
  einen kleinen Bildschirm. Auf Linux ohne Desktop vorher
  `DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus` setzen, sonst wartet
  der Datei-Dialog des Systems auf eine Eingabe.
* `bash scripts/mouse.sh [Reiter,Reiter]` – Maus-Rundgang im Hintergrund
  (Log `/tmp/mouse.log`, Bericht `target/mouse/report.md` und
  `handling.md`); einzeln: `cargo test --release --test mouse <Name> --
  --ignored --nocapture` mit `mouse_tour`, `annotation_handling`,
  `text_tools`, `polygon_gestures`, `dialog_previews`.
* `MRPDF_TESTFILES=<Ordner> cargo test --release --test real_files -- --ignored --nocapture`
  – Messungen und Rundläufe mit echten Dateien.
* `MRPDF_TESTFILES=<Ordner> cargo test --release --test robustness -- --include-ignored`
  – beschädigte Varianten einer echten Datei.
