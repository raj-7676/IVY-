# Testing Ivy on a Mac

For anyone trying the first Mac build of Ivy. You need a Mac with Apple silicon (M1 or newer) on macOS 13 or
later. It takes about 30 minutes, most of it the one-time 2.4 GB model download.

## 1. Install

1. Open `Ivy_0.2.8_aarch64.dmg` and drag **Ivy** onto **Applications**. Always start Ivy from Applications, not from
   the disk image or Downloads: macOS runs apps from those places in a temporary copy, and "launch at login" would
   point at a copy that's gone after a restart.
2. Open Ivy from Applications. macOS says it can't check Ivy for malware, because Ivy isn't signed with a paid
   Apple certificate. Allow it once:
   - **macOS 15 (Sequoia) or newer:** click **Done**. Open **System Settings → Privacy & Security**, scroll down to
     "Ivy was blocked…", click **Open Anyway**, then **Open Anyway** again and enter your password.
   - **macOS 14 or older:** right-click Ivy in Applications, choose **Open**, then **Open** again.
   - Or in Terminal: `xattr -dr com.apple.quarantine /Applications/Ivy.app`, then open Ivy normally.
3. Ivy downloads its speech model (2.4 GB) once. The setup wizard opens by itself when that's done.

## 2. Setup wizard

- **Step 1:** press **⌥ Option + Space**. The keys on screen should light up.
- **Step 3 (Allow Ivy):**
  - Click **Open Accessibility settings** and switch **Ivy** on in the list. macOS may also show its own
    "Ivy would like to control this computer" box; choose **Open System Settings**.
  - Click **Allow microphone** and choose **Allow**.
  - Both should turn green ("Allowed") within two seconds.
- **Step 5:** hold ⌥ Option + Space, say the line, and let go.

## 3. What to try

Write ✓ or ✗ next to each one, with a note when something looks wrong.

1. In **Notes** or **TextEdit**: hold ⌥ Option + Space, speak, let go. The text appears where the cursor is, and
   the small black bar at the top says "Pasted to Notes".
2. The same in a browser text box (Safari or Chrome), in a chat app (WhatsApp, Slack, Discord), and in VS Code if
   you have them.
3. Press ⌥ Option + Space **twice quickly**, talk without holding, then press it **once** to finish.
4. Say: "I want to order French fries. No, no, I want a burger." Only the burger should be typed.
5. While the bar is showing, keep typing in your app. Your typing must stay in your app; the bar never takes
   the keyboard.
6. Put Safari in **full screen** and dictate. The bar should show over it.
7. Click the desktop so nothing is selected and dictate. The bar should say it couldn't paste and to press
   **⌥ Option + V**. Click into a text box and press ⌥ Option + V: the text appears.
8. After a paste, click **Typos? Touch Up** on the bar. A misspelled word is fixed in place, or it says
   "No typos found".
9. Start a recording and click the **X** on the bar. Nothing is typed, and your app keeps the keyboard.
10. **Settings → Dictation shortcut → ⌃ Control + ⇧ Shift.** Hold both keys and speak. Shortcuts like
    Control + Shift + T in other apps must still work and never start a recording.
11. Copy some text, dictate something, then press ⌘V. Your copied text comes back, not the dictation.
12. The **Ivy icon in the menu bar** (top right): Dictate Now, Pause Ivy, Settings and Quit all work.
13. Close the window with the red button. Ivy keeps running. Click Ivy in the Dock and the window comes back.
14. **Tone screen → Add app:** pick an app from the list, then dictate in that app. It uses that tone.
15. Restart the Mac. Ivy starts by itself in the background (menu bar icon, no window), and after about
    90 seconds the bar says "Ivy is running in the background".
16. **Speed:** roughly how long from letting go to the text appearing, for a 10-second dictation and for a
    one-minute one.

## 4. If something goes wrong

Send Yash a screenshot and Ivy's log:

- Finder → **Go → Go to Folder…** → paste `~/Library/Application Support/app.ivy.dictation/` → `debug.log`.
  It records timings and errors, never what you said.
- Your macOS version and chip: Apple menu → **About This Mac**.

**After installing a newer Ivy:** macOS treats it as a new app, so Accessibility may show Ivy as switched on but
not work. Switch Ivy **off and on again** in System Settings → Privacy & Security → Accessibility.
