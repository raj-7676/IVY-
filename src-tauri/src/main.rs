// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
  // WebView2 is Chromium-based and enforces the same autoplay policy as
  // any Chrome tab: AudioContext stays silent until a real user gesture
  // happens. The launch intro plays its sound the instant the window
  // opens, before any gesture is possible, so it was silently dead every
  // launch. Must be set before the webview is created.
  // The other switches stop WebView2's own background traffic to Microsoft (experiment configs,
  // component updates, connection telemetry): Ivy's window only ever shows local content.
  // Switches already set by whoever launched Ivy are kept after these (the UI test run adds
  // --remote-debugging-port this way).
  let extra = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
  std::env::set_var(
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    format!(
      "--autoplay-policy=no-user-gesture-required --disable-background-networking \
       --disable-component-update --disable-domain-reliability --disable-sync {extra}"
    ),
  );
  // Linux, before GTK starts (whatever the user set wins):
  // - On a Wayland desktop Ivy's windows are X11 windows (XWayland): Wayland lets no app place a window or keep it
  //   above others, which the capsule needs (src/linux.rs).
  // - WebKitGTK's DMA-BUF renderer leaves the window blank on some NVIDIA drivers.
  #[cfg(target_os = "linux")]
  {
    if std::env::var_os("GDK_BACKEND").is_none() && std::env::var_os("DISPLAY").is_some() {
      std::env::set_var("GDK_BACKEND", "x11");
    }
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
      std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
  }
  app_lib::run();
}
