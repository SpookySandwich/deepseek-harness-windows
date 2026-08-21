use std::{
    io::BufRead,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    utils::config::WindowEffectsConfig,
    window::Effect as WindowEffect,
    webview::WebviewWindowBuilder,
    Manager, RunEvent, WebviewUrl, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_notification::NotificationExt;

/// The shell is a GUI-subsystem app: without this flag, every console-subsystem
/// child it spawns (node.exe for the sidecar and the bridge) pops a console
/// window on startup.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const WINDOW_LABEL: &str = "main";

/// Restyles the dsh web content to the Windows 11 layering model: the sidebar
/// sits directly on the native Mica backdrop (seamless with the untinted,
/// DWM-drawn titlebar) and the content area keeps its original opaque fill,
/// framed as a WinUI-style "layer card" by a rounded top-left corner and a
/// thin surface stroke. Runs on every page load (boot screen and the dsh web
/// app).
///
/// The token overrides must target `body`, not `:root`: design-platform.css
/// defines the alias tokens on `body`, and an inherited custom property
/// resolves to the nearest ancestor that defines it — a `:root` override is
/// shadowed by body's own definition for every descendant.
///
/// Layering, bottom to top: Mica → body → .frame (spans the whole window) →
/// columns. Class names are CSS-module-hashed, so .frame is matched through
/// its stable `data-shell-overlay` child and the columns through their fixed
/// DOM order (1=sidebar, 2=center, 3=details, see AppFrame.tsx).
///
/// Key tokens (see packages/client/ui-theme/.../design-platform.css):
///   --dsw-alias-bg-base           content surface fill (untouched, stays opaque)
///   --dsw-specific-sidebar-fill   the left sidebar column (cleared here)
///   --dsw-alias-bg-layer-1/2/3    popups / settings / overlays (keep opaque)
const MICA_SCRIPT: &str = r#"
(function () {
  function inject() {
    if (document.getElementById('dsh-mica')) return;
    var el = document.createElement('style');
    el.id = 'dsh-mica';
    el.textContent = [
      'html, body { background: transparent !important; }',
      'body { --dsw-specific-sidebar-fill: transparent !important; }',
      'div:has(> [data-shell-overlay]) { background: transparent !important; }',
      'div:has(> [data-shell-overlay]) > div:nth-of-type(1) { border-right: none !important; }',
      'div:has(> [data-shell-overlay]) > div:nth-of-type(2) {',
      '  border-top-left-radius: 8px;',
      '  border-left: 1px solid rgba(0, 0, 0, 0.08);',
      '  border-top: 1px solid rgba(0, 0, 0, 0.08);',
      '}',
      'div:has(> [data-shell-overlay]) > div:nth-of-type(3) {',
      '  border-top: 1px solid rgba(0, 0, 0, 0.08);',
      '}',
      'body[data-ds-dark-theme] div:has(> [data-shell-overlay]) > div:nth-of-type(2) {',
      '  border-left-color: rgba(255, 255, 255, 0.09);',
      '  border-top-color: rgba(255, 255, 255, 0.09);',
      '}',
      'body[data-ds-dark-theme] div:has(> [data-shell-overlay]) > div:nth-of-type(3) {',
      '  border-top-color: rgba(255, 255, 255, 0.09);',
      '}',
      '/* dsh boot loading screen (AppRoot): #root direct child that is not the',
      '   .frame (no overlay descendant) — clear its bg-base fill so the loading',
      '   page floats on Mica like the shell boot page before it. */',
      '#root > div:not(:has([data-shell-overlay])) { background: transparent !important; }'
    ].join('\n');
    (document.head || document.documentElement).appendChild(el);
  }
  if (document.head) { inject(); } else { document.addEventListener('DOMContentLoaded', inject); }
})();
"#;

/// Owns the spawned sidecar and bridge processes so they can be killed on exit.
#[derive(Default)]
struct SidecarState(Mutex<Vec<Child>>);

/// A resolved sidecar runtime: node binary, dsh CLI entry, and its base dir.
struct Runtime {
    node: PathBuf,
    bin: PathBuf,
    dir: PathBuf,
}

/// First port tried for the sidecar. The web app is served from
/// `http://127.0.0.1:<port>`, and that string is the WebView's *origin* — which
/// is what partitions `localStorage`, `sessionStorage`, IndexedDB and cookies.
///
/// This used to bind `127.0.0.1:0` and take whatever ephemeral port Windows
/// handed out, so every launch produced a new origin and every plugin's stored
/// settings silently reset to defaults. (A user profile here had accumulated 25
/// orphaned localStorage partitions, one per launch.) Keeping the port stable
/// keeps the origin stable, so settings persist across restarts.
///
/// The range is in the IANA dynamic/private block and above the ephemeral range
/// Windows hands out by default (which starts at 49152), so a stable pick is
/// unlikely to be stolen by an unrelated process between runs.
const PREFERRED_PORT: u16 = 47794;
const PORT_SCAN: u16 = 32;

/// Bind a stable port when possible, so the WebView origin does not change
/// between launches. Scans a short deterministic range and only falls back to an
/// ephemeral port if every candidate is taken — in that case stored settings are
/// lost for that run, which is strictly better than failing to start.
fn find_free_port() -> u16 {
    for offset in 0..PORT_SCAN {
        let candidate = PREFERRED_PORT.saturating_add(offset);
        // Binding and immediately dropping leaves a window where another
        // process could take the port before the sidecar binds it. The sidecar
        // failing to bind is loud and recoverable, so prefer that over the
        // silent per-launch origin churn this replaces.
        if TcpListener::bind(("127.0.0.1", candidate)).is_ok() {
            return candidate;
        }
    }
    eprintln!(
        "[shell] ports {}-{} all busy; falling back to an ephemeral port (plugin settings will not persist this run)",
        PREFERRED_PORT,
        PREFERRED_PORT.saturating_add(PORT_SCAN - 1)
    );
    TcpListener::bind("127.0.0.1:0")
        .expect("failed to bind for port discovery")
        .local_addr()
        .expect("no local addr")
        .port()
}

fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(150));
    }
    false
}

/// The bundled plugin's package name. Must match the payload directory
/// prepare-runtime.ps1 writes under `plugins/`, which it takes from the
/// package's own manifest.
const PLUGIN_PACKAGE: &str = "dsh-plugin-smooth-stream";

/// The bundled plugin's registry spec, recorded in the profile's dependencies
/// so `dsh plugin update` can refresh it later.
const PLUGIN_SPEC: &str = "github:SpookySandwich/dsh-plugin-smooth-stream";

fn copy_dir(src: &PathBuf, dst: &PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir {dst:?}: {e}"))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("read {src:?}: {e}"))? {
        let entry = entry.map_err(|e| format!("read {src:?}: {e}"))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("copy {from:?}: {e}"))?;
        }
    }
    Ok(())
}

/// Pre-install the bundled dsh-smooth-stream plugin into the web profile when
/// the installer left its marker (the NSIS component, selected by default).
/// The profile change is what `dsh plugin --profile web add` would produce:
/// the package under node_modules plus manifest entries. Runs before the
/// sidecar spawns so the first boot already has the plugin. No-op without the
/// marker; the marker is removed afterwards so a later manual `plugin remove`
/// is not undone on every start.
fn install_bundled_plugin(app: &tauri::AppHandle, rt: &Runtime) -> Result<(), String> {
    let marker = rt.dir.join("plugins").join("install-smooth-stream");
    let payload = rt.dir.join("plugins").join(PLUGIN_PACKAGE);
    if !marker.exists() || !payload.exists() {
        return Ok(());
    }

    let home = std::env::var_os("DSH_HOME")
        .map(PathBuf::from)
        .or_else(|| app.path().home_dir().ok().map(|h| h.join(".dsh")))
        .ok_or("cannot resolve dsh home")?;
    let profile = home.join("profiles").join("web");

    // Package files land verbatim under the profile's node_modules.
    let dest = profile.join("node_modules").join(PLUGIN_PACKAGE);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| format!("clear {dest:?}: {e}"))?;
    }
    copy_dir(&payload, &dest)?;

    // Manifest: a fresh profile gets the stock web bundles plus ours; an
    // existing one gets the two entries merged in, everything else untouched.
    let manifest_path = profile.join("package.json");
    let mut manifest: serde_json::Value = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| format!("parse {manifest_path:?}: {e}"))?,
        Err(_) => serde_json::json!({
            "name": "dsh-profile-web",
            "private": true,
            "dsh": { "profile": { "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } }
        }),
    };
    // serde_json index assignment auto-vivifies null slots into objects.
    if manifest["dsh"]["profile"]["bundles"].as_array().is_none() {
        manifest["dsh"]["profile"]["bundles"] = serde_json::json!([]);
    }
    let bundles = manifest["dsh"]["profile"]["bundles"]
        .as_array_mut()
        .ok_or("profile bundles is not a list")?;
    if !bundles.iter().any(|b| b == PLUGIN_PACKAGE) {
        bundles.push(serde_json::json!(PLUGIN_PACKAGE));
    }
    if manifest["dependencies"].as_object().is_none() {
        manifest["dependencies"] = serde_json::json!({});
    }
    manifest["dependencies"]
        .as_object_mut()
        .ok_or("profile dependencies is not an object")?
        .entry(PLUGIN_PACKAGE)
        .or_insert_with(|| serde_json::json!(PLUGIN_SPEC));
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, text + "\n").map_err(|e| format!("write {manifest_path:?}: {e}"))?;

    let _ = std::fs::remove_file(&marker);
    Ok(())
}

/// Resolve the sidecar runtime. Development points at the sidecar directory via
/// DSH_RUNTIME_DIR; production resolves the bundled resource directory.
fn resolve_runtime(app: &tauri::AppHandle) -> Result<Runtime, String> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("DSH_RUNTIME_DIR") {
        bases.push(PathBuf::from(dir));
    }
    if let Ok(res) = app.path().resource_dir() {
        bases.push(res.join("runtime"));
        bases.push(res.join("resources").join("runtime"));
        bases.push(res);
    }

    for base in bases {
        let bin = base
            .join("node_modules")
            .join("@deepseek-ai")
            .join("dsh")
            .join("lib")
            .join("bin.js");
        if bin.exists() {
            return Ok(Runtime {
                node: base.join("node.exe"),
                bin,
                dir: base,
            });
        }
    }
    Err("dsh runtime not found; set DSH_RUNTIME_DIR to the sidecar directory".into())
}

/// Spawn the dsh web backend as a child process and drain its output.
fn spawn_sidecar(app: &tauri::AppHandle, port: u16) -> Result<Child, String> {
    let rt = resolve_runtime(app)?;
    let node = if rt.node.exists() { rt.node } else { PathBuf::from("node") };
    let bin = rt.bin.to_string_lossy().into_owned();
    let port = port.to_string();

    let mut cmd = Command::new(&node);
    cmd.arg(&bin)
        .arg("web")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(&port)
        .current_dir(&rt.dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn dsh sidecar ({node:?}): {e}"))?;

    if let Some(out) = child.stdout.take() {
        pump(out, "sidecar");
    }
    if let Some(err) = child.stderr.take() {
        pump(err, "sidecar");
    }
    Ok(child)
}

/// Read a child stream to completion, logging each line so the pipe never fills.
fn pump(stream: impl std::io::Read + Send + 'static, tag: &'static str) {
    thread::spawn(move || {
        let reader = std::io::BufReader::new(stream);
        for line in reader.lines() {
            if let Ok(line) = line {
                eprintln!("[{tag}] {line}");
            }
        }
    });
}

/// Spawn the notification bridge: a Node script that subscribes to the dsh
/// mux WebSocket and emits a "turn-end" JSON line when a task completes.
fn spawn_bridge(app: &tauri::AppHandle, port: u16) -> Result<Child, String> {
    let rt = resolve_runtime(app)?;
    let bridge = rt.dir.join("bridge.mjs");
    if !bridge.exists() {
        // Notifications are optional; a missing bridge is not fatal.
        eprintln!("[bridge] bridge.mjs not found; notifications disabled");
        return Err("bridge.mjs not found".into());
    }
    let node = if rt.node.exists() { rt.node } else { PathBuf::from("node") };
    let bridge = bridge.to_string_lossy().into_owned();

    let mut cmd = Command::new(&node);
    cmd.arg(&bridge)
        .env("DSH_PORT", port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to spawn notification bridge ({node:?}): {e}"))?;

    if let Some(err) = child.stderr.take() {
        pump(err, "bridge");
    }
    if let Some(out) = child.stdout.take() {
        let handle = app.clone();
        thread::spawn(move || {
            let reader = std::io::BufReader::new(out);
            for line in reader.lines() {
                if let Ok(line) = line {
                    if line.contains("turn-end") {
                        // Notify only when the user is elsewhere: a focused
                        // window already shows the finished turn. A missing
                        // window or a failed focus read defaults to notifying
                        // rather than dropping the completion signal.
                        let focused = handle
                            .get_webview_window(WINDOW_LABEL)
                            .and_then(|w| w.is_focused().ok())
                            .unwrap_or(false);
                        if !focused {
                            let _ = handle
                                .notification()
                                .builder()
                                .title("DeepSeek Harness")
                                .body("任务已完成")
                                .show();
                        }
                    }
                }
            }
        });
    }
    Ok(child)
}

fn build_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "显示 DeepSeek Harness", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(app, "autostart", "开机自启动", true, app.autolaunch().is_enabled().unwrap_or(false), None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &autostart, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("missing default window icon")?;

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("DeepSeek Harness")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window(WINDOW_LABEL) {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "autostart" => {
                let enabled = app.autolaunch().is_enabled().unwrap_or(false);
                let _ = if enabled {
                    app.autolaunch().disable()
                } else {
                    app.autolaunch().enable()
                };
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window(WINDOW_LABEL) {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window(WINDOW_LABEL) {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(SidecarState::default())
        .setup(|app| {
            WebviewWindowBuilder::new(app.handle(), WINDOW_LABEL, WebviewUrl::App("index.html".into()))
                .title("DeepSeek Harness")
                .inner_size(1280.0, 840.0)
                .min_inner_size(940.0, 620.0)
                .transparent(true)
                .initialization_script(MICA_SCRIPT)
                .effects(WindowEffectsConfig {
                    effects: vec![WindowEffect::Mica],
                    ..Default::default()
                })
                .build()?;

            // Enable auto-start on first run only, so a later tray toggle sticks.
            // The marker records a successful enable: a transient failure must
            // not suppress first-run auto-start for the app's lifetime.
            if let Ok(config_dir) = app.path().app_config_dir() {
                let marker = config_dir.join("autostart-initialized");
                if !marker.exists() && app.autolaunch().enable().is_ok() {
                    let _ = std::fs::create_dir_all(&config_dir);
                    let _ = std::fs::write(&marker, b"1");
                }
            }

            build_tray(app.handle())?;

            let port = find_free_port();
            let rt = resolve_runtime(app.handle())?;
            if let Err(e) = install_bundled_plugin(app.handle(), &rt) {
                eprintln!("[shell] bundled plugin install skipped: {e}");
            }
            let sidecar = spawn_sidecar(app.handle(), port)?;
            let bridge = spawn_bridge(app.handle(), port).ok();
            let mut children = vec![sidecar];
            if let Some(b) = bridge {
                children.push(b);
            }
            *app.state::<SidecarState>().0.lock().unwrap() = children;

            let handle = app.handle().clone();
            thread::spawn(move || {
                if wait_for_port(port, Duration::from_secs(90)) {
                    let url = format!("http://127.0.0.1:{port}");
                    if let Ok(parsed) = tauri::Url::parse(&url) {
                        if let Some(window) = handle.get_webview_window(WINDOW_LABEL) {
                            let _ = window.navigate(parsed);
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                } else {
                    eprintln!("[shell] dsh backend failed to start within timeout");
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == WINDOW_LABEL {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<SidecarState>() {
                let children = std::mem::take(&mut *state.0.lock().unwrap());
                for mut child in children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    });
}
