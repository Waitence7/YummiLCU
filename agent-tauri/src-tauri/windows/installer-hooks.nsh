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

; Interactive installs skip the Finish page. Let the installer finish first;
; .onInstSuccess marks the launch and .onGUIEnd starts the Agent only after the
; installer window has fully closed. Silent/update installs remain unattended.
!macro NSIS_HOOK_POSTINSTALL
  IfSilent yummi_postinstall_done 0
  SetAutoClose true
  yummi_postinstall_done:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro YUMMI_STOP_RUNNING_AGENT
!macroend
