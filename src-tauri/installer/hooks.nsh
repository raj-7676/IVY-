; Qwen 2.5 3B's Q4_K_M GGUF (~1.96GB) companion file copy
!macro NSIS_HOOK_POSTINSTALL
  SetOutPath "$INSTDIR\models\qwen2.5-3b"
  IfFileExists "$EXEDIR\qwen2.5-3b-instruct-q4_k_m.gguf" copy_model skip_model
  copy_model:
    ; Specify the full destination path (no trailing backslash) to avoid the
    ; NSIS parser bug where \" inside a double-quoted string is treated as an
    ; escaped quote, preventing the string from terminating.
    CopyFiles /SILENT "$EXEDIR\qwen2.5-3b-instruct-q4_k_m.gguf" "$INSTDIR\models\qwen2.5-3b\qwen2.5-3b-instruct-q4_k_m.gguf" 2055600
  skip_model:

  ; Silent/unattended installs (winget, Chocolatey, MDM, or Tauri's own
  ; updater re-running this installer) must never block on a modal dialog.
  IfSilent finish_hw_gpu

  ; Hardware Acceleration Preference Selection (GPU vs CPU).
  ; Text is a single unbroken string — NSIS does not support line-
  ; continuation inside quoted MessageBox arguments.
  MessageBox MB_YESNO|MB_ICONQUESTION "Choose Hardware Acceleration for Ivy:$\r$\n$\r$\n[YES] GPU Accelerated (Recommended)$\r$\n  DirectML graphics acceleration (NVIDIA, AMD, Intel Arc).$\r$\n  Accuracy mode runs Qwen AI live on every dictation (adds a little time) for extra correction, plus on-demand Touch Up & Summarize.$\r$\n  Best all-round: emails, professional writing, and coding.$\r$\n$\r$\n[NO] CPU Mode (Universal Compatibility)$\r$\n  Zero VRAM usage. Works on any PC.$\r$\n  Live dictation stays instant (50+ rules, no AI); Touch Up & Summarize still available on-demand.$\r$\n  Best for coding and maximum battery life.$\r$\n$\r$\nClick YES for GPU, NO for CPU." IDYES finish_hw_gpu IDNO finish_hw_cpu

  finish_hw_gpu:
    CreateDirectory "$APPDATA\app.ivy.dictation"
    FileOpen $0 "$APPDATA\app.ivy.dictation\hardware_preference.txt" w
    FileWrite $0 "gpu"
    FileClose $0
    Goto finish_hw

  finish_hw_cpu:
    CreateDirectory "$APPDATA\app.ivy.dictation"
    FileOpen $0 "$APPDATA\app.ivy.dictation\hardware_preference.txt" w
    FileWrite $0 "cpu"
    FileClose $0

  finish_hw:
!macroend

; Uninstaller cleanup for companion model
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\models\qwen2.5-3b\qwen2.5-3b-instruct-q4_k_m.gguf"
  RMDir "$INSTDIR\models\qwen2.5-3b"
!macroend

; Ivy's own 24h retention already limits how much voice data can exist at
; any given time, but settings.json, debug.log, and up to 24h of
; transcripts/audio still live in %APPDATA% and Tauri's generated
; uninstaller never touches it. A privacy-positioned voice app leaving any
; of that behind after uninstall is worth a real, explicit choice — skipped
; entirely for silent/unattended uninstalls, never deleted without asking.
!macro NSIS_HOOK_POSTUNINSTALL
  IfSilent finish_data_cleanup
  MessageBox MB_YESNO|MB_ICONQUESTION "Also delete your Ivy data?$\r$\n$\r$\nThis removes settings, dictation history, and any saved recordings from $APPDATA\app.ivy.dictation.$\r$\nThis cannot be undone." IDYES delete_data IDNO finish_data_cleanup
  delete_data:
    RMDir /r "$APPDATA\app.ivy.dictation"
  finish_data_cleanup:
!macroend
