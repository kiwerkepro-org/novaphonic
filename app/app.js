/*
 * app.js
 * NovaPhonic Oberfläche. Ruft ausschließlich Tauri Rust Commands auf, die im
 * Hintergrund auto-editor, DeepFilterNet und FFmpeg als eigenständige Programme
 * (Sidecars) ausführen. Die Videodatei selbst wird nie in dieses Skript geladen,
 * es werden nur Dateipfade zwischen Oberfläche und Rust ausgetauscht.
 */
(function () {
  "use strict";

  function $(id) { return document.getElementById(id); }
  const root = document.documentElement;

  function isTauri() {
    return typeof window.__TAURI__ !== 'undefined' && window.__TAURI__.core;
  }

  // ---------- Theme ----------
  const THEME_KEY = 'novaphonic-theme';
  const themeToggle = $('themeToggle');
  const themeLabel = $('themeLabel');

  function applyTheme(theme) {
    root.setAttribute('data-theme', theme);
    themeLabel.textContent = theme === 'dark' ? 'Dunkel' : 'Hell';
    try { localStorage.setItem(THEME_KEY, theme); } catch (e) { /* ignore */ }
  }
  (function initTheme() {
    let saved = null;
    try { saved = localStorage.getItem(THEME_KEY); } catch (e) { /* ignore */ }
    applyTheme(saved === 'light' ? 'light' : 'dark');
  })();
  themeToggle.addEventListener('click', () => {
    const current = root.getAttribute('data-theme');
    applyTheme(current === 'dark' ? 'light' : 'dark');
  });

  // ---------- Werkzeug Check ----------
  async function checkTools() {
    if (!isTauri()) return;
    try {
      const missing = await window.__TAURI__.core.invoke('check_tools');
      if (Array.isArray(missing) && missing.length > 0) {
        $('toolWarning').style.display = 'block';
        $('toolWarningText').textContent =
          'Nicht gefunden: ' + missing.join(', ') +
          '. Bitte in der BAUANLEITUNG.md nachsehen, wie diese Werkzeuge bereitgestellt werden.';
      }
    } catch (e) {
      // check_tools selbst nicht verfügbar, z.B. während der Entwicklung im Browser
    }
  }

  // ---------- Dateiauswahl ----------
  let selectedPath = null;

  function formatBytes(bytes) {
    if (!bytes || bytes <= 0) return '';
    const units = ['B', 'KB', 'MB', 'GB'];
    let i = 0, val = bytes;
    while (val >= 1024 && i < units.length - 1) { val /= 1024; i++; }
    return val.toFixed(val >= 10 || i === 0 ? 0 : 1) + ' ' + units[i];
  }

  function setSelectedFile(path, sizeBytes) {
    selectedPath = path;
    const name = path.split(/[\\/]/).pop();
    $('sfName').textContent = name;
    $('sfSize').textContent = sizeBytes ? formatBytes(sizeBytes) : '';
    $('selectedFileBox').style.display = 'block';
    $('startBtn').disabled = false;
  }

  function clearSelectedFile() {
    selectedPath = null;
    $('selectedFileBox').style.display = 'none';
    $('startBtn').disabled = true;
  }

  async function pickFile() {
    if (!isTauri()) {
      alert('Dateiauswahl ist nur innerhalb der NovaPhonic Anwendung verfügbar, nicht im Browser.');
      return;
    }
    try {
      const result = await window.__TAURI__.core.invoke('pick_video_file');
      if (result && result.path) {
        setSelectedFile(result.path, result.sizeBytes);
      }
    } catch (e) {
      console.error(e);
    }
  }

  const dropzone = $('dropzone');
  dropzone.addEventListener('click', pickFile);
  dropzone.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); pickFile(); }
  });
  $('sfRemove').addEventListener('click', (e) => { e.stopPropagation(); clearSelectedFile(); });

  // Echtes Drag & Drop einer Datei aus dem Windows Explorer in das Fenster.
  // Liefert direkt einen Dateipfad, es wird nichts im Browser eingelesen.
  (function initDragDrop() {
    if (!isTauri() || !window.__TAURI__.webview) return;
    try {
      window.__TAURI__.webview.getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === 'over') {
          dropzone.classList.add('drag');
        } else if (event.payload.type === 'drop') {
          dropzone.classList.remove('drag');
          const paths = event.payload.paths || [];
          if (paths.length > 0) {
            const p = paths[0];
            const ext = p.split('.').pop().toLowerCase();
            if (['mp4', 'mov', 'mkv', 'avi', 'webm'].includes(ext)) {
              setSelectedFile(p, null);
            } else {
              alert('Bitte eine Videodatei ablegen (MP4, MOV, MKV, AVI oder WEBM).');
            }
          }
        } else {
          dropzone.classList.remove('drag');
        }
      });
    } catch (e) {
      // Drag & Drop API in dieser Tauri Version evtl. anders benannt, kein Problem,
      // Klicken zum Auswählen funktioniert trotzdem.
      console.warn('Drag & Drop nicht verfügbar:', e);
    }
  })();

  // ---------- Einstellungen ----------
  const marginSlider = $('marginSlider');
  const marginTag = $('marginTag');
  function updateMarginTag() {
    const val = (parseInt(marginSlider.value, 10) / 10).toFixed(1);
    marginTag.textContent = val.replace('.', ',') + ' s';
  }
  marginSlider.addEventListener('input', updateMarginTag);
  updateMarginTag();

  const loudnormToggle = $('loudnormToggle');
  const loudnormOptions = $('loudnormOptions');
  function syncLoudnormOptions() {
    loudnormOptions.style.display = loudnormToggle.checked ? 'block' : 'none';
  }
  loudnormToggle.addEventListener('change', syncLoudnormOptions);
  syncLoudnormOptions();

  // ---------- Pipeline Ablauf ----------
  const STEP_IDS = ['cut', 'denoise', 'loudnorm'];

  function resetSteps() {
    for (const id of STEP_IDS) {
      const el = $('step-' + id);
      el.classList.remove('running', 'done', 'error');
      $('step-' + id + '-detail').textContent = '';
    }
    $('logBox').textContent = '';
  }

  function appendLog(line) {
    const box = $('logBox');
    box.textContent += (box.textContent ? '\n' : '') + line;
    box.scrollTop = box.scrollHeight;
  }

  function setStepStatus(step, status, detail) {
    const el = $('step-' + step);
    if (!el) return;
    el.classList.remove('running', 'done', 'error');
    if (status) el.classList.add(status);
    if (detail !== undefined) $('step-' + step + '-detail').textContent = detail || '';
  }

  let unlistenProgress = null;

  async function initProgressListener() {
    if (!isTauri()) return;
    unlistenProgress = await window.__TAURI__.event.listen('pipeline-progress', (event) => {
      const p = event.payload || {};
      if (p.step) setStepStatus(p.step, p.status, p.detail);
      if (p.log) appendLog(p.log);
    });
  }

  const startBtn = $('startBtn');
  startBtn.addEventListener('click', async () => {
    if (!selectedPath || !isTauri()) return;
    startBtn.disabled = true;
    dropzone.style.pointerEvents = 'none';
    resetSteps();
    $('resultArea').innerHTML = '<div class="empty-hint">Verarbeitung läuft…</div>';

    const options = {
      inputPath: selectedPath,
      marginSeconds: parseInt(marginSlider.value, 10) / 10,
      denoise: $('denoiseToggle').checked,
      loudnorm: loudnormToggle.checked,
      loudnormTarget: parseInt($('loudnormTarget').value, 10)
    };

    try {
      const result = await window.__TAURI__.core.invoke('process_video', { options });
      showResult(result);
    } catch (e) {
      console.error(e);
      $('resultArea').innerHTML =
        '<div class="status-error">Verarbeitung fehlgeschlagen: ' +
        String(e).replace(/</g, '&lt;') + '</div>';
    } finally {
      startBtn.disabled = false;
      dropzone.style.pointerEvents = '';
    }
  });

  function showResult(result) {
    const name = result.outputPath.split(/[\\/]/).pop();
    $('resultArea').innerHTML =
      '<div class="result-box">' +
      '<span class="rb-name">' + name.replace(/</g, '&lt;') + '</span>' +
      '<button id="saveResultBtn" type="button">Speichern unter…</button>' +
      '</div>' +
      '<div class="status-ok" style="margin-top:8px;">Fertig. Liegt vorerst unter: ' +
      result.outputPath.replace(/</g, '&lt;') + '</div>';
    $('saveResultBtn').addEventListener('click', async () => {
      try {
        const saved = await window.__TAURI__.core.invoke('save_output_as', {
          sourcePath: result.outputPath,
          suggestedName: name
        });
        if (saved) appendLog('Gespeichert unter: ' + saved);
      } catch (e) {
        console.error(e);
      }
    });
  }

  // ---------- Updater (wie bei NovaImage) ----------
  const updateBanner = $('updateBanner');
  const updateBannerText = $('updateBannerText');
  let pendingUpdate = null;

  $('updateDismissBtn').addEventListener('click', () => {
    updateBanner.style.display = 'none';
  });
  $('updateInstallBtn').addEventListener('click', async () => {
    if (!pendingUpdate) return;
    updateBannerText.innerHTML = '<strong>Wird installiert…</strong>';
    try {
      await pendingUpdate.downloadAndInstall();
      await window.__TAURI__.process.relaunch();
    } catch (e) {
      console.error(e);
      updateBannerText.innerHTML = '<strong>Update fehlgeschlagen.</strong> Bitte später erneut versuchen oder den Installer manuell von GitHub laden.';
    }
  });

  async function checkForUpdate(silent) {
    if (!isTauri() || !window.__TAURI__.updater) return 'none';
    try {
      const update = await window.__TAURI__.updater.check();
      if (update) {
        pendingUpdate = update;
        updateBannerText.innerHTML =
          '<strong>Update verfügbar (' + update.version + ').</strong> Eine neue Version von NovaPhonic steht bereit.';
        updateBanner.style.display = 'flex';
        return 'available';
      }
      return 'none';
    } catch (e) {
      if (!silent) console.error(e);
      return 'error';
    }
  }

  const checkUpdateBtn = $('checkUpdateBtn');
  if (isTauri() && window.__TAURI__.updater) {
    checkUpdateBtn.style.display = 'inline-flex';
  }
  checkUpdateBtn.addEventListener('click', async () => {
    checkUpdateBtn.disabled = true;
    checkUpdateBtn.textContent = 'Prüfe…';
    const status = await checkForUpdate(false);
    if (status === 'available') {
      checkUpdateBtn.disabled = false;
      checkUpdateBtn.textContent = 'Nach Updates suchen';
    } else if (status === 'error') {
      checkUpdateBtn.textContent = 'Prüfung fehlgeschlagen';
      setTimeout(() => {
        checkUpdateBtn.disabled = false;
        checkUpdateBtn.textContent = 'Nach Updates suchen';
      }, 2500);
    } else {
      checkUpdateBtn.textContent = 'Bereits aktuell';
      setTimeout(() => {
        checkUpdateBtn.disabled = false;
        checkUpdateBtn.textContent = 'Nach Updates suchen';
      }, 2500);
    }
  });

  // ---------- Start ----------
  initProgressListener();
  checkTools();
  checkForUpdate(true);
})();
