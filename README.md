# NovaPhonic

Ein Tool von KI-WERKE. Schneidet Füllwörter und Stille automatisch aus Videos mit
einer sprechenden Person, entfernt Hintergrundgeräusche und gleicht Lautstärke sowie
Klangbalance an, ein selbstgebautes, lokales Gegenstück zu einfachen Auphonic
Funktionen.

> **Stufe 1, dauerhaft kostenlos.** Für Aufnahmen mit einer einzelnen Person, also
> einer Stimme im Video, läuft NovaPhonic komplett lokal auf deinem eigenen Rechner,
> keine Internetverbindung nötig, keine Anmeldung, keine Kosten.

> **Mehr ist geplant.** Eine Mehrsprechererkennung für Interviews kommt als
> kostenpflichtiges Zusatzmodul über einen gemieteten Server hinzu, ohne dauerhafte
> Speicherung der hochgeladenen Videos. Wer sehen will, was als Nächstes kommt, oder
> mitreden möchte: die
> [KIW Schmiede Community auf Skool](https://www.skool.com/kiw-schmiede-9100)

## Ordnerstruktur

```
app/               <- Oberfläche der Tauri App (HTML/CSS/JS)
tauri-app/         <- Desktop App Gerüst (Windows Installer)
  BAUANLEITUNG.md   <- für alle, die NovaPhonic selbst aus dem Quellcode bauen wollen
GITHUB_SETUP.md     <- für alle, die dieses Repository als Vorlage nutzen wollen
```

## Installation

1. Auf der [Releases Seite](https://github.com/kiwerkepro-org/novaphonic/releases)
   dieses Repositories die aktuellste Version öffnen.
2. Die Datei `NovaPhonic_<Version>_x64-setup.exe` herunterladen.
3. Installer starten, den Anweisungen folgen.
4. NovaPhonic prüft beim Start automatisch, ob eine neuere Version vorliegt, und
   bietet ein Update per Klick an.

Nur für Windows. auto-editor, DeepFilterNet und FFmpeg sind bereits im Installer
enthalten, es muss nichts zusätzlich installiert werden.

## Nutzung

1. Video per Drag and Drop in das Fenster ziehen, oder über die Dateiauswahl laden.
2. Bei Bedarf die Einstellungen anpassen: Mindestabstand um Sprechpausen (Margin),
   Entrauschen ein oder aus, Lautstärkeanpassung ein oder aus und Zielwert (LUFS).
3. Auf Start klicken. Die drei Schritte Schneiden, Entrauschen und Lautstärke
   anpassen laufen nacheinander durch, mit Fortschrittsanzeige und Protokoll.
4. Ergebnis über "Speichern unter" an den gewünschten Ort auf der Festplatte legen.

## Technische Details

NovaPhonic ist eine schlanke Tauri Oberfläche, die im Hintergrund drei eigenständige
Open Source Werkzeuge als sogenannte Sidecar Programme aufruft:

- **auto-editor** (Public Domain, Unlicense): schneidet Füllwörter und Stille anhand
  von Lautstärke aus dem Video.
- **DeepFilterNet / deep-filter** (MIT/Apache 2.0): entfernt Hintergrundgeräusche aus
  der Tonspur.
- **FFmpeg**: gleicht Lautstärke und Klangbalance an (`loudnorm`, EBU R128) und
  übernimmt Audio Extraktion sowie das Wiederzusammenfügen von Bild- und Tonspur.

Alle drei laufen ohne Grafikkarte auf einer normalen CPU. Die Videodatei verlässt bei
Stufe 1 nie den eigenen Rechner, es findet keine Verbindung zu einem Server statt.

## Grenzen

- Reine Lautstärke basierte Schnitterkennung, keine inhaltliche Prüfung. Schnitte
  können vereinzelt etwas abrupt wirken, weiche Übergänge sind nicht eingebaut.
- Entrauschen verbessert Hintergrundgeräusche, ersetzt aber keine professionelle
  Studioaufnahme oder ein bezahltes Tonstudio.
- Aktuell nur für Aufnahmen mit einer einzelnen sprechenden Person gedacht. Bei
  mehreren Personen im Video werden Stimmen nicht getrennt erkannt.

## Nächste Schritte (geplant)

- Mehrsprechererkennung für Interviews (Stufe 2), kostenpflichtiges Zusatzmodul über
  einen gemieteten Server, ohne dauerhafte Speicherung.
- Feineres Schneiden mit weichen Übergängen statt reiner Hartschnitte.

## Lizenz

GNU General Public License v3.0 (GPLv3), siehe [LICENSE](LICENSE). Wer den Code
verändert und weitergibt, muss den veränderten Quellcode ebenfalls unter der GPLv3
offenlegen, das gilt auch bei kommerzieller Nutzung.

## Markenrechte

Die Namen "NovaPhonic", "KI-WERKE" und "KIW Schmiede" sowie die zugehörigen Logos
sind eigenständig geschützte Kennzeichen von KI-WERKE und ausdrücklich nicht Teil der
GPLv3-Lizenz. Der Quellcode darf verändert und weitergegeben werden, die Namen und
Logos dürfen dabei aber nicht für eigene, insbesondere abgewandelte oder umbenannte
Versionen verwendet werden. Details siehe [LICENSE](LICENSE).
