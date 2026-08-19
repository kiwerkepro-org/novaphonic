# NovaPhonic

Ein Tool von KI-WERKE. Schneidet Füllwörter und Stille automatisch aus Videos mit
einer sprechenden Person, entfernt Hintergrundgeräusche und gleicht Lautstärke sowie
Klangbalance an, ein selbstgebautes, lokales Gegenstück zu einfachen Auphonic
Funktionen.

## Status

Stufe 1 (diese App): dauerhaft kostenlos, komplett lokal, keine Internetverbindung
nötig, kein Server, für Aufnahmen mit einer Person. Näheres zur größeren
Produktvision (Stufe 2 mit Mehrsprechererkennung, Server, Abo Modell) liegt im
Projektgedächtnis, nicht in diesem Repository.

## Wie es technisch funktioniert

NovaPhonic ist eine schlanke Tauri Oberfläche (HTML/CSS/JS, siehe `app/`), die im
Hintergrund drei eigenständige Open Source Werkzeuge als sogenannte Sidecar Programme
aufruft, alle im Installer enthalten, nichts muss separat installiert werden:

- **auto-editor** (Public Domain, Unlicense): schneidet Füllwörter/Stille anhand von
  Lautstärke aus dem Video.
- **DeepFilterNet / deep-filter** (MIT/Apache 2.0): entfernt Hintergrundgeräusche aus
  der Tonspur.
- **FFmpeg**: gleicht Lautstärke und Klangbalance an (`loudnorm`, EBU R128) und
  übernimmt Audio Extraktion sowie das Wiederzusammenfügen von Bild- und Tonspur.

Alle drei laufen ohne Grafikkarte auf einer normalen CPU. Die Videodatei wird nie in
die Oberfläche selbst geladen, es werden nur Dateipfade zwischen Oberfläche und dem
Rust Kern (`tauri-app/src-tauri/`) ausgetauscht.

## Bauen

Siehe `tauri-app/BAUANLEITUNG.md` für die vollständige Anleitung, inklusive dem
Beschaffen der drei Sidecar Werkzeuge. Siehe `GITHUB_SETUP.md` für die einmalige
Einrichtung des GitHub Repositories und der automatischen Cloud Build Pipeline.

## Lizenz

GNU General Public License v3, siehe `LICENSE`.
