; Tauri 2.11.4 custom NSIS template: one current-user installation, no data purge,
; process termination, dependency installation, or application auto-launch.
Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma
!include MUI2.nsh
!include LogicLib.nsh
!include FileFunc.nsh
!include x64.nsh
!include "Win\COM.nsh"
!include "Win\Propkey.nsh"
!include "utils.nsh"
!addplugindir "{{additional_plugins_path}}"

!define PRODUCTNAME "{{product_name}}"
!define VERSION "{{version}}"
!define BUNDLEID "{{bundle_id}}"
!define MAINBINARYNAME "{{main_binary_name}}"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}"
!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
!if "{{arch}}" != "x64"
  !error "This installer contract supports only the verified x64 application."
!endif
{{#each resources}}
!error "Review the owned-file contract before adding installer resources."
{{/each}}
{{#each binaries}}
!error "Review the owned-file contract before adding external binaries."
{{/each}}

Name "${PRODUCTNAME}"
OutFile "{{out_file}}"
InstallDir "$LOCALAPPDATA\${PRODUCTNAME}"
VIProductVersion "{{version_with_build}}"
VIAddVersionKey "ProductName" "${PRODUCTNAME}"
VIAddVersionKey "FileDescription" "${PRODUCTNAME}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
!if "{{installer_icon}}" != ""
  !define MUI_ICON "{{installer_icon}}"
!endif
!if "{{uninstaller_sign_cmd}}" != ""
  !uninstfinalize '{{uninstaller_sign_cmd}}'
!endif
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

!macro Fail message
  SetErrorLevel 2
  Abort "${message}"
!macroend

!macro RequireStopped
  nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 != 1
    !insertmacro Fail "Quit PR Sniper first. No process was stopped."
  ${EndIf}
!macroend

!macro RequireOwner
  ReadRegStr $0 HKCU "${UNINSTKEY}" "PRSniperInstaller"
  ${If} $0 != "${BUNDLEID}|$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro Fail "Installer ownership does not match this exact application path."
  ${EndIf}
  ReadRegStr $0 HKCU "${UNINSTKEY}" "InstallLocation"
  ${If} $0 != "$INSTDIR"
    !insertmacro Fail "InstallLocation belongs to another installation."
  ${EndIf}
  ReadRegStr $0 HKCU "${UNINSTKEY}" "UninstallString"
  ${If} $0 != '$\"$INSTDIR\uninstall.exe$\"'
    !insertmacro Fail "UninstallString belongs to another installation."
  ${EndIf}
!macroend

!macro Context
  SetShellVarContext current
  SetRegView 64
  ${IfNot} ${RunningX64}
    !insertmacro Fail "This installer requires Windows x64."
  ${EndIf}
  ${If} $INSTDIR != "$LOCALAPPDATA\${PRODUCTNAME}"
    !insertmacro Fail "Only the stable current-user application directory is supported."
  ${EndIf}
!macroend

Function .onInit
  !insertmacro Context
  !insertmacro RequireStopped
  ; Distinguish an absent installer key from foreign or unreadable state.
  System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${UNINSTKEY}", i 0, i 0x20119, *p.r0) i.r1'
  ${If} $1 = 0
    System::Call 'advapi32::RegCloseKey(p r0)'
    !insertmacro RequireOwner
    ReadRegStr $0 HKCU "${UNINSTKEY}" "DisplayVersion"
    nsis_tauri_utils::SemverCompare "${VERSION}" $0
    Pop $0
    ${If} $0 != 0
    ${AndIf} $0 != 1
      !insertmacro Fail "Downgrades and unknown installed versions are not supported."
    ${EndIf}
  ${ElseIf} $1 = 2
    ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${OrIf} ${FileExists} "$INSTDIR\uninstall.exe"
      !insertmacro Fail "An existing application is not owned by this installer."
    ${EndIf}
  ${Else}
    !insertmacro Fail "Cannot inspect existing installer ownership."
  ${EndIf}
  ${If} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $0
    ${If} $0 != 1
      !insertmacro Fail "The Start Menu shortcut belongs to another installation."
    ${EndIf}
  ${EndIf}
  ReadRegStr $0 HKLM "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${If} $0 == ""
    ReadRegStr $0 HKCU "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}" "pv"
  ${EndIf}
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    !insertmacro Fail "Install Microsoft Edge WebView2 Evergreen Runtime first, then retry."
  ${EndIf}
FunctionEnd

Section Install
  !insertmacro RequireStopped
  SetOutPath "$INSTDIR"
  File "{{main_binary_path}}"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "${UNINSTKEY}" "PRSniperInstaller" "${BUNDLEID}|$INSTDIR\${MAINBINARYNAME}.exe"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${PRODUCTNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\"'
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "Dylan McCurry"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  ${If} ${Errors}
    !insertmacro Fail "Installation was incomplete. Inspect the exact application directory."
  ${EndIf}
SectionEnd

Function un.onInit
  !insertmacro Context
  !insertmacro RequireOwner
  ReadRegStr $0 HKCU "${UNINSTKEY}" "DisplayVersion"
  ${If} $0 != "${VERSION}"
    !insertmacro Fail "This uninstaller belongs to a different application version."
  ${EndIf}
  !insertmacro RequireStopped
FunctionEnd

Section Uninstall
  !insertmacro RequireOwner
  !insertmacro RequireStopped
  ClearErrors
  Delete "$INSTDIR\${MAINBINARYNAME}.exe"
  ${If} ${Errors}
    !insertmacro Fail "Cannot remove the application. Quit it and retry."
  ${EndIf}
  !insertmacro IsShortcutTarget "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 1
    Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  ${EndIf}
  Delete "$INSTDIR\uninstall.exe"
  ; These are the only installer-owned values. Preserve unknown values/children.
  DeleteRegValue HKCU "${UNINSTKEY}" "PRSniperInstaller"
  DeleteRegValue HKCU "${UNINSTKEY}" "DisplayName"
  DeleteRegValue HKCU "${UNINSTKEY}" "DisplayVersion"
  DeleteRegValue HKCU "${UNINSTKEY}" "DisplayIcon"
  DeleteRegValue HKCU "${UNINSTKEY}" "Publisher"
  DeleteRegValue HKCU "${UNINSTKEY}" "InstallLocation"
  DeleteRegValue HKCU "${UNINSTKEY}" "UninstallString"
  DeleteRegValue HKCU "${UNINSTKEY}" "QuietUninstallString"
  DeleteRegValue HKCU "${UNINSTKEY}" "NoModify"
  DeleteRegValue HKCU "${UNINSTKEY}" "NoRepair"
  DeleteRegKey /ifempty HKCU "${UNINSTKEY}"
  RMDir "$INSTDIR"
SectionEnd
