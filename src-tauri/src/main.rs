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
  std::env::set_var(
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    "--autoplay-policy=no-user-gesture-required --disable-background-networking \
     --disable-component-update --disable-domain-reliability --disable-sync",
  );
  app_lib::run();
}
