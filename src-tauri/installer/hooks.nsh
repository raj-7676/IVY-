; Ivy's speech model (lite: Qwen3-ASR-1.7B fine-tuned by the lab, ~2.4 GB) is two files kept OUTSIDE the
; installer: NSIS can't hold more than 2 GB and GitHub caps each release file at 2 GB. Ivy downloads them itself
; on first start, with progress in its own window (src-tauri/src/model.rs). Setup only handles two cases:
;   1. already installed with the right size (an update): kept;
;   2. lying next to the setup file (an offline install, repack style): copied in, and kept only if it
;      matches the SHA-256 pinned here (else deleted, and Ivy downloads it).
; The model files never change between Ivy versions; a new model means new names, sizes and hashes here AND
; in model.rs (scripts/package-installer.mjs checks both against the real files).

; ID makes the labels unique per file. SIZE is in bytes, KB is for CopyFiles' progress bar.
!macro IvyModelFile ID NAME SIZE KB SHA
  StrCpy $R0 "$INSTDIR\models\ivy-lite\${NAME}"
  IfFileExists $R0 0 ivy_not_installed_${ID}
    FileOpen $R1 $R0 r
    FileSeek $R1 0 END $R2
    FileClose $R1
    StrCmp $R2 "${SIZE}" ivy_done_${ID}
  ivy_not_installed_${ID}:
  IfFileExists "$EXEDIR\${NAME}" 0 ivy_done_${ID}
    CopyFiles /SILENT "$EXEDIR\${NAME}" $R0 ${KB}
    DetailPrint "Checking ${NAME}..."
    nsExec::ExecToStack `powershell.exe -NoProfile -NonInteractive -Command "[Console]::Write((Get-FileHash -Algorithm SHA256 -LiteralPath '$R0').Hash)"`
    Pop $R1
    Pop $R2
    StrCmp $R2 "${SHA}" ivy_done_${ID}
    Delete $R0
  ivy_done_${ID}:
!macroend

; Ivy's speech engine (llama.cpp) needs Microsoft's Visual C++ runtime (MSVCP140.dll, VCOMP140.dll), at
; least the version of the compiler it was built with (VS 2022 toolset 14.44). Most PCs already have it; a
; fresh Windows doesn't, and Ivy then can't start. The official redistributable is bundled into setup and
; run only when it's missing or older. It needs admin rights, so Windows asks for permission then.
!define IVY_VC_MIN_MINOR 44
!macro IvyVcRuntime
  SetRegView 64
  ReadRegDWORD $R1 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Installed"
  ReadRegDWORD $R2 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" "Minor"
  SetRegView lastused
  StrCmp $R1 "1" 0 ivy_vc_install
  IntCmp $R2 ${IVY_VC_MIN_MINOR} ivy_vc_done ivy_vc_install ivy_vc_done
  ivy_vc_install:
    DetailPrint "Installing the Microsoft Visual C++ runtime..."
    InitPluginsDir
    ; Absolute path from scripts/package-installer.mjs (this file is compiled from Tauri's generated folder).
    File "/oname=$PLUGINSDIR\vc_redist.x64.exe" "$%IVY_VC_REDIST%"
    ; ShellExecute, not CreateProcess: the redistributable asks for admin rights (UAC).
    ExecShellWait "" "$PLUGINSDIR\vc_redist.x64.exe" "/install /quiet /norestart"
    Delete "$PLUGINSDIR\vc_redist.x64.exe"
  ivy_vc_done:
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro IvyVcRuntime
  ; Also where Ivy downloads the model to (lib.rs models_dir: the install folder's models\).
  CreateDirectory "$INSTDIR\models\ivy-lite"
  !insertmacro IvyModelFile 1 "mmproj-ivy-lite-f16.gguf" 641774016 626733 "07ED1CC9C96C19ABA84354B9135C69332747A98D50363A321BF1418AECECFC00"
  !insertmacro IvyModelFile 2 "ivy-lite-Q8_0.gguf" 1834422208 1791428 "DA50C4DCFC9BB36BAECA3A5DEDB742988DBE1058AB282DBB62D8A31BE27795ED"
  ; GPU or CPU is chosen in the app's setup wizard (and Settings), not here.
!macroend

; The model files were copied in by setup or downloaded by Ivy, so Tauri's uninstaller doesn't know about them.
; Also the pieces of an unfinished download (*.part, *.pieces), and 0.2.4's (*.parts folders).
!macro NSIS_HOOK_PREUNINSTALL
  ; An upgrade (installer.nsi runs this uninstaller with /UPDATE) keeps the model.
  StrCmp $UpdateMode 1 ivy_keep_model
  Delete "$INSTDIR\models\ivy-lite\ivy-lite-Q8_0.gguf"
  Delete "$INSTDIR\models\ivy-lite\mmproj-ivy-lite-f16.gguf"
  Delete "$INSTDIR\models\ivy-lite\*.part"
  Delete "$INSTDIR\models\ivy-lite\*.pieces"
  RMDir /r "$INSTDIR\models\ivy-lite\ivy-lite-Q8_0.gguf.parts"
  RMDir /r "$INSTDIR\models\ivy-lite\mmproj-ivy-lite-f16.gguf.parts"
  RMDir "$INSTDIR\models\ivy-lite"
  RMDir "$INSTDIR\models"
  ivy_keep_model:
!macroend

; Ivy's own 24h retention already limits how much voice data can exist at
; any given time, but settings.json, debug.log, and up to 24h of
; transcripts/audio still live in %LOCALAPPDATA% (with the app log and
; WebView2's cache), and 0.1.5 and older kept them in %APPDATA%. Tauri's
; generated uninstaller touches neither. A privacy-positioned voice app leaving any
; of that behind after uninstall is worth a real, explicit choice — skipped
; entirely for silent/unattended uninstalls, never deleted without asking.
!macro NSIS_HOOK_POSTUNINSTALL
  IfSilent finish_data_cleanup
  StrCmp $UpdateMode 1 finish_data_cleanup
  MessageBox MB_YESNO|MB_ICONQUESTION "Also delete your Ivy data?$\r$\n$\r$\nThis removes settings, dictation history, saved recordings and logs from $LOCALAPPDATA\app.ivy.dictation.$\r$\nThis cannot be undone." IDYES delete_data IDNO finish_data_cleanup
  delete_data:
    RMDir /r "$LOCALAPPDATA\app.ivy.dictation"
    RMDir /r "$APPDATA\app.ivy.dictation"
  finish_data_cleanup:
!macroend
