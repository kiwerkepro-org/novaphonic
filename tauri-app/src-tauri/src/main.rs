#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

// NovaPhonic Kernlogik. Ruft drei eigenständige Programme (Sidecars) im Hintergrund
// auf: auto-editor (Schnitt), deep-filter/DeepFilterNet (Entrauschen), ffmpeg
// (Lautstärke Angleichung, Audio Extraktion, Remux). Es werden ausschließlich
// Dateipfade verarbeitet, nie Videoinhalte selbst in den Arbeitsspeicher der
// Oberfläche geladen.
//
// Wichtiger Hinweis für den ersten echten Build: In dieser Entwicklungsumgebung
// steht kein Rust Compiler zur Verfügung, dieser Code wurde also nicht kompiliert,
// nur sorgfältig von Hand geprüft. Zwei Stellen bitte beim ersten Testlauf gezielt
// gegenprüfen: 1) die genauen Kommandozeilen-Optionen von `deep-filter` (siehe
// `deep-filter --help`, hier als `-o <Zieldatei>` angenommen), 2) ob `auto-editor`
// den Flag `--no-open` in der installierten Version so kennt. Beides ist unten mit
// Kommentaren markiert.

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
            Ok(dest.to_string_lossy().to_string())
        }
        None => Err("abgebrochen".to_string()),
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
        .unwrap_or("video");
    let ext = input.extension().and_then(|s| s.to_str()).unwrap_or("mp4");
    let parent = input.parent().unwrap_or_else(|| Path::new("."));

    // ---------- 1) Schnitt (auto-editor) ----------
    emit_progress(&app, "cut", "running", Some("startet…".into()), None);
    let cut_path = parent.join(format!("{stem}_geschnitten.{ext}"));
    let margin = format!("{}sec", options.margin_seconds);
    run_sidecar(
        &app,
        "auto-editor",
        vec![
            input.to_string_lossy().to_string(),
            "--margin".into(),
            margin,
            // Bitte beim ersten Testlauf prüfen, ob dieses Flag in der installierten
            // auto-editor Version noch so heißt (verhindert, dass sich nach dem
            // Schneiden automatisch ein Videoplayer öffnet).
            "--no-open".into(),
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
    emit_progress(&app, "cut", "done", Some("fertig".into()), None);

    let mut current = cut_path;

    // ---------- 2) Entrauschen (DeepFilterNet), optional ----------
    if options.denoise {
        emit_progress(&app, "denoise", "running", Some("startet…".into()), None);
        let raw_audio = parent.join(format!("{stem}_audio.wav"));
        let denoised_audio = parent.join(format!("{stem}_entrauscht.wav"));

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

        // Achtung, bitte gegen `deep-filter --help` prüfen: hier wird eine exakte
        // Ausgabedatei per -o angenommen. Manche deep-filter Versionen erwarten
        // stattdessen einen Ausgabeordner (z.B. -o <Ordner>), in dem eine Datei
        // mit demselben Namen wie die Eingabe erzeugt wird. Falls das der Fall
        // ist, muss diese Stelle entsprechend angepasst werden.
        run_sidecar(
            &app,
            "deep-filter",
            vec![
                raw_audio.to_string_lossy().to_string(),
                "-o".into(),
                denoised_audio.to_string_lossy().to_string(),
            ],
            "denoise",
        )
        .await
        .map_err(|e| {
            emit_progress(&app, "denoise", "error", Some(e.clone()), None);
            e
        })?;

        let remuxed = parent.join(format!("{stem}_entrauscht_video.{ext}"));
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
        let _ = fs::remove_file(&current);
        current = remuxed;
        emit_progress(&app, "denoise", "done", Some("fertig".into()), None);
    } else {
        emit_progress(&app, "denoise", "done", Some("übersprungen".into()), None);
    }

    // ---------- 3) Lautstärke & Klangbalance (FFmpeg loudnorm), optional ----------
    let final_path = parent.join(format!("{stem}_nova.{ext}"));
    if options.loudnorm {
        emit_progress(&app, "loudnorm", "running", Some("startet…".into()), None);
        // Bewusst Einzelpass-Loudnorm statt klassischem Zweipass-Verfahren: kein
        // Parsen von FFmpeg Messwerten aus stderr nötig, dadurch robuster. Etwas
        // weniger präzise als Zweipass, für Sprachaufnahmen in der Praxis meist
        // ausreichend. Lässt sich als Ausbaustufe später nachrüsten.
        let filter = format!("loudnorm=I={}:TP=-1.5:LRA=11", options.loudnorm_target);
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

    Ok(ProcessResult {
        output_path: final_path.to_string_lossy().to_string(),
    })
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
