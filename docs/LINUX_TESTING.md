# Testing Ivy on Linux

For anyone trying the first Linux build of Ivy. You need a 64-bit PC (x86-64) with Ubuntu 22.04 or later, Linux
Mint 21 or later, Debian 12, or Fedora 37 or later. It takes about 30 minutes, most of it the one-time 2.4 GB
model download.

**No Linux PC?** A "Try Ubuntu" USB stick turns any PC into one for an afternoon, without installing anything on
its disk:

1. Download Ubuntu 24.04 Desktop from ubuntu.com and write it to a USB stick of 8 GB or more (balenaEtcher or
   Rufus).
2. Restart the PC, open its boot menu (usually F12, F10 or Esc right after switching on) and pick the USB stick.
3. Choose **Try Ubuntu** and connect to Wi-Fi.

Everything you do there is gone after a restart, so run the whole test in one go. The PC needs 8 GB of memory or
more, because the model download is kept in memory too.

## 1. Install

Open a terminal in the folder with the file:

- **Ubuntu, Mint, Debian:** `sudo apt install ./Ivy_0.2.8_amd64.deb`
- **Fedora:** `sudo dnf install ./Ivy-0.2.8-1.x86_64.rpm`

Then start **Ivy** from the apps menu. It downloads its speech model once, and the setup wizard opens by itself
when that's done.

## 2. Which kind of desktop is this?

Run `echo $XDG_SESSION_TYPE` in a terminal:

- **wayland** (Ubuntu's and Fedora's default): only **Ctrl + Shift** works as the dictation key, there is no
  Alt + V, and some apps can't be told apart (see below).
- **x11** (Linux Mint, or "Ubuntu on Xorg" chosen with the gear icon on the login screen): everything works as on
  Windows, with Alt + Space or Ctrl + Shift.

If you can, test both: log out, pick the other session with the gear icon on the login screen, and log in again.

## 3. Setup wizard

- **Step 1:** press **Ctrl + Shift**. The keys on screen should light up.
- **Step 3 (GPU or CPU):** if a yellow box says Ivy can't use the keyboard, click **Fix keyboard access** and enter
  your password. The box should go away within two seconds.
- **Step 5:** hold Ctrl + Shift, say the line, and let go.

## 4. What to try

Write ✓ or ✗ next to each one, with a note when something looks wrong.

1. In **Text Editor**: hold Ctrl + Shift, speak, let go. The text appears where the cursor is, and the small black
   bar at the top says "Pasted" (on X11, or for an older-style app on Wayland: "Pasted to" and the app's name).
2. The same in **Firefox** (a search box or an email), in a **terminal**, in **LibreOffice Writer**, and in VS Code,
   Slack or Discord if you have them.
3. Press Ctrl + Shift **twice quickly**, talk without holding, then press it **once** to finish.
4. Say: "I want to order French fries. No, no, I want a burger." Only the burger should be typed.
5. While the bar is showing, keep typing in your app. Your typing must stay in your app; the bar never takes the
   keyboard.
6. In Firefox press **Ctrl + Shift + T** (reopens a closed tab). It must still work and never start a recording.
7. Copy some text, dictate something, then press Ctrl + V. Your copied text comes back, not the dictation.
8. Click the desktop so nothing is selected and dictate. The bar says it couldn't paste. On X11, click into a text
   box and press **Alt + V**: the text appears. On Wayland, the text is in **History**.
9. After a paste, click **Typos? Touch Up** on the bar. A misspelled word is fixed in place, or it says "No typos
   found". On Wayland the button shows only after a paste into an older-style (X11) app; in apps made for
   Wayland, such as Text Editor, it doesn't show. Note which apps show it.
10. Start a recording and click the **X** on the bar. Nothing is typed, and your app keeps the keyboard.
11. The **Ivy icon** at the top right (Ubuntu shows it; plain GNOME on Fedora needs the "AppIndicator" extension):
    Dictate Now, Pause Ivy, Settings and Quit all work.
12. **Settings → Launch at Startup** on, then log out and back in. Ivy runs in the background (no window).
13. **Tone screen → Add app:** pick an app from the list, then dictate in that app. It uses that tone.
14. **Settings → Where Ivy runs:** which graphics card it names, or "No graphics card found".
15. **Speed:** roughly how long from letting go to the text appearing, for a 10-second dictation and for a
    one-minute one.

## 5. If something goes wrong

Send Yash a screenshot and Ivy's logs:

- `~/.local/share/app.ivy.dictation/debug.log` (in Files, press Ctrl + H to show hidden folders). It records
  timings and errors, never what you said. Also `~/.local/share/app.ivy.dictation/logs/Ivy.log`.
- Your desktop and graphics: the output of `echo $XDG_SESSION_TYPE $XDG_CURRENT_DESKTOP` and of
  `vulkaninfo --summary` (from the `vulkan-tools` package).
- Ivy started from a terminal (`ivy`) prints errors there.
- The keyboard helper: `ls -l /usr/libexec/ivy-keys` should start with `-rwxr-sr-x 1 root input`. If it doesn't,
  run `sudo /usr/libexec/ivy-keys-setup` and restart Ivy.

**To remove Ivy:** `sudo apt remove ivy` (or `sudo dnf remove ivy`). Your settings, history and the model stay in
`~/.local/share/app.ivy.dictation`; delete that folder to remove them too.
