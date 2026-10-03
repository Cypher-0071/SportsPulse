; SportsPulse Windows installer (NSIS 3, per-user, no admin required).
;
; Build (from this directory):
;   makensis -DVERSION=<ver> -DOUTFILE=<name> sportspulse.nsi
; Defaults below allow a bare `makensis sportspulse.nsi` too.
;
; All source paths use forward slashes so the same script compiles on Linux
; (cross builds) and Windows; NSIS normalizes them for the target.

!ifndef VERSION
!define VERSION "0.1.0"
!endif
!ifndef OUTFILE
!define OUTFILE "SportsPulse-Setup.exe"
!endif

!define APPNAME "SportsPulse"
!define APP_EXE "sportspulse.exe"
!define PUBLISHER "Cypher-0071"
!define SRC_EXE "../target/x86_64-pc-windows-msvc/release/sportspulse.exe"
!define APP_ICON "../assets/icon.ico"
!define LICENSE_FILE "../../LICENSE"

Name "${APPNAME}"
OutFile "${OUTFILE}"
Unicode True
RequestExecutionLevel user
InstallDir "$LOCALAPPDATA\${APPNAME}"
InstallDirRegKey HKCU "Software\${APPNAME}" "InstallDir"
Icon "${APP_ICON}"
UninstallIcon "${APP_ICON}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "${APPNAME} live sports scoreboard"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "(c) ${PUBLISHER}"

!include "MUI2.nsh"
!include "LogicLib.nsh"

!define MUI_ABORTWARNING
!define MUI_ICON "${APP_ICON}"
!define MUI_UNICON "${APP_ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Run ${APPNAME} now"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${LICENSE_FILE}"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Close a running copy (tray -> Quit works too). The main window answers
; WM_CLOSE (0x10) with a full quit; returns nonzero while one is still alive.
Function CloseRunningApp
  FindWindow $0 "SPNativeMain" ""
  ${If} $0 != 0
    SendMessage $0 0x10 0 0
    Sleep 2000
    FindWindow $0 "SPNativeMain" ""
  ${EndIf}
FunctionEnd

Section "${APPNAME} (required)" SEC_MAIN
  SectionIn RO

  Call CloseRunningApp
  ${If} $0 != 0
    MessageBox MB_OK "${APPNAME} is still running. Quit it from the tray icon, then run setup again."
    Abort
  ${EndIf}

  SetOutPath "$INSTDIR"
  File "${SRC_EXE}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "Software\${APPNAME}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\${APPNAME}" "Version" "${VERSION}"

  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}" "NoRepair" 1
SectionEnd

Section "Start with Windows" SEC_AUTOSTART
  ; Silent tray start (no window steals focus at login).
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APPNAME}" '"$INSTDIR\${APP_EXE}" --minimized'
SectionEnd

Section /o "Desktop shortcut" SEC_DESKTOP
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_MAIN} "The ${APPNAME} scoreboard, tray icon and uninstaller."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_AUTOSTART} "Launch ${APPNAME} silently into the tray at login."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} "Add a ${APPNAME} icon to the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Call un.CloseRunningApp

  Delete "$INSTDIR\${APP_EXE}"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  RMDir "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APPNAME}"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"
  DeleteRegKey HKCU "Software\${APPNAME}"
  RMDir "$INSTDIR"
SectionEnd

Function un.CloseRunningApp
  FindWindow $0 "SPNativeMain" ""
  ${If} $0 != 0
    SendMessage $0 0x10 0 0
    Sleep 2000
  ${EndIf}
FunctionEnd
