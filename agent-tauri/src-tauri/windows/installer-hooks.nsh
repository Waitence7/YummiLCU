; Let the running Agent remove its tray icon and stop its background workers
; before Tauri's installer falls back to its normal force-close prompt.
!macro YUMMI_STOP_RUNNING_AGENT
  !define YummiStopUniqueId ${__LINE__}

  nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
  Pop $R0
  ${If} $R0 = 0
    DetailPrint "Stopping the running ${PRODUCTNAME} cleanly..."
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --shutdown-for-install' $R1

    StrCpy $R2 0
    yummi_wait_for_exit_${YummiStopUniqueId}:
      nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
      Pop $R0
      ${If} $R0 != 0
        Goto yummi_stop_done_${YummiStopUniqueId}
      ${EndIf}

      IntOp $R2 $R2 + 1
      ${If} $R2 < 50
        Sleep 100
        Goto yummi_wait_for_exit_${YummiStopUniqueId}
      ${EndIf}
  ${EndIf}

  yummi_stop_done_${YummiStopUniqueId}:
  !undef YummiStopUniqueId
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro YUMMI_STOP_RUNNING_AGENT
!macroend

; Interactive installs start a tiny post-install Agent handoff. The Agent
; receives this installer's PID and waits for the process to really exit before
; creating/focusing its main window. This avoids relying on NSIS .onGUIEnd,
; whose timing varies across Windows/NSIS configurations.
!macro NSIS_HOOK_POSTINSTALL
  IfSilent yummi_postinstall_done 0
  ${If} $PassiveMode != 1
    System::Call 'kernel32::GetCurrentProcessId() i .r3'
    DetailPrint "Scheduling ${PRODUCTNAME} to open after setup closes..."
    ClearErrors
    Exec '"$INSTDIR\${MAINBINARYNAME}.exe" --post-install-launch=$3'
    ${If} ${Errors}
      ; currentUser installers normally use Exec directly. Keep RunAsUser as a
      ; compatibility fallback if process creation is rejected by Windows.
      nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" "--post-install-launch=$3"
    ${EndIf}
  ${EndIf}
  SetAutoClose true
  yummi_postinstall_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro YUMMI_STOP_RUNNING_AGENT
!macroend
