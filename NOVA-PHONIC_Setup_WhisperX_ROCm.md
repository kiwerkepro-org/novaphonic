# Automatisierte Video Schnitt Pipeline mit WhisperX, Pyannote und ROCm

Vollständige Übersicht aller benötigten Werkzeuge sowie ein praxisnaher, schrittweiser Ablaufplan für einen Ubuntu Server mit AMD ROCm GPU und Docker.

## 1. Benötigte Werkzeuge und Komponenten

**WhisperX (KI Transkription und Wort Alignment)**
Nutzt intern CTranslate2 und faster whisper und erzeugt exakte Zeitstempel für jedes einzelne Wort.
GitHub: https://github.com/m-bain/whisperX

**pyannote.audio (Sprecher Diarisierung)**
Trennt automatisch beliebig viele Stimmen im Raum und weist jedes Wort dem jeweiligen Sprecher zu.
GitHub: https://github.com/pyannote/pyannote-audio
Hugging Face Modell: https://huggingface.co/pyannote/speaker-diarization-3.1

**FFmpeg (Video und Audio Engine)**
Extrahiert die Audiospur und schneidet das UHD Videomaterial verlustfrei ohne Neukodierung zusammen.
Webseite: https://ffmpeg.org

**ROCm PyTorch Docker Basis**
Stellt die GPU Treiberanbindung für AMD Grafikkarten im Container bereit.
Dokumentation: https://rocm.docs.amd.com
PyTorch: https://pytorch.org

**Docker Engine**
Webseite: https://docs.docker.com/engine/install/ubuntu/

## 2. Speicher und Verzeichnisstruktur auf dem Server

Passend zur Partitionierung werden feste Einhängepunkte (Mounts) eingerichtet:

```
/                       (500 GB: Ubuntu System und Root)
/srv/ai-tools           (100 GB: Docker Images, Skripte, Hugging Face / Whisper Modell-Caches)
/srv/backups            (300 GB: Datensicherungen und Skript Backups)
/data/media             (ca. 3.6 TB: Rohvideos, Arbeitsverzeichnis, finale Schnitte)
```

Benötigte Ordner im Terminal erstellen:

```bash
sudo mkdir -p /srv/ai-tools/models /srv/ai-tools/pipeline /data/media/input /data/media/output /data/media/temp
sudo chown -R $USER:$USER /srv/ai-tools /data/media
```

## 3. Vorbereitung: Hugging Face Token für Diarisierung

Das Diarisierungsmodell pyannote/speaker-diarization-3.1 ist kostenfrei auf Hugging Face verfügbar, erfordert jedoch eine einmalige Nutzungsbestätigung:

1. Kostenlosen Account erstellen auf https://huggingface.co
2. Die Modellseite https://huggingface.co/pyannote/speaker-diarization-3.1 aufrufen und die Lizenzbedingungen akzeptieren.
3. Die Segmentierungsseite https://huggingface.co/pyannote/segmentation-3.0 aufrufen und ebenfalls akzeptieren.
4. Unter Settings, Access Tokens einen Lese Token (Read Token) erstellen.

## 4. Docker Setup für AMD ROCm

Im Ordner `/srv/ai-tools/pipeline` werden die folgenden zwei Dateien erstellt.

### Datei 1: Dockerfile

```dockerfile
FROM rocm/pytorch:rocm6.2_ubuntu22.04_py3.10_pytorch_2.3.0
ENV DEBIAN_FRONTEND=noninteractive
ENV PYTHONUNBUFFERED=1
RUN apt-get update && apt-get install -y \
    ffmpeg \
    git \
    libsndfile1 \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
RUN pip install --no-cache-dir \
    whisperx \
    pyannote.audio \
    torchaudio \
    pandas
COPY cut_engine.py /app/cut_engine.py
ENTRYPOINT ["python3", "/app/cut_engine.py"]
```

### Datei 2: cut_engine.py

Dieses Skript steuert die gesamte Logik: Audio extrahieren, WhisperX mit Diarisierung ausführen, Füllwörter filtern, Dialogpausen schützen und das Video verlustfrei per FFmpeg schneiden.

```python
import os
import sys
import json
import subprocess
import torch
import whisperx
import gc

FILLER_WORDS = {
    "äh", "ähm", "ahm", "uhm", "uh", "er", "ah", "mh", "mhm", "öhm", "öh"
}

def run_cmd(cmd):
    subprocess.run(cmd, shell=True, check=True)

def process_video(video_path, output_path, hf_token, device="cuda"):
    base_name = os.path.splitext(os.path.basename(video_path))[0]
    temp_audio = f"/data/media/temp/{base_name}_temp.wav"

    print(f"[1/4] Extrahiere 16kHz Audio aus {video_path}...")
    run_cmd(f'ffmpeg -y -i "{video_path}" -vn -acodec pcm_s16le -ar 16000 -ac 1 "{temp_audio}"')

    print("[2/4] Starte WhisperX Transkription...")
    # 8GB VRAM Optimierung: float16 und moderate Batch-Size
    compute_type = "float16" if torch.cuda.is_available() else "int8"
    model = whisperx.load_model("large-v3", device, compute_type=compute_type, download_root="/srv/ai-tools/models")
    audio = whisperx.load_audio(temp_audio)
    result = model.transcribe(audio, batch_size=4)

    # Alignment für wortgenaue Zeitstempel
    model_a, metadata = whisperx.load_align_model(language_code=result["language"], device=device)
    result = whisperx.align(result["segments"], model_a, metadata, audio, device, return_char_alignments=False)

    # Speicher freigeben für Diarisierung
    del model
    del model_a
    gc.collect()
    torch.cuda.empty_cache()

    print("[3/4] Starte Sprecher-Diarisierung (Pyannote)...")
    diarize_model = whisperx.DiarizationPipeline(use_auth_token=hf_token, device=device)
    diarize_segments = diarize_model(audio)
    result = whisperx.assign_word_speakers(diarize_segments, result)

    del diarize_model
    gc.collect()
    torch.cuda.empty_cache()

    # Schnitte berechnen
    print("[4/4] Analysiere Füllwörter und erstelle Schnittliste...")
    cut_intervals = []

    for segment in result["segments"]:
        speaker = segment.get("speaker", "UNKNOWN")
        words = segment.get("words", [])

        for w in words:
            word_clean = w.get("word", "").strip().lower().strip(".,!?:;\"'")
            if word_clean in FILLER_WORDS:
                if "start" in w and "end" in w:
                    # Füllwort gefunden: Intervall zum Herausschneiden markieren
                    cut_intervals.append((w["start"], w["end"], speaker, word_clean))
                    print(f"  -> Entferne Füllwort '{word_clean}' von {speaker} ({w['start']:.2f}s bis {w['end']:.2f}s)")

    # Audio-Länge ermitteln
    probe_cmd = f'ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1:nokey=1 "{temp_audio}"'
    total_duration = float(subprocess.check_output(probe_cmd, shell=True).decode().strip())

    # Zeitbereiche berechnen, die BEHALTEN werden sollen
    keep_segments = []
    current_pos = 0.0

    for start, end, _, _ in sorted(cut_intervals, key=lambda x: x[0]):
        if start > current_pos:
            keep_segments.append((current_pos, start))
        current_pos = max(current_pos, end)

    if current_pos < total_duration:
        keep_segments.append((current_pos, total_duration))

    # Verlustfreier Schnitt via FFmpeg Concat Demuxer
    concat_list_file = f"/data/media/temp/{base_name}_concat.txt"
    segment_files = []

    with open(concat_list_file, "w") as f:
        for idx, (k_start, k_end) in enumerate(keep_segments):
            dur = k_end - k_start
            if dur < 0.1: # Sehr kurze Fragmente überspringen
                continue
            seg_file = f"/data/media/temp/seg_{idx}_{base_name}.mp4"
            # Stream Copy (-c copy) für 100% verlustfreie UHD-Qualität
            run_cmd(f'ffmpeg -y -ss {k_start} -i "{video_path}" -t {dur} -c copy -avoid_negative_ts make_zero "{seg_file}"')
            f.write(f"file '{seg_file}'\n")
            segment_files.append(seg_file)

    print(f"Füge fertiges Video zusammen -> {output_path}")
    run_cmd(f'ffmpeg -y -f concat -safe 0 -i "{concat_list_file}" -c copy "{output_path}"')

    # Aufräumen
    os.remove(temp_audio)
    os.remove(concat_list_file)
    for sf in segment_files:
        if os.path.exists(sf):
            os.remove(sf)
    print("Fertiggestellt!")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Verwendung: python3 cut_engine.py <Eingabedatei> <Ausgabedatei> <HF_TOKEN>")
        sys.exit(1)

    in_video = sys.argv[1]
    out_video = sys.argv[2]
    token = sys.argv[3]
    process_video(in_video, out_video, token)
```

## 5. Image bauen und ausführen

### Schritt A: Docker Image erstellen

Navigiere in den Ordner und baue das Image:

```bash
cd /srv/ai-tools/pipeline
docker build -t rocm-video-cut .
```

### Schritt B: Video verarbeiten

Platziere deine UHD Aufnahme (zum Beispiel interview_uhd.mp4) im Ordner `/data/media/input/`.

Starte die Verarbeitung mit Zugriff auf die AMD GPU:

```bash
docker run --rm \
    --device=/dev/kfd \
    --device=/dev/dri \
    --group-add video \
    --group-add render \
    --security-opt seccomp=unconfined \
    -v /srv/ai-tools/models:/srv/ai-tools/models \
    -v /data/media:/data/media \
    rocm-video-cut \
    "/data/media/input/interview_uhd.mp4" \
    "/data/media/output/interview_geschnitten.mp4" \
    "DEIN_HUGGINGFACE_TOKEN"
```

## 6. Wichtige Details für 8 GB VRAM und saubere Dialoge

**VRAM Schutz bei 8 GB**
Im Skript wird nach der WhisperX Transkription der Speicher explizit mit `del model`, `gc.collect()` und `torch.cuda.empty_cache()` freigegeben, bevor das Diarisierungsmodell geladen wird. Dadurch passen selbst große 4K Dateien problemlos durch die 8 GB GPU.

**100% Erhalt der UHD Qualität**
Da FFmpeg mit `-c copy` arbeitet, werden die Videoframes nicht neu gerendert oder komprimiert. Die Bildschärfe und die Farben der Kameraaufnahme bleiben bitgenau erhalten.

**Natürliche Sprecherpausen**
Weil gezielt nur Füllwörter und keine regulären Sprechpausen zwischen verschiedenen Personen weggeschnitten werden, bleibt der natürliche Gesprächsrhythmus zwischen zwei oder mehr Personen vollkommen intakt.

## 7. Fachliche Prüfung des Entwurfs (Stand 2026-08-18)

Der ursprüngliche Entwurf wurde vor dem Bau kritisch geprüft. Dabei wurden fünf Kernprobleme identifiziert, die vor der Umsetzung berücksichtigt werden müssen.

**CTranslate2 und ROCm**
WhisperX baut auf CTranslate2 (faster whisper) auf. CTranslate2 hat keinen nativen ROCm/HIP Support, sondern unterstützt primär Nvidia CUDA und x86 CPUs. Der Aufruf von `whisperx.load_model(..., device="cuda")` wird auf dem ROCm Server vermutlich mit einem Initialisierungsfehler abbrechen, nicht nur langsamer laufen. Alignment (wav2vec2 über Hugging Face Transformers) und Diarisierung (Pyannote) laufen dagegen auf reinem PyTorch und sollten auf ROCm funktionieren. Empfohlene Architektur Alternative: nur den Transkriptionsschritt ersetzen, zum Beispiel durch die Hugging Face Transformers Whisper Pipeline oder OpenAI Whisper, und das Ergebnis in das von `whisperx.align` erwartete Segmentformat überführen. Der Rest der WhisperX Pipeline (Alignment, Diarisierung) bleibt dabei erhalten. Alternativ Transkription auf der CPU laufen lassen, während Alignment und Diarisierung auf der ROCm GPU laufen.

**Keyframe Problem bei Stream Copy**
Ein Schnitt per `-c copy` trifft bei UHD MP4 mit größeren GOP Abständen nur Keyframes, keine frame genauen Positionen. Kurze Füllwörter von einigen hundert Millisekunden lassen sich so nicht sauber isolieren, es kann zu eingefrorenem Bild oder Ton/Bild Versatz kommen. Lösung: Smart Cut Ansatz, bei dem nur kurze Übergangsfenster um die tatsächlichen Schnittkanten neu kodiert werden (CPU mit libx264, CRF 17 für optisch verlustfreie Qualität, oder AMD GPU beschleunigt über VAAPI mit hevc_vaapi/h264_vaapi), während der Großteil des Videos weiterhin per Stream Copy erhalten bleibt. Zusätzlich sollten Schnitte in der Tonspur minimale Überblendungen von 5 bis 10 Millisekunden erhalten, um Knackgeräusche zu vermeiden. Für VAAPI müssen im Docker Image zusätzlich Mesa VAAPI Treiber installiert werden, die im ROCm PyTorch Basisimage nicht automatisch enthalten sind.

**Consumer GPU Workaround**
Consumer Karten wie RX 6600, 6700 oder 7600 werden von ROCm standardmäßig nicht offiziell zertifiziert. Ohne Überschreibung der Architekturversion verweigert der Treiber den Start. Im Docker Setup muss die Umgebungsvariable `HSA_OVERRIDE_GFX_VERSION` gesetzt werden, für RDNA2 Karten (RX 6000 Serie) auf `10.3.0`, für RDNA3 Karten (RX 7000 Serie) auf `11.0.0`. Das genaue GPU Modell im Server steht noch aus und sollte vor dem Bau bestätigt werden. Vor dem Docker Test empfiehlt sich außerdem, auf dem Host selbst (außerhalb jedes Containers) einmal `rocminfo` beziehungsweise `rocm-smi` laufen zu lassen, um sicherzustellen, dass der Kernel Treiber (amdgpu/KFD) die Karte korrekt erkennt.

**Jump Cuts bei Einzelkamera Material**
Bei Aufnahmen von einer einzelnen, fest stehenden Kamera erzeugt automatisches Herausschneiden von Füllwörtern unvermeidbar sichtbare Bildsprünge (Kopf oder Handposition ändert sich zwischen den beiden Enden des Schnitts). Das ist eine redaktionelle Entscheidung, keine rein technische. Siehe Testergebnis unten für die Bewertung bei JJs eigenem Material sowie konkrete Lösungsansätze.

**Wartungsrisiko ROCm**
ROCm auf Consumer Grafikkarten läuft nicht dauerhaft geräuschlos. Updates von Ubuntu, Kernel, PyTorch oder Docker können GPU Passthrough oder die Architektur Overrides erneut anpassungsbedürftig machen. Das sollte als wiederkehrender Wartungsaufwand eingeplant werden, nicht als einmalige Einrichtung.

Zusätzlich empfohlen: Versionen von whisperx und pyannote.audio im Dockerfile fest pinnen statt ungepinnt zu installieren, da beide Projekte ihre Abhängigkeiten häufig ändern und ein ungepinntes `pip install` beim nächsten Build brechen kann.

## 8. Praxistest mit auto-editor als Zwischenschritt

Bevor Zeit in den vollständigen ROCm Aufbau fließt, wurde als pragmatischer Zwischenschritt das schlanke Open Source Tool auto-editor (https://auto-editor.com, https://github.com/WyattBlue/auto-editor) genutzt, um am echten Bildmaterial zu prüfen, wie sich automatischer Schnitt grundsätzlich anfühlt, ohne bereits in die komplexe WhisperX/Pyannote/ROCm Pipeline zu investieren.

**Installation unter Windows**
Die Installation per `pip install auto-editor` schlug lokal fehl. Funktioniert hat stattdessen der Download der vorkompilierten Windows Binärdatei `auto-editor-windows-x86_64.exe` von der GitHub Releases Seite (aktuell Version 31.5.0), ohne dass dafür Python benötigt wird.

**Lizenzhinweis**
Der Quellcode von auto-editor steht unter der Unlicense (public domain, keine Einschränkungen). Die kompilierte Windows exe von der Releases Seite hat aber offenbar eine eigene, kommerzielle Beschränkung: Bei UHD Auflösung ohne Lizenz wird automatisch auf 75 Prozent herunterskaliert (3840x2160 zu 2880x1620). Für Testzwecke unkritisch. Für eine spätere Produktivnutzung mit genau diesem Tool müsste entweder eine Lizenz erworben werden (siehe app.auto-editor.com) oder selbst aus dem Quellcode kompiliert werden, was laut Dokumentation ein WSL Umfeld mit Nim, Cmake, Meson und Ninja voraussetzt.

**Testergebnis**
Getestet an einem echten UHD Testvideo, Verarbeitungszeit rund 80 Sekunden. Füllwörter beziehungsweise Grundlaute wurden zuverlässig herausgeschnitten. Eine klangliche Verbesserung gab es erwartungsgemäß nicht, auto-editor schneidet nur, bearbeitet den Ton nicht (dafür wäre zum Beispiel Auphonic zuständig). Die Jump Cuts an den Schnittstellen sind spürbar, wurden von JJ aber nur als "eine Spur" störend eingestuft, kein Dealbreaker.

**Bewertung**
Automatischer Schnitt funktioniert grundsätzlich für JJs Content. Feinschliff bei den Übergängen ist sinnvoll. Zwei Lösungsansätze für weichere Schnitte, unabhängig davon ob mit auto-editor oder der eigenen Pipeline umgesetzt:

1. Kurzer Zoom Punch In direkt am Schnittpunkt (minimaler Zoom von wenigen Prozent), lässt den Schnitt wie eine bewusste redaktionelle Entscheidung wirken statt wie einen Fehler.
2. Sehr kurze Bildüberblendung (wenige Frames) an der Schnittstelle, zusätzlich zu den ohnehin geplanten Audio Crossfades.

## 9. Fazit und weiteres Vorgehen

Die Grundsatzfrage, ob automatischer Schnitt für JJs Material funktioniert, ist positiv beantwortet. Als Nächstes steht der gezielte Aufbau der lokalen WhisperX/Pyannote/ROCm Pipeline an, unter Berücksichtigung der in Abschnitt 7 genannten Kernprobleme (Transkriptions-Backend Wechsel weg von CTranslate2, Smart Cut statt reinem Stream Copy, korrekter HSA Override, Versions-Pinning) sowie der beiden Lösungsansätze gegen harte Jump Cuts aus Abschnitt 8.

Noch offen: genaues GPU Modell (für den korrekten HSA_OVERRIDE_GFX_VERSION Wert), tatsächliche CPU Realtime Faktor Messung für die Transkription.
