; Ivy's speech model (lite: Qwen3-ASR-1.7B fine-tuned by the lab, ~2.4 GB) ships as two files NEXT TO
; the installer, not inside it: NSIS can't hold more than 2 GB and GitHub caps each release file at 2 GB.
; The release is setup.exe + ivy-lite-Q8_0.gguf + mmproj-ivy-lite-f16.gguf in one folder; this copies
; them into $INSTDIR\models\ivy-lite, where lite.rs loads them.
!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$INSTDIR\models\ivy-lite"
  IfFileExists "$EXEDIR\ivy-lite-Q8_0.gguf" 0 model_missing
  IfFileExists "$EXEDIR\mmproj-ivy-lite-f16.gguf" 0 model_missing
    ; Full destination paths (no trailing backslash) avoid the NSIS parser bug where \" inside a
    ; double-quoted string is treated as an escaped quote. Sizes are in KB, for the progress bar.
    CopyFiles /SILENT "$EXEDIR\ivy-lite-Q8_0.gguf" "$INSTDIR\models\ivy-lite\ivy-lite-Q8_0.gguf" 1791428
    CopyFiles /SILENT "$EXEDIR\mmproj-ivy-lite-f16.gguf" "$INSTDIR\models\ivy-lite\mmproj-ivy-lite-f16.gguf" 626733
    Goto model_done
  model_missing:
    IfSilent model_done
    MessageBox MB_OK|MB_ICONEXCLAMATION "Ivy's speech model files were not found next to this installer.$\r$\n$\r$\nPut ivy-lite-Q8_0.gguf and mmproj-ivy-lite-f16.gguf in the same folder as the setup file and run it again.$\r$\n$\r$\nIvy is installed, but it can't transcribe until those two files are in:$\r$\n$INSTDIR\models\ivy-lite"
  model_done:
  ; GPU or CPU is chosen in the app's setup wizard (and Settings), not here.
!macroend

; The model files were copied in by the hook above, so Tauri's uninstaller doesn't know about them.
!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\models\ivy-lite\ivy-lite-Q8_0.gguf"
  Delete "$INSTDIR\models\ivy-lite\mmproj-ivy-lite-f16.gguf"
  RMDir "$INSTDIR\models\ivy-lite"
  RMDir "$INSTDIR\models"
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
