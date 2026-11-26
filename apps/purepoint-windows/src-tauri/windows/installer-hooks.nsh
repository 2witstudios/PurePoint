; PurePoint NSIS installer hooks (WIN-10.2, REQ-WINAPP-077), included by Tauri's installer.nsi
; through bundle.windows.nsis.installerHooks.
;
; Install and uninstall first stop what runs from the install dir, so no file is locked:
;   1. the app itself (so it does not restart the daemon or Point Guard in between),
;   2. the daemon, gracefully with `pu.exe daemon stop` (agents are suspended and resume on the
;      next start),
;   3. anything still running from $INSTDIR: Point Guard's node.exe and lock helper, a daemon
;      that did not stop.
; Uninstall (not the uninstall an update runs) also deletes the HKCU Run values `pu daemon
; install` and `pu phone` write. %USERPROFILE%\.pu (the installed CLI, projects, Point Guard
; payloads) is never touched; Tauri's template already deletes the app's own Run value.

!define PUREPOINT_RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define PUREPOINT_STARTUP_APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
; $R9 is the native PowerShell (set below). The installer is a 32-bit process, and a 32-bit
; PowerShell cannot read the image path of the 64-bit app, so 64-bit Windows uses Sysnative.
!define PUREPOINT_POWERSHELL `"$R9" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command`
; Processes whose image is under $env:PUREPOINT_INSTDIR (an environment variable, so a quote in
; the path cannot break the command).
!define PUREPOINT_UNDER_INSTDIR `$$d = [IO.Path]::GetFullPath($$env:PUREPOINT_INSTDIR).TrimEnd('\') + '\'; Get-Process | Where-Object { $$_.Path -and $$_.Path.StartsWith($$d, [StringComparison]::OrdinalIgnoreCase) }`

!macro PUREPOINT_STOP_RUNNING
  System::Call 'Kernel32::SetEnvironmentVariable(t "PUREPOINT_INSTDIR", t "$INSTDIR") i .r0'
  StrCpy $R9 "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  ${If} ${RunningX64}
  ${AndIf} ${FileExists} "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
    StrCpy $R9 "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
  ${EndIf}
  nsExec::ExecToLog /TIMEOUT=15000 `${PUREPOINT_POWERSHELL} "${PUREPOINT_UNDER_INSTDIR} | Where-Object { $$_.Name -eq '${MAINBINARYNAME}' } | Stop-Process -Force -ErrorAction SilentlyContinue"`
  Pop $0
  ${If} ${FileExists} "$INSTDIR\pu.exe"
    nsExec::ExecToLog /TIMEOUT=30000 `"$INSTDIR\pu.exe" daemon stop`
    Pop $0
  ${EndIf}
  nsExec::ExecToLog /TIMEOUT=15000 `${PUREPOINT_POWERSHELL} "${PUREPOINT_UNDER_INSTDIR} | Stop-Process -Force -ErrorAction SilentlyContinue"`
  Pop $0
  ; Let Windows release the image handles before files are replaced or deleted.
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PUREPOINT_STOP_RUNNING
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro PUREPOINT_STOP_RUNNING
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "${PUREPOINT_RUN_KEY}" "PurePoint Engine"
    DeleteRegValue HKCU "${PUREPOINT_RUN_KEY}" "PurePoint Point Guard"
    DeleteRegValue HKCU "${PUREPOINT_STARTUP_APPROVED_KEY}" "PurePoint Engine"
    DeleteRegValue HKCU "${PUREPOINT_STARTUP_APPROVED_KEY}" "PurePoint Point Guard"
  ${EndIf}
!macroend
