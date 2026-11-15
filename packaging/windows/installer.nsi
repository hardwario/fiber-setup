; FIBER Setup: the Windows installer (NSIS 3, Unicode; this file is UTF-8).
;
; Installs for all users into Program Files: the app runs as administrator, so its files must
; not lie where a user (or a program running as the user) can replace them. Besides copying
; the files it does two things a zip cannot: it installs WebView2, which the app's window
; needs, where Windows does not have it yet, and it installs the USB driver for a FIBER in USB
; mode.
;
; Built in two passes, so that the uninstaller can be signed (scripts/windows-package.ps1):
;   1. with /DUNINSTALLER_ONLY: a program that only writes uninstall.exe and quits;
;   2. without: the installer, which carries the uninstall.exe signed in between.
;
; Given on the command line:
;   /DVERSION=1.2.3          the app's version
;   /DAPP_DIR=<dir>          fiber-setup.exe and the usbboot folder, as they will be installed
;   /DOUT_FILE=<file>        what to write
;   /DUNINSTALLER=<file>     pass 2: the uninstall.exe to install
;   /DWEBVIEW2=<file>        pass 2, optional: Microsoft's MicrosoftEdgeWebview2Setup.exe

Unicode true
ManifestDPIAware true
SetCompressor /SOLID lzma
RequestExecutionLevel admin

!include "MUI2.nsh"
!include "x64.nsh"
!include "FileFunc.nsh"

!define PRODUCT "FIBER Setup"
!define PUBLISHER "HARDWARIO a.s."
!define EXE "fiber-setup.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\FIBER Setup"
; WebView2's entry among Edge's installed products (32-bit registry view).
!define WEBVIEW2_KEY "SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

Name "${PRODUCT}"
OutFile "${OUT_FILE}"
InstallDir "$PROGRAMFILES64\${PRODUCT}"
BrandingText "${PRODUCT} ${VERSION}"
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "© 2026 ${PUBLISHER}"
VIAddVersionKey "FileDescription" "${PRODUCT} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"

!define MUI_ICON "..\..\src-tauri\icons\icon.ico"
!define MUI_UNICON "..\..\src-tauri\icons\icon.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!ifdef UNINSTALLER_ONLY
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!endif

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Czech"

LangString Running ${LANG_ENGLISH} "${PRODUCT} is running. Close it, then choose Retry."
LangString Running ${LANG_CZECH} "${PRODUCT} je spuštěný. Zavřete ho a pak zvolte Opakovat."
LangString WebView ${LANG_ENGLISH} "Installing Microsoft Edge WebView2, which the app's window needs..."
LangString WebView ${LANG_CZECH} "Instaluje se Microsoft Edge WebView2, který potřebuje okno aplikace..."
LangString Driver ${LANG_ENGLISH} "Installing the USB driver for a FIBER in USB mode. Windows may ask you to confirm it."
LangString Driver ${LANG_CZECH} "Instaluje se ovladač USB pro FIBER v režimu USB. Windows vás mohou požádat o potvrzení."
LangString DriverLater ${LANG_ENGLISH} "The USB driver was not installed now; the app offers it again in its Connect step."
LangString DriverLater ${LANG_CZECH} "Ovladač USB se teď nenainstaloval; aplikace ho nabídne znovu v kroku Připojení."
LangString Need64 ${LANG_ENGLISH} "${PRODUCT} needs a 64-bit Windows 10 or 11."
LangString Need64 ${LANG_CZECH} "${PRODUCT} potřebuje 64bitové Windows 10 nebo 11."

!ifdef UNINSTALLER_ONLY

; Pass 1: nothing is installed. Starting this program writes uninstall.exe next to it.
Function .onInit
  WriteUninstaller "$EXEDIR\uninstall.exe"
  SetErrorLevel 0
  Quit
FunctionEnd

Section
SectionEnd

Section "Uninstall"
  SetRegView 64
  SetShellVarContext all
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  ; Only what the installer put there: the app, its usbboot folder, this uninstaller.
  Delete "$INSTDIR\${EXE}"
  RMDir /r "$INSTDIR\usbboot"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKLM "${UNINSTALL_KEY}"
  ; The USB driver stays in Windows: it is a few lines that harm nothing, and another program
  ; that talks to a Raspberry Pi in USB mode may rely on it by now.
SectionEnd

!else

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "$(Need64)"
    Abort
  ${EndIf}
  SetRegView 64
FunctionEnd

Section "Install"
  SetShellVarContext all
  SetOutPath "$INSTDIR"

  ; A running app cannot be replaced: ask for it to be closed.
  retry:
  ClearErrors
  Delete "$INSTDIR\${EXE}"
  ${If} ${Errors}
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(Running)" /SD IDCANCEL IDRETRY retry
    Abort
  ${EndIf}
  ; An older version's helper files must not stay beside the new ones.
  RMDir /r "$INSTDIR\usbboot"

  File /r "${APP_DIR}\*.*"
  File "/oname=uninstall.exe" "${UNINSTALLER}"
  CreateShortcut "$SMPROGRAMS\${PRODUCT}.lnk" "$INSTDIR\${EXE}"

  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${EXE}"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr HKLM "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/hardwario/fiber-setup"
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINSTALL_KEY}" "EstimatedSize" "$0"

!ifdef WEBVIEW2
  ; Windows 11 has WebView2, and so do most Windows 10: installed is a version other than
  ; nothing and 0.0.0.0, for the computer or for this user.
  SetRegView 32
  ReadRegStr $0 HKLM "${WEBVIEW2_KEY}" "pv"
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    ReadRegStr $0 HKCU "${WEBVIEW2_KEY}" "pv"
  ${EndIf}
  SetRegView 64
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    DetailPrint "$(WebView)"
    InitPluginsDir
    File "/oname=$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" "${WEBVIEW2}"
    ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1
    DetailPrint "MicrosoftEdgeWebview2Setup.exe: $1"
  ${EndIf}
!endif

  ; The USB driver. pnputil is a 64-bit program; this installer is a 32-bit one, to which
  ; Windows shows System32 as another folder, so it is reached through Sysnative. 0: installed;
  ; 259: added, with no FIBER connected to bind it to; 3010: installed, wants a restart.
  DetailPrint "$(Driver)"
  nsExec::ExecToLog '"$WINDIR\Sysnative\pnputil.exe" /add-driver "$INSTDIR\usbboot\driver\fiber-usbboot.inf" /install'
  Pop $0
  ${If} $0 != 0
  ${AndIf} $0 != 259
  ${AndIf} $0 != 3010
    DetailPrint "$(DriverLater) (pnputil: $0)"
  ${EndIf}
SectionEnd

!endif
