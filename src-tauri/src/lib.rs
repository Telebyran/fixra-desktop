//! Fixra för Mac och Windows.
//!
//! Skalet är tunt med avsikt: all affärslogik bor i app.fixra.se (och i
//! Postgres), så skrivbordsappen får varje ny funktion samma sekund som webben
//! publiceras. Det skalet tillför är det webbläsaren inte kan ge:
//! eget fönster och dockikon, menyrad med kortkommandon, ikon i menyfältet/
//! aktivitetsfältet, fixra://-länkar, hämtningar till Hämtade filer,
//! notiser, märke på dockikonen och automatiska uppdateringar.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{
    AboutMetadataBuilder, Menu, MenuBuilder, MenuEvent, MenuItemBuilder, PredefinedMenuItem,
    SubmenuBuilder,
};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::{DownloadEvent, NewWindowResponse};
use tauri::{AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow};
#[cfg(target_os = "macos")]
use tauri::{RunEvent, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

const APP_ORIGIN: &str = "https://app.fixra.se";

/// Adressen skalet öppnar. Bara utvecklingsbyggen får peka om den
/// (FIXRA_APP_ORIGIN=http://localhost:8080) — releasebygget är låst till app.fixra.se.
fn app_origin() -> String {
    #[cfg(debug_assertions)]
    if let Ok(o) = std::env::var("FIXRA_APP_ORIGIN") {
        return o.trim_end_matches('/').to_string();
    }
    APP_ORIGIN.to_string()
}
const MAIN: &str = "main";

/// Adresser som får öppnas inne i appen. Allt annat (fixra.se, hjälpsidor,
/// kundens egna länkar, Google Maps …) öppnas i datorns vanliga webbläsare —
/// annars fastnar användaren på en sida utan väg tillbaka.
const IN_APP_HOSTS: &[&str] = &[
    "app.fixra.se",
    "fixra.lovable.app",
    // Inloggning: Lovable/Supabase-auth och Microsoft Entra.
    "oauth.lovable.app",
    "login.microsoftonline.com",
    "login.microsoft.com",
    "login.live.com",
    "account.live.com",
    "account.microsoft.com",
];
const IN_APP_HOST_SUFFIXES: &[&str] = &[".supabase.co", ".fixra.se"];

/// Menyval → sida i Fixra. Samma sökvägar som huvudmenyn i app-shell.tsx.
const GO_TO: &[(&str, &str, &str, Option<&str>)] = &[
    ("go_idag", "Idag", "/idag", Some("CmdOrCtrl+1")),
    ("go_schema", "Schema", "/planering", Some("CmdOrCtrl+2")),
    ("go_kunder", "Kunder", "/kunder", Some("CmdOrCtrl+3")),
    ("go_offerter", "Offerter", "/offerter", Some("CmdOrCtrl+4")),
    ("go_attest", "Attest", "/attest", Some("CmdOrCtrl+5")),
    ("go_ekonomi", "Ekonomi", "/ekonomi", Some("CmdOrCtrl+6")),
    ("go_mina_pass", "Mina pass", "/mina-pass", Some("CmdOrCtrl+7")),
    ("go_installningar", "Inställningar", "/installningar", Some("CmdOrCtrl+,")),
];

#[derive(Default)]
struct Zoom(Mutex<f64>);

// ---------------------------------------------------------------- adresser

fn is_in_app(url: &Url) -> bool {
    match url.scheme() {
        // Skalets egen startsida (tauri://localhost på Mac, http(s)://tauri.localhost på Windows).
        "tauri" | "asset" | "about" | "data" | "blob" => return true,
        "http" | "https" => {}
        _ => return false,
    }
    let Some(host) = url.host_str() else { return false };
    if host == "tauri.localhost" || host == "localhost" {
        return true;
    }
    IN_APP_HOSTS.contains(&host) || IN_APP_HOST_SUFFIXES.iter().any(|s| host.ends_with(s))
}

/// `fixra://kunder/123?flik=avtal` → `/kunder/123?flik=avtal`.
/// `fixra://auth/callback#access_token=…` → `/auth/callback#access_token=…`
/// (så att inloggningslänkar från mejlet landar i appen och inte i webbläsaren).
fn deep_link_to_path(url: &Url) -> Option<String> {
    if url.scheme() != "fixra" {
        return None;
    }
    let mut path = String::from("/");
    if let Some(host) = url.host_str() {
        path.push_str(host);
    }
    let rest = url.path().trim_start_matches('/');
    if !rest.is_empty() {
        if path.len() > 1 {
            path.push('/');
        }
        path.push_str(rest);
    }
    // Bara vanliga sökvägstecken — en länk får aldrig kunna bli javascript: eller en annan värd.
    if path.contains("//") || path.contains('\\') || path.contains("..") {
        return None;
    }
    if let Some(q) = url.query() {
        path.push('?');
        path.push_str(q);
    }
    if let Some(f) = url.fragment() {
        path.push('#');
        path.push_str(f);
    }
    Some(path)
}

// ---------------------------------------------------------------- fönstret

fn main_window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(MAIN)
}

fn show<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = main_window(app) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Går till en sida i Fixra. Har webbappen registrerat
/// `window.fixraDesktop.onNavigate` byter vi sida utan omladdning; annars
/// laddas adressen om — långsammare men alltid rätt.
fn go<R: Runtime>(app: &AppHandle<R>, path: &str) {
    show(app);
    let Some(w) = main_window(app) else { return };
    let origin = app_origin();
    let Ok(target) = Url::parse(&format!("{origin}{path}")) else { return };
    let on_app = w
        .url()
        .map(|u| u.origin() == target.origin())
        .unwrap_or(false);
    // Fragment (t.ex. inloggningstoken) kräver alltid riktig laddning.
    if on_app && !path.contains('#') {
        let p = serde_json::to_string(path).unwrap_or_default();
        let full = serde_json::to_string(target.as_str()).unwrap_or_default();
        let _ = w.eval(format!(
            "(function(){{var d=window.fixraDesktop;if(d&&typeof d.onNavigate==='function'){{d.onNavigate({p});}}else{{window.location.assign({full});}}}})();"
        ));
    } else {
        let _ = w.navigate(target);
    }
}

fn set_zoom<R: Runtime>(app: &AppHandle<R>, f: impl FnOnce(f64) -> f64) {
    let state = app.state::<Zoom>();
    let mut z = state.0.lock().unwrap();
    *z = f(*z).clamp(0.5, 2.0);
    if let Some(w) = main_window(app) {
        let _ = w.set_zoom(*z);
    }
}

// ---------------------------------------------------------------- menyer

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let version = app.package_info().version.to_string();

    let about = AboutMetadataBuilder::new()
        .name(Some("Fixra"))
        .version(Some(version))
        .copyright(Some("© 2026 Techgruppen i Norden AB"))
        .website(Some("https://fixra.se"))
        .website_label(Some("fixra.se"))
        .build();

    let check_updates = MenuItemBuilder::with_id("check_updates", "Sök efter uppdateringar…").build(app)?;

    #[cfg(target_os = "macos")]
    let app_menu = SubmenuBuilder::new(app, "Fixra")
        .item(&PredefinedMenuItem::about(app, Some("Om Fixra"), Some(about.clone()))?)
        .item(&check_updates)
        .separator()
        .item(&PredefinedMenuItem::services(app, Some("Tjänster"))?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, Some("Göm Fixra"))?)
        .item(&PredefinedMenuItem::hide_others(app, Some("Göm övriga"))?)
        .item(&PredefinedMenuItem::show_all(app, Some("Visa alla"))?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("Avsluta Fixra"))?)
        .build()?;

    let file_menu = {
        let b = SubmenuBuilder::new(app, "Arkiv")
            .item(&MenuItemBuilder::with_id("open_browser", "Öppna sidan i webbläsaren").accelerator("CmdOrCtrl+Shift+O").build(app)?)
            .item(&MenuItemBuilder::with_id("copy_link", "Kopiera länk till sidan").accelerator("CmdOrCtrl+Shift+C").build(app)?)
            .separator()
            .item(&PredefinedMenuItem::close_window(app, Some("Stäng fönster"))?);
        #[cfg(not(target_os = "macos"))]
        let b = b
            .separator()
            .item(&check_updates)
            .item(&PredefinedMenuItem::quit(app, Some("Avsluta"))?);
        b.build()?
    };

    let edit_menu = SubmenuBuilder::new(app, "Redigera")
        .item(&PredefinedMenuItem::undo(app, Some("Ångra"))?)
        .item(&PredefinedMenuItem::redo(app, Some("Gör om"))?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some("Klipp ut"))?)
        .item(&PredefinedMenuItem::copy(app, Some("Kopiera"))?)
        .item(&PredefinedMenuItem::paste(app, Some("Klistra in"))?)
        .item(&PredefinedMenuItem::select_all(app, Some("Markera allt"))?)
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "Visa")
        .item(&MenuItemBuilder::with_id("reload", "Ladda om").accelerator("CmdOrCtrl+R").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("zoom_reset", "Verklig storlek").accelerator("CmdOrCtrl+0").build(app)?)
        .item(&MenuItemBuilder::with_id("zoom_in", "Zooma in").accelerator("CmdOrCtrl+Plus").build(app)?)
        .item(&MenuItemBuilder::with_id("zoom_out", "Zooma ut").accelerator("CmdOrCtrl+-").build(app)?)
        .separator()
        .item(&PredefinedMenuItem::fullscreen(app, Some("Helskärm"))?)
        .build()?;

    let mut go_menu = SubmenuBuilder::new(app, "Gå")
        .item(&MenuItemBuilder::with_id("back", "Bakåt").accelerator("CmdOrCtrl+[").build(app)?)
        .item(&MenuItemBuilder::with_id("forward", "Framåt").accelerator("CmdOrCtrl+]").build(app)?)
        .separator();
    for (id, label, _, accel) in GO_TO {
        let mut item = MenuItemBuilder::with_id(*id, *label);
        if let Some(a) = accel {
            item = item.accelerator(*a);
        }
        go_menu = go_menu.item(&item.build(app)?);
    }
    let go_menu = go_menu.build()?;

    let window_menu = SubmenuBuilder::new(app, "Fönster")
        .item(&PredefinedMenuItem::minimize(app, Some("Minimera"))?)
        .item(&PredefinedMenuItem::maximize(app, Some("Zooma"))?)
        .build()?;

    let help_menu = {
        let b = SubmenuBuilder::new(app, "Hjälp")
            .item(&MenuItemBuilder::with_id("help_site", "Fixra.se").build(app)?)
            .item(&MenuItemBuilder::with_id("help_support", "Kontakta support").build(app)?);
        #[cfg(not(target_os = "macos"))]
        let b = b.separator().item(&PredefinedMenuItem::about(app, Some("Om Fixra"), Some(about))?);
        b.build()?
    };

    let menu = MenuBuilder::new(app);
    #[cfg(target_os = "macos")]
    let menu = menu.item(&app_menu);
    menu.item(&file_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&go_menu)
        .item(&window_menu)
        .item(&help_menu)
        .build()
}

fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id("show", "Öppna Fixra").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("go_idag", "Idag").build(app)?)
        .item(&MenuItemBuilder::with_id("go_schema", "Schema").build(app)?)
        .item(&MenuItemBuilder::with_id("go_mina_pass", "Mina pass").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("check_updates", "Sök efter uppdateringar…").build(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Avsluta Fixra").build(app)?)
        .build()?;

    // Mac: svart mall-ikon som följer menyfältets ljus/mörker. Windows: färgikonen.
    #[cfg(target_os = "macos")]
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    #[cfg(not(target_os = "macos"))]
    let icon = Image::from_bytes(include_bytes!("../icons/32x32.png"))?;

    TrayIconBuilder::with_id("fixra-tray")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Fixra")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let id = event.id().as_ref();
    if let Some((_, _, path, _)) = GO_TO.iter().find(|(gid, ..)| *gid == id) {
        go(app, path);
        return;
    }
    let w = main_window(app);
    match id {
        "show" => show(app),
        "quit" => app.exit(0),
        "reload" => {
            if let Some(w) = w {
                let _ = w.eval("window.location.reload()");
            }
        }
        "back" => {
            if let Some(w) = w {
                let _ = w.eval("window.history.back()");
            }
        }
        "forward" => {
            if let Some(w) = w {
                let _ = w.eval("window.history.forward()");
            }
        }
        "zoom_in" => set_zoom(app, |z| z + 0.1),
        "zoom_out" => set_zoom(app, |z| z - 0.1),
        "zoom_reset" => set_zoom(app, |_| 1.0),
        "open_browser" => {
            if let Some(url) = w.and_then(|w| w.url().ok()) {
                if url.scheme().starts_with("http") {
                    let _ = app.opener().open_url(url.as_str(), None::<&str>);
                }
            }
        }
        "copy_link" => {
            if let Some(w) = w {
                let _ = w.eval("navigator.clipboard&&navigator.clipboard.writeText(window.location.href)");
            }
        }
        "help_site" => {
            let _ = app.opener().open_url("https://fixra.se", None::<&str>);
        }
        "help_support" => {
            let _ = app.opener().open_url("mailto:support@fixra.se", None::<&str>);
        }
        "check_updates" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { check_for_updates(app, true).await });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- hämtningar

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("fil");
    let ext = p.extension().and_then(|s| s.to_str());
    for i in 1..1000 {
        let n = match ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let c = dir.join(n);
        if !c.exists() {
            return c;
        }
    }
    candidate
}

// ---------------------------------------------------------------- uppdateringar

async fn check_for_updates<R: Runtime>(app: AppHandle<R>, manual: bool) {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(_) => return,
    };
    match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            app.dialog()
                .message(format!(
                    "Fixra {version} finns. Vill du installera nu? Appen startar om — inget du har sparat försvinner."
                ))
                .title("Ny version av Fixra")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Installera och starta om".into(),
                    "Senare".into(),
                ))
                .show(move |ok| {
                    let _ = tx.send(ok);
                });
            let ok = tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or(false))
                .await
                .unwrap_or(false);
            if !ok {
                return;
            }
            match update.download_and_install(|_, _| {}, || {}).await {
                Ok(()) => app.restart(),
                Err(e) => {
                    app.dialog()
                        .message(format!("Uppdateringen kunde inte installeras: {e}"))
                        .title("Fixra")
                        .kind(MessageDialogKind::Error)
                        .show(|_| {});
                }
            }
        }
        Ok(None) => {
            if manual {
                let v = app.package_info().version.to_string();
                app.dialog()
                    .message(format!("Du har senaste versionen ({v})."))
                    .title("Fixra")
                    .show(|_| {});
            }
        }
        Err(e) => {
            if manual {
                app.dialog()
                    .message(format!("Kunde inte söka efter uppdateringar just nu.\n\n{e}"))
                    .title("Fixra")
                    .kind(MessageDialogKind::Warning)
                    .show(|_| {});
            }
        }
    }
}

// ---------------------------------------------------------------- kommandon från webbappen

/// Märke på dockikonen/aktivitetsfältet, t.ex. antal nya förfrågningar.
/// Webbappen anropar: `window.__TAURI__.core.invoke('set_badge', { count })`.
#[tauri::command]
fn set_badge(window: WebviewWindow, count: Option<i64>) {
    let c = count.filter(|n| *n > 0);
    let _ = window.set_badge_count(c);
}

// ---------------------------------------------------------------- start

fn init_script(start_path: &str) -> String {
    let platform = if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    let start = serde_json::to_string(start_path).unwrap_or_else(|_| "\"/\"".into());
    format!(
        r#"(function(){{
  if (window.fixraDesktop) return;
  window.fixraDesktop = {{ platform: "{platform}", version: "{version}", origin: {origin}, startPath: {start}, onNavigate: null }};
  var mark = function(){{ document.documentElement.classList.add("fixra-desktop", "fixra-desktop-{platform}"); }};
  if (document.documentElement) mark(); else document.addEventListener("DOMContentLoaded", mark);
}})();"#,
        version = env!("CARGO_PKG_VERSION"),
        origin = serde_json::to_string(&app_origin()).unwrap_or_default(),
    )
}

pub fn run() {
    let mut builder = tauri::Builder::default();

    // Single instance måste registreras först: ett andra klick på ikonen, eller en
    // fixra://-länk medan appen redan körs, ska landa i det öppna fönstret.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            show(app);
            if let Some(path) = argv
                .iter()
                .filter_map(|a| Url::parse(a).ok())
                .find_map(|u| deep_link_to_path(&u))
            {
                go(app, &path);
            }
        }));
    }

    let app = builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Zoom(Mutex::new(1.0)))
        .invoke_handler(tauri::generate_handler![set_badge])
        .menu(build_menu)
        .on_menu_event(on_menu)
        .setup(|app| {
            let handle = app.handle().clone();

            // Registrera fixra:// i Windows-registret även vid körning utan installation (utveckling).
            #[cfg(any(windows, target_os = "linux"))]
            {
                let _ = app.deep_link().register_all();
            }

            // Kallstart via länk: fixra://… öppnar direkt rätt sida.
            let start_path = app
                .deep_link()
                .get_current()
                .ok()
                .flatten()
                .and_then(|urls| urls.iter().find_map(deep_link_to_path))
                .unwrap_or_else(|| "/".to_string());

            let nav_handle = handle.clone();
            let popup_handle = handle.clone();
            let dl_handle = handle.clone();

            let mut win = tauri::WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
                .title("Fixra")
                .inner_size(1360.0, 860.0)
                .min_inner_size(960.0, 620.0)
                .center()
                .visible(true)
                .initialization_script(init_script(&start_path))
                .on_navigation(move |url| {
                    if is_in_app(url) {
                        return true;
                    }
                    // mailto:, tel:, Google Maps, fixra.se … till systemet.
                    let _ = nav_handle.opener().open_url(url.as_str(), None::<&str>);
                    false
                })
                .on_new_window(move |url, _features| {
                    // Förhandsvisad PDF (blob:) visas i appen; allt annat i webbläsaren.
                    if matches!(url.scheme(), "blob" | "data") {
                        return NewWindowResponse::Allow;
                    }
                    if is_in_app(&url) {
                        // target="_blank" till en egen sida: öppna i samma fönster.
                        if let Some(w) = main_window(&popup_handle) {
                            let _ = w.navigate(url);
                        }
                    } else {
                        let _ = popup_handle.opener().open_url(url.as_str(), None::<&str>);
                    }
                    NewWindowResponse::Deny
                })
                .on_download(move |_webview, event| {
                    match event {
                        DownloadEvent::Requested { destination, .. } => {
                            let name = destination
                                .file_name()
                                .and_then(|n| n.to_str())
                                .filter(|n| !n.is_empty())
                                .unwrap_or("fixra-hamtning")
                                .to_string();
                            // Hämtade filer; saknas den (ovanligt) används ~/Downloads.
                            let dir = dl_handle
                                .path()
                                .download_dir()
                                .or_else(|_| dl_handle.path().home_dir().map(|h| h.join("Downloads")));
                            if let Ok(dir) = dir {
                                let _ = std::fs::create_dir_all(&dir);
                                *destination = unique_path(&dir, &name);
                            }
                        }
                        DownloadEvent::Finished { path, success, .. } => {
                            if success {
                                if let Some(p) = path {
                                    let name = p
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("Filen")
                                        .to_string();
                                    let _ = dl_handle
                                        .notification()
                                        .builder()
                                        .title("Hämtad till Hämtade filer")
                                        .body(name)
                                        .show();
                                    let _ = dl_handle.opener().reveal_item_in_dir(&p);
                                }
                            }
                        }
                        _ => {}
                    }
                    true
                });

            // Fixras skogsgröna bakgrund i stället för ett vitt blixtljus vid start.
            win = win.background_color(tauri::window::Color(10, 41, 28, 255));
            let window = win.build()?;

            // Mac: röda knappen gömmer fönstret (appen ligger kvar i dockan och
            // menyfältet, precis som Mail och Slack). Windows: stänger som vanligt.
            #[cfg(target_os = "macos")]
            {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }
            #[cfg(not(target_os = "macos"))]
            let _ = &window;

            build_tray(&handle)?;

            // Länkar som kommer medan appen körs (Mac skickar dem hit, inte via argv).
            let link_handle = handle.clone();
            app.deep_link().on_open_url(move |event| {
                if let Some(path) = event.urls().iter().find_map(deep_link_to_path) {
                    go(&link_handle, &path);
                }
            });

            // Uppdateringar: vid start (efter 20 s) och sedan var sjätte timme.
            let upd = handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio_sleep(Duration::from_secs(20)).await;
                loop {
                    check_for_updates(upd.clone(), false).await;
                    tokio_sleep(Duration::from_secs(6 * 60 * 60)).await;
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Fixra kunde inte starta");

    app.run(|app, event| {
        match event {
            // Mac: klick på dockikonen när fönstret är gömt.
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => show(app),
            _ => {
                let _ = app;
            }
        }
    });
}

async fn tokio_sleep(d: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Option<String> {
        deep_link_to_path(&Url::parse(s).unwrap())
    }

    #[test]
    fn djuplankar() {
        assert_eq!(p("fixra://kunder/123").as_deref(), Some("/kunder/123"));
        assert_eq!(p("fixra://idag").as_deref(), Some("/idag"));
        assert_eq!(p("fixra://planering?vecka=39").as_deref(), Some("/planering?vecka=39"));
        assert_eq!(
            p("fixra://auth/callback#access_token=abc&type=magiclink").as_deref(),
            Some("/auth/callback#access_token=abc&type=magiclink")
        );
        assert_eq!(p("https://evil.example/x"), None);
        assert_eq!(p("fixra://kunder/..%2F.."), None);
    }

    #[test]
    fn vad_som_oppnas_i_appen() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(is_in_app(&u("https://app.fixra.se/idag")));
        assert!(is_in_app(&u("https://login.microsoftonline.com/common/oauth2")));
        assert!(is_in_app(&u("https://wqptufxlagbvowxtgcbq.supabase.co/auth/v1/verify")));
        assert!(is_in_app(&u("tauri://localhost/index.html")));
        assert!(is_in_app(&u("http://tauri.localhost/index.html")));
        assert!(!is_in_app(&u("https://www.google.com/maps/dir/?api=1")));
        assert!(!is_in_app(&u("https://evil-fixra.se.example.com/")));
        assert!(!is_in_app(&u("mailto:kund@example.se")));
        assert!(!is_in_app(&u("https://notfixra.se/")));
    }
}
