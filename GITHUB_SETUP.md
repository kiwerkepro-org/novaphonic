# NovaPhonic auf GitHub einrichten

Ich habe hier keinen GitHub Zugriff (keine Zugangsdaten, kein verbundenes GitHub Konto),
das Repository muss deshalb einmalig von dir selbst angelegt und gepusht werden, genau
wie bei NovaImage. Der GitHub Actions Workflow, der danach automatisch den Windows
Installer baut (inklusive Download der drei Sidecar Werkzeuge), liegt bereits fertig
unter `.github/workflows/build.yml`.

## Schritte

1. Auf GitHub ein neues, leeres Repository anlegen, zum Beispiel
   `https://github.com/kiwerkepro-org/novaphonic` (passend zum Namensmuster von
   NovaImage).

2. Eingabeaufforderung oder PowerShell öffnen und in den Ordner wechseln, in dem
   dieses NOVA-PHONIC Verzeichnis liegt, dann:

```
cd "C:\KIW-SCHMIEDE\NOVA-PHONIC" && git init && git add . && git commit -m "Initial commit: NovaPhonic Tauri App" && git branch -M main && git remote add origin https://github.com/kiwerkepro-org/novaphonic.git && git push -u origin main
```

   Falls das Repository bereits eine README oder Lizenz enthält und der Push deswegen
   abgelehnt wird, hilft `git push -u origin main --force` (nur beim allerersten Push
   unbedenklich, da das Repo sonst leer ist), oder vorher
   `git pull origin main --allow-unrelated-histories`.

3. Danach im Repository auf GitHub den Reiter **Actions** öffnen. Der Workflow
   "Windows Installer bauen" startet automatisch nach dem Push (weil Dateien unter
   `tauri-app/` enthalten sind) und kann zusätzlich jederzeit manuell über den Button
   **Run workflow** gestartet werden.

4. Nach erfolgreichem Lauf (dauert etwas länger als bei NovaImage, weil zusätzlich
   auto-editor, FFmpeg und DeepFilterNet heruntergeladen werden) findest du im
   jeweiligen Workflow Lauf unter **Artifacts** die Datei
   `novaphonic-windows-installer` zum Herunterladen, darin enthalten der fertige
   `.exe` (NSIS) Installer.

## Falls der erste Cloud Build fehlschlägt

Am wahrscheinlichsten ist ein Problem beim automatischen Herunterladen der
DeepFilterNet Windows Datei, weil sich deren genauer Dateiname von Version zu Version
ändern kann. Die Fehlermeldung im Actions Log zeigt das klar an, im Zweifel kurz die
Log-Ausgabe hierher kopieren, dann wird der entsprechende Schritt im Workflow
angepasst.

## Später: eigenes Signierschlüsselpaar für automatische Updates

Siehe dazu den Abschnitt "Wie Updates beim Nutzer ankommen" in
`tauri-app/BAUANLEITUNG.md`.
