#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

// NovaPhonic Kernlogik. Ruft drei eigenständige Programme (Sidecars) im Hintergrund
// auf: auto-editor (Schnitt), deep-filter/DeepFilterNet (Entrauschen), ffmpeg
// (Lautstärke Angleichung, Audio Extraktion, Remux). Es werden ausschließlich
// Dateipfade verarbeitet, nie Videoinhalte selbst in den Arbeitsspeicher der
// Oberfläche geladen.
//
// Alle Zwischenschritte (geschnittenes Video, rohe/entrauschte Tonspur, usw.)
// laufen in einem echten temporären Systemordner (std::env::temp_dir()), nicht im
// Ordner der Eingabedatei. Grund: Bei einem echten Testlauf am 2026-08-19 lag das
// Eingabevideo in einem Nextcloud-synchronisierten Ordner, der Sync-Client hat
// neu entstehende Zwischen-Dateien sofort selbst kurz geöffnet, was zu "Zugriff
// verweigert" Fehlern führte, sobald NovaPhonic dieselbe Datei gleich danach
// weiterverarbeiten wollte. Der temporäre Arbeitsordner wird bei einem Fehler
// sofort wieder vollständig gelöscht, bei Erfolg bleibt nur das fertige Ergebnis
// bis zum Speichern über den "Speichern unter" Dialog liegen, siehe
// `save_output_as`, das räumt danach ebenfalls auf.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickedFile {
    path: String,
    size_bytes: u64,
}

#[tauri::command]
fn pick_video_file() -> Option<PickedFile> {
    let file = rfd::FileDialog::new()
        .add_filter("Video", &["mp4", "mov", "mkv", "avi", "webm"])
        .pick_file()?;
    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    Some(PickedFile {
        path: file.to_string_lossy().to_string(),
        size_bytes: size,
    })
}

#[tauri::command]
fn save_output_as(source_path: String, suggested_name: String) -> Result<String, String> {
    let dialog = rfd::FileDialog::new().set_file_name(&suggested_name);
    match dialog.save_file() {
        Some(dest) => {
            fs::copy(&source_path, &dest)
                .map_err(|e| format!("Konnte Datei nicht speichern: {e}"))?;
            cleanup_temp_work_dir(&source_path);
            Ok(dest.to_string_lossy().to_string())
        }
        None => Err("abgebrochen".to_string()),
    }
}

// Löscht den temporären Arbeitsordner einer fertigen Ergebnisdatei, nachdem sie
// erfolgreich an ihr endgültiges Ziel kopiert wurde. Sicherheitshalber nur, wenn
// der Pfad wirklich im System-Temp-Ordner liegt und unserem eigenen Namensmuster
// entspricht, damit hier niemals versehentlich ein echter Nutzerordner betroffen
// sein kann.
fn cleanup_temp_work_dir(source_path: &str) {
    let path = PathBuf::from(source_path);
    let Some(work_dir) = path.parent() else {
        return;
    };
    let Some(folder_name) = work_dir.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    if work_dir.starts_with(std::env::temp_dir()) && folder_name.starts_with("novaphonic-") {
        let _ = fs::remove_dir_all(work_dir);
    }
}

// Wichtig: Tauri spricht Sidecar Programme zur Laufzeit nur über ihren bloßen
// Namen an (z.B. "auto-editor"), der Ordnerpfad "binaries/" wird ausschließlich
// beim Einrichten in tauri.conf.json (externalBin) und in den Capabilities
// verwendet, nicht beim eigentlichen Aufruf hier im Code.
const SIDECARS: [(&str, &str); 3] = [
    ("auto-editor", "--version"),
    ("deep-filter", "--version"),
    ("ffmpeg", "-version"),
];

#[tauri::command]
async fn check_tools(app: AppHandle) -> Vec<String> {
    let mut missing = Vec::new();
    for (sidecar_name, version_flag) in SIDECARS {
        let ok = match app.shell().sidecar(sidecar_name) {
            Ok(cmd) => cmd
                .args([version_flag])
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false),
            Err(_) => false,
        };
        if !ok {
            // Kurzer, für Menschen lesbarer Name statt des internen Sidecar-Pfads.
            let readable = sidecar_name.trim_start_matches("binaries/");
            missing.push(readable.to_string());
        }
    }
    missing
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProcessOptions {
    input_path: String,
    margin_seconds: f64,
    denoise: bool,
    loudnorm: bool,
    loudnorm_target: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProcessResult {
    output_path: String,
}

#[derive(Clone, Serialize)]
struct ProgressPayload {
    step: String,
    status: String,
    detail: Option<String>,
    log: Option<String>,
}

fn emit_progress(
    app: &AppHandle,
    step: &str,
    status: &str,
    detail: Option<String>,
    log: Option<String>,
) {
    let _ = app.emit(
        "pipeline-progress",
        ProgressPayload {
            step: step.to_string(),
            status: status.to_string(),
            detail,
            log,
        },
    );
}

async fn run_sidecar(
    app: &AppHandle,
    sidecar_name: &str,
    args: Vec<String>,
    step: &str,
) -> Result<(), String> {
    let cmd = app
        .shell()
        .sidecar(sidecar_name)
        .map_err(|e| e.to_string())?
        .args(args);
    let (mut rx, _child) = cmd.spawn().map_err(|e| e.to_string())?;

    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(line) => {
                emit_progress(
                    app,
                    step,
                    "running",
                    None,
                    Some(String::from_utf8_lossy(&line).trim().to_string()),
                );
            }
            CommandEvent::Stderr(line) => {
                emit_progress(
                    app,
                    step,
                    "running",
                    None,
                    Some(String::from_utf8_lossy(&line).trim().to_string()),
                );
            }
            CommandEvent::Error(err) => {
                return Err(err);
            }
            CommandEvent::Terminated(payload) => {
                if payload.code != Some(0) {
                    return Err(format!(
                        "{sidecar_name} wurde mit Code {:?} beendet",
                        payload.code
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[tauri::command]
async fn process_video(app: AppHandle, options: ProcessOptions) -> Result<ProcessResult, String> {
    let input = PathBuf::from(&options.input_path);
    if !input.exists() {
        return Err("Eingabedatei nicht gefunden.".to_string());
    }
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video")
        .to_string();
    let ext = input
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4")
        .to_string();

    // Eigener, garantiert eindeutiger Arbeitsordner im System-Temp-Verzeichnis,
    // pro Verarbeitungslauf neu, damit sich mehrere Läufe nie in die Quere kommen.
    let work_dir =
        std::env::temp_dir().join(format!("novaphonic-{stem}-{}", std::process::id()));
    fs::create_dir_all(&work_dir).map_err(|e| e.to_string())?;

    match run_pipeline(&app, &options, &input, &work_dir, &stem, &ext).await {
        Ok(final_path) => Ok(ProcessResult {
            output_path: final_path.to_string_lossy().to_string(),
        }),
        Err(e) => {
            // Bei einem Fehler bleibt nichts Halbfertiges im Temp-Ordner liegen.
            let _ = fs::remove_dir_all(&work_dir);
            Err(e)
        }
    }
}

async fn run_pipeline(
    app: &AppHandle,
    options: &ProcessOptions,
    input: &Path,
    work_dir: &Path,
    stem: &str,
    ext: &str,
) -> Result<PathBuf, String> {
    // ---------- 1) Entrauschen (DeepFilterNet), optional ----------
    // Bewusst VOR dem Schnitt statt danach (bis 2026-08-19 war das andersherum):
    // auto-editor erkennt Stille anhand einer festen Lautstärke-Schwelle
    // (Standard ca. -28 dB). Enthält die Aufnahme ein leises, aber durchgehendes
    // Hintergrundgeräusch (Raumbrummen, Lüfter), liegt das oft über dieser
    // Schwelle, auto-editor findet dann an keiner Stelle echte Stille und
    // schneidet nichts weg. Nutzerfeedback vom 2026-08-19: "Der Clip ist
    // durchgehend ungeschnitten, die natürlichen Sprechpausen sind noch
    // vollständig erhalten." Wenn DeepFilterNet das Hintergrundgeräusch schon
    // vor dem Schnitt entfernt, bleibt an den Pausen echte digitale Stille
    // übrig, und die Standard-Schwelle von auto-editor funktioniert wieder
    // zuverlässig, auch bei künftigen Aufnahmen mit anderem Grundrauschen,
    // ohne dass man die Schwelle manuell pro Video nachjustieren muss.
    let mut current = input.to_path_buf();
    if options.denoise {
        emit_progress(app, "denoise", "running", Some("startet…".into()), None);
        let raw_audio = work_dir.join(format!("{stem}_audio.wav"));
        let denoised_audio = work_dir.join(format!("{stem}_entrauscht.wav"));

        run_sidecar(
            &app,
            "ffmpeg",
            vec![
                "-y".into(),
                "-i".into(),
                current.to_string_lossy().to_string(),
                "-vn".into(),
                "-acodec".into(),
                "pcm_s16le".into(),
                "-ar".into(),
                "48000".into(),
                "-ac".into(),
                "1".into(),
                raw_audio.to_string_lossy().to_string(),
            ],
            "denoise",
        )
        .await
        .map_err(|e| {
            emit_progress(&app, "denoise", "error", Some(e.clone()), None);
            e
        })?;

        // deep-filter erwartet bei -o einen AusgabeORDNER, keine Zieldatei, und
        // schreibt dort eine Datei mit demselben Namen wie die Eingabedatei hinein
        // (per echtem Testlauf bestätigt, 2026-08-19). Deshalb erst in einen eigenen
        // Zwischenordner schreiben lassen und die entstandene Datei danach auf den
        // gewünschten Namen umbenennen.
        let denoise_out_dir = work_dir.join(format!("{stem}_denoise_tmp"));
        fs::create_dir_all(&denoise_out_dir).map_err(|e| e.to_string())?;
        run_sidecar(
            &app,
            "deep-filter",
            vec![
                raw_audio.to_string_lossy().to_string(),
                // Begrenzt, wie stark DeepFilterNet maximal absenken darf. Ohne
                // dieses Limit greift das Modell an manchen Stellen so stark ein,
                // dass mit dem Rauschen auch ein Teil der hohen Frequenzen der
                // Stimme mit entfernt wird, das klingt dann dumpf und mattiert
                // statt klar. Nutzerfeedback vom 2026-08-19: Klang zu dumpf im
                // Vergleich zu Auphonic. 30 dB ist ein vorsichtiger Mittelweg,
                // lässt sich bei Bedarf noch weiter anpassen.
                "--atten-lim-db".into(),
                "30".into(),
                "-o".into(),
                denoise_out_dir.to_string_lossy().to_string(),
            ],
            "denoise",
        )
        .await
        .map_err(|e| {
            emit_progress(&app, "denoise", "error", Some(e.clone()), None);
            e
        })?;
        let denoise_result = denoise_out_dir.join(
            raw_audio
                .file_name()
                .ok_or_else(|| "Ungültiger Audio Dateiname.".to_string())?,
        );
        fs::rename(&denoise_result, &denoised_audio)
            .or_else(|_| fs::copy(&denoise_result, &denoised_audio).map(|_| ()))
            .map_err(|e| {
                let msg = format!(
                    "Entrauschte Datei nicht am erwarteten Ort {} gefunden: {e}",
                    denoise_result.display()
                );
                emit_progress(&app, "denoise", "error", Some(msg.clone()), None);
                msg
            })?;
        let _ = fs::remove_dir_all(&denoise_out_dir);

        let remuxed = work_dir.join(format!("{stem}_entrauscht_video.{ext}"));
        run_sidecar(
            &app,
            "ffmpeg",
            vec![
                "-y".into(),
                "-i".into(),
                current.to_string_lossy().to_string(),
                "-i".into(),
                denoised_audio.to_string_lossy().to_string(),
                "-map".into(),
                "0:v:0".into(),
                "-map".into(),
                "1:a:0".into(),
                "-c:v".into(),
                "copy".into(),
                "-shortest".into(),
                remuxed.to_string_lossy().to_string(),
            ],
            "denoise",
        )
        .await
        .map_err(|e| {
            emit_progress(&app, "denoise", "error", Some(e.clone()), None);
            e
        })?;

        let _ = fs::remove_file(&raw_audio);
        let _ = fs::remove_file(&denoised_audio);
        current = remuxed;
        emit_progress(&app, "denoise", "done", Some("fertig".into()), None);
    } else {
        emit_progress(&app, "denoise", "done", Some("übersprungen".into()), None);
    }

    // ---------- 2) Schnitt (auto-editor) ----------
    emit_progress(app, "cut", "running", Some("startet…".into()), None);
    let cut_path = work_dir.join(format!("{stem}_geschnitten.{ext}"));
    let margin = format!("{}sec", options.margin_seconds);
    // Läuft jetzt auf `current`, also auf der schon entrauschten Datei (falls
    // Entrauschen aktiv war) statt auf der Originaldatei, siehe Kommentar oben.
    let cut_input = current.clone();
    run_sidecar(
        &app,
        "auto-editor",
        vec![
            cut_input.to_string_lossy().to_string(),
            "--margin".into(),
            margin,
            // Bitte beim ersten Testlauf prüfen, ob dieses Flag in der installierten
            // auto-editor Version noch so heißt (verhindert, dass sich nach dem
            // Schneiden automatisch ein Videoplayer öffnet).
            "--no-open".into(),
            // Weicher Übergang statt hartem Jump Cut: auto-editor blendet Bild und
            // Ton an jedem Schnittpunkt kurz ineinander (0,15s), statt hart zu
            // schneiden. Die zweite Zahl (0,05s) sorgt dafür, dass auch sehr kurze
            // Schnitte (z.B. ein einzelnes rausgeschnittenes Füllwort) noch eine
            // Überblendung bekommen, auto-editor würde sonst nur Schnitte ab 1
            // Sekunde Länge überblenden. Nutzerfeedback vom 2026-08-19: harte,
            // unruhige Schnittpunkte im Bild. Wichtig: die Zeitangaben brauchen
            // zwingend eine Einheit ("sec"), eine nackte Kommazahl wie "0.15"
            // lehnt auto-editor mit "Time format expects an integer" ab (per
            // echtem Testlauf am 2026-08-19 bestätigt).
            "--transition".into(),
            "dissolve:0.15sec:0.05sec".into(),
            "-o".into(),
            cut_path.to_string_lossy().to_string(),
        ],
        "cut",
    )
    .await
    .map_err(|e| {
        emit_progress(&app, "cut", "error", Some(e.clone()), None);
        e
    })?;
    // Zwischenergebnis aus Schritt 1 aufräumen, aber niemals die Originaldatei
    // des Nutzers löschen (die liegt außerhalb des Temp-Ordners, falls
    // Entrauschen deaktiviert war und `cut_input` noch auf sie zeigt).
    if cut_input.as_path() != input {
        let _ = fs::remove_file(&cut_input);
    }
    current = cut_path;
    emit_progress(&app, "cut", "done", Some("fertig".into()), None);

    // ---------- 3) Klangveredelung & Lautstärke (FFmpeg), optional ----------
    let final_path = work_dir.join(format!("{stem}_nova.{ext}"));
    if options.loudnorm {
        emit_progress(&app, "loudnorm", "running", Some("startet…".into()), None);
        // Bewusst Einzelpass-Loudnorm statt klassischem Zweipass-Verfahren: kein
        // Parsen von FFmpeg Messwerten aus stderr nötig, dadurch robuster. Etwas
        // weniger präzise als Zweipass, für Sprachaufnahmen in der Praxis meist
        // ausreichend. Lässt sich als Ausbaustufe später nachrüsten.
        //
        // Vor loudnorm zusätzlich vier Klangschritte, weil reines loudnorm nur die
        // Lautheit regelt, aber keine Klangfarbe. Stand 2026-08-19, zweite
        // Nachschärfung: nach dem ersten Versuch (Low/High Shelf) war der Klang
        // laut Nutzerfeedback immer noch "bassbetont und im mittleren Bereich
        // etwas topfig". Deshalb jetzt: 1) echter Hochpassfilter bei 90 Hz statt
        // eines sanften Shelfs, entfernt Trittschall/Dröhnen konsequenter (Wert
        // stammt aus einer externen Analyse-Empfehlung, die JJ eingeholt hat).
        // 2) gezielte, moderate Absenkung um 350 Hz, genau der Bereich, der für
        // den "topfigen" Klang bei nah sitzenden Lavaliermikrofonen typisch ist,
        // das hatte die externe Empfehlung noch nicht abgedeckt. 3) ein
        // schmalerer Presence Boost um 3500 Hz statt eines breiten High Shelf ab
        // 4000 Hz, wirkt natürlicher (ebenfalls aus der externen Empfehlung
        // übernommen). 4) unveränderter, leichter Kompressor für gleichmäßigere
        // Aussteuerung. Alle Werte bewusst moderat, um keine unnatürlichen oder
        // metallischen Artefakte zu erzeugen.
        let filter = format!(
            "highpass=f=90,equalizer=f=350:t=q:w=1.5:g=-3,equalizer=f=3500:t=q:w=1.2:g=3.5,acompressor=threshold=0.1:ratio=2.5:attack=15:release=200:makeup=1.5,loudnorm=I={}:TP=-1.5:LRA=11",
            options.loudnorm_target
        );
        run_sidecar(
            &app,
            "ffmpeg",
            vec![
                "-y".into(),
                "-i".into(),
                current.to_string_lossy().to_string(),
                "-af".into(),
                filter,
                "-c:v".into(),
                "copy".into(),
                final_path.to_string_lossy().to_string(),
            ],
            "loudnorm",
        )
        .await
        .map_err(|e| {
            emit_progress(&app, "loudnorm", "error", Some(e.clone()), None);
            e
        })?;
        let _ = fs::remove_file(&current);
        emit_progress(&app, "loudnorm", "done", Some("fertig".into()), None);
    } else {
        fs::rename(&current, &final_path).or_else(|_| fs::copy(&current, &final_path).map(|_| ()))
            .map_err(|e| e.to_string())?;
        emit_progress(&app, "loudnorm", "done", Some("übersprungen".into()), None);
    }

    Ok(final_path)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            pick_video_file,
            save_output_as,
            check_tools,
            process_video
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten der Tauri Anwendung");
}
