# Fehlerliste

Funde aus den Prüfrunden und aus dem Gebrauch. Neueste Prüfung oben.

Einstufung: **A** stürzt ab / Daten in Gefahr · **B** falsch oder unbedienbar ·
**C** langsam oder umständlich · **D** Schönheitsfehler

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

### Offen

| Nr | Stufe | Fund | Vorschlag |
|---|---|---|---|
| F4 | C | Bei sehr großen Dateien bleibt nur **ein** Rückgängig-Schritt: Jeder Schritt speichert die ganze Datei, der Verlauf darf 512 MB belegen. Bei 274 MB werden ältere Schritte ohne Hinweis verworfen | Ältere Schritte auf die Festplatte auslagern (Temp-Ordner), statt sie zu verwerfen; mindestens einen Hinweis zeigen |
| F5 | C | Änderungen an der Seitenstruktur (drehen, löschen, einfügen, umkehren) dauern bei 274 MB / 1476 Seiten etwa 1,5 bis 2 s, weil danach alle Seitengrößen neu gelesen werden | Seitengrößen nur für betroffene Seiten neu lesen |
| F6 | C | Öffnen der 274-MB-Datei dauert etwa 1,6 s aus demselben Grund | Seitengrößen beim Öffnen schneller ermitteln |

## Werkzeuge der Prüfung

* `cargo test --release --test tour -- --ignored --nocapture` – Befehls-Rundgang,
  Bericht und Dialogfotos in `target/tour/`; `MRPDF_TOUR_SIZE=1024x680` prüft
  einen kleinen Bildschirm. Auf Linux ohne Desktop vorher
  `DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent/bus` setzen, sonst wartet
  der Datei-Dialog des Systems auf eine Eingabe.
* `MRPDF_TESTFILES=<Ordner> cargo test --release --test real_files -- --ignored --nocapture`
  – Messungen und Rundläufe mit echten Dateien.
* `MRPDF_TESTFILES=<Ordner> cargo test --release --test robustness -- --include-ignored`
  – beschädigte Varianten einer echten Datei.
