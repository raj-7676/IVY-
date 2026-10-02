; Outer single-file distributable wrapper.
;
; Why this exists: the real Ivy installer (Whisper embedded) plus Qwen 2.5
; 3B's ~1.96GB GGUF exceed NSIS's own 2GB compiled-data-block cap (see
; installer/hooks.nsh), so one downloadable file needs this trick.
;
; This wrapper sidesteps the cap instead of fighting it: it embeds the real
; installer the normal way via `File`, and the
; GGUF is appended as raw bytes onto the *end* of this compiled exe by the
; build script (scripts/package-installer.mjs), followed by a 16-byte
; footer: 8 bytes magic + 8 bytes GGUF length (both little-endian int64).
; NSIS's 2GB ceiling only applies to its own internal data blocks, not to
; the total size of the PE file on disk, so appended tail data is invisible
; to that limit — the same "companion file, not embedded" trick as
; hooks.nsh, just glued onto one file instead of shipped as two.
;
; At runtime: extract the embedded real installer to $PLUGINSDIR, read the
; footer from our own exe to find the GGUF's offset and length, stream-copy
; that byte range out to $PLUGINSDIR next to it (chunked, with progress),
; then launch the real installer exactly as if the user had downloaded both
; files themselves — same GPU/CPU prompt, same everything. $PLUGINSDIR is
; deleted automatically by NSIS once this process exits.
;
; Every System::Call below writes its result into $0/$1/$2 (the r0/r1/r2
; shorthand IS $0/$1/$2 per the System plugin docs) and the very next line
; checks it with plain IntCmp — never comparing across an intervening call,
; so no result gets clobbered before it's read. IntCmp is used in its
; 3-argument form throughout: "IntCmp $x 0 fail_label" jumps only when the
; Win32 call returned FALSE/0 and simply falls through to the next line on
; success — no dummy placeholder labels.

Unicode true
Name "Ivy Setup"
OutFile "wrapper-unstamped.exe"
RequestExecutionLevel user
SilentInstall silent
ShowInstDetails show
Icon "..\icons\icon.ico"

!define FOOTER_MAGIC 0x4956594658585901  ; fixed 8-byte tag, MSB clear (fits signed int64)
!define FOOTER_SIZE 16                    ; 8 bytes magic + 8 bytes gguf length
!define GENERIC_READ 0x80000000
!define GENERIC_WRITE 0x40000000
!define FILE_SHARE_READ 1
!define OPEN_EXISTING 3
!define CREATE_ALWAYS 2
!define FILE_BEGIN 0
!define CHUNK_SIZE 8388608                ; 8MB copy chunks, always fits in 32 bits

Var SelfPath
Var SrcHandle
Var DstHandle
Var Buf
Var SelfSize
Var GgufLen
Var GgufOff
Var Copied
Var Remaining
Var ThisChunk
Var LastPct
Var Pct
Var InnerPath
Var GgufPath
Var Flag
Var BytesDone

Section "Main"
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"

  ; The real installer is embedded normally — small enough for NSIS's own
  ; data blocks, no trick needed for this half. INSTALLER_EXE is passed in
  ; via `makensis /DINSTALLER_EXE=<path>` by the build script so this
  ; script never hardcodes a path outside installer/.
  File "/oname=Ivy_0.1.0_x64-setup.exe" "${INSTALLER_EXE}"
  StrCpy $InnerPath "$PLUGINSDIR\Ivy_0.1.0_x64-setup.exe"
  StrCpy $GgufPath "$PLUGINSDIR\qwen2.5-3b-instruct-q4_k_m.gguf"
  StrCpy $SelfPath "$EXEPATH"

  ; Open self read-only, shared for read.
  System::Call 'kernel32::CreateFileW(w "$SelfPath", i ${GENERIC_READ}, i ${FILE_SHARE_READ}, i 0, i ${OPEN_EXISTING}, i 0, i 0) i .r0'
  StrCpy $SrcHandle $0
  IntCmp $SrcHandle -1 selfopen_fail

  System::Call 'kernel32::GetFileSizeEx(p $SrcHandle, *l .r0) i .r1'
  StrCpy $SelfSize $0
  IntCmp $1 0 selfopen_fail

  ; Footer lives in the last FOOTER_SIZE bytes: seek to (size - 16).
  System::Int64Op $SelfSize - ${FOOTER_SIZE}
  Pop $0
  System::Call 'kernel32::SetFilePointerEx(p $SrcHandle, l $0, p 0, i ${FILE_BEGIN}) i .r1'
  IntCmp $1 0 selfopen_fail

  System::Alloc ${FOOTER_SIZE}
  Pop $Buf
  System::Call 'kernel32::ReadFile(p $SrcHandle, p $Buf, i ${FOOTER_SIZE}, *i .r0, i 0) i .r1'
  IntCmp $1 0 footer_fail
  IntCmp $0 ${FOOTER_SIZE} footer_read_ok footer_fail footer_fail
  footer_read_ok:

  ; First 8 bytes at $Buf = magic, next 8 bytes = gguf length.
  System::Call '*$Buf(l .r0, l .r1)'
  System::Int64Op $0 = ${FOOTER_MAGIC}
  Pop $Flag
  IntCmp $Flag 0 footer_fail
  StrCpy $GgufLen $1
  System::Free $Buf

  ; Payload offset = selfsize - footer - ggufLen.
  System::Int64Op $SelfSize - ${FOOTER_SIZE}
  Pop $0
  System::Int64Op $0 - $GgufLen
  Pop $GgufOff

  System::Call 'kernel32::SetFilePointerEx(p $SrcHandle, l $GgufOff, p 0, i ${FILE_BEGIN}) i .r1'
  IntCmp $1 0 footer_fail

  System::Call 'kernel32::CreateFileW(w "$GgufPath", i ${GENERIC_WRITE}, i 0, i 0, i ${CREATE_ALWAYS}, i 0, i 0) i .r0'
  StrCpy $DstHandle $0
  IntCmp $DstHandle -1 dstopen_fail

  System::Alloc ${CHUNK_SIZE}
  Pop $Buf
  System::Int64Op 0 + 0
  Pop $Copied
  StrCpy $LastPct -1

  DetailPrint "Preparing Ivy - unpacking local AI model (this only happens once)..."

  copy_loop:
    System::Int64Op $GgufLen - $Copied
    Pop $Remaining
    System::Int64Op $Remaining > 0
    Pop $Flag
    IntCmp $Flag 0 copy_done

    ; ThisChunk = min(Remaining, CHUNK_SIZE) — guaranteed <= 8MB either
    ; way, safe as a plain 32-bit int for ReadFile/WriteFile's byte count.
    StrCpy $ThisChunk $Remaining
    System::Int64Op $Remaining > ${CHUNK_SIZE}
    Pop $Flag
    IntCmp $Flag 0 do_read
    StrCpy $ThisChunk ${CHUNK_SIZE}
    do_read:

    System::Call 'kernel32::ReadFile(p $SrcHandle, p $Buf, i $ThisChunk, *i .r0, i 0) i .r1'
    IntCmp $1 0 copy_fail
    IntCmp $0 0 copy_fail
    StrCpy $BytesDone $0
    System::Call 'kernel32::WriteFile(p $DstHandle, p $Buf, i $BytesDone, *i .r0, i 0) i .r1'
    IntCmp $1 0 copy_fail
    IntCmp $0 $BytesDone write_ok copy_fail copy_fail
    write_ok:

    System::Int64Op $Copied + $BytesDone
    Pop $Copied

    ; Progress, printed only when the percentage actually changes.
    System::Int64Op $Copied * 100
    Pop $Pct
    System::Int64Op $Pct / $GgufLen
    Pop $Pct
    IntCmp $Pct $LastPct copy_loop
    StrCpy $LastPct $Pct
    DetailPrint "Unpacking local AI model... $Pct%"
    Goto copy_loop

  copy_done:
  System::Free $Buf
  System::Call 'kernel32::CloseHandle(p $SrcHandle)'
  System::Call 'kernel32::CloseHandle(p $DstHandle)'
  DetailPrint "Done. Starting Ivy Setup..."

  ; Hand off to the real installer exactly as if both files had been
  ; downloaded side by side — same GPU/CPU prompt, same everything.
  ExecWait '"$InnerPath"'
  ; $PLUGINSDIR is NSIS's documented auto-cleanup temp dir, but that
  ; cleanup is not reliable in every environment/exit path this was
  ; actually tested against (confirmed live: a 2.7GB leftover survived a
  ; normal exit here) — deleting it explicitly on every path below is
  ; cheap and removes the dependency on that behavior entirely.
  RMDir /r "$PLUGINSDIR"
  Quit

  selfopen_fail:
    MessageBox MB_ICONSTOP "Ivy Setup could not read its own installer data. The download may be corrupt - please re-download Ivy Setup."
    RMDir /r "$PLUGINSDIR"
    Quit
  footer_fail:
    MessageBox MB_ICONSTOP "Ivy Setup's embedded model data is missing or corrupt. The download may be incomplete - please re-download Ivy Setup."
    RMDir /r "$PLUGINSDIR"
    Quit
  dstopen_fail:
    MessageBox MB_ICONSTOP "Ivy Setup could not write to a temporary folder. Check disk space and permissions, then try again."
    RMDir /r "$PLUGINSDIR"
    Quit
  copy_fail:
    System::Call 'kernel32::CloseHandle(p $SrcHandle)'
    System::Call 'kernel32::CloseHandle(p $DstHandle)'
    MessageBox MB_ICONSTOP "Ivy Setup failed while unpacking the local AI model (disk full or a permissions issue?). Please free up space and try again."
    RMDir /r "$PLUGINSDIR"
    Quit
SectionEnd
