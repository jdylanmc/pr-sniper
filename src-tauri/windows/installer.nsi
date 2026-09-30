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
!ifndef PR_SNIPER_TEST_BUILD
  !define WEBVIEW_MACHINE_KEY "SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}"
  !define WEBVIEW_USER_KEY "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2APPGUID}"
!endif
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

Var LifecycleMutex
Var Registry
Var PreviousVersion
Var RegistrationVersion
Var Transaction
Var ShortcutStage
Var ShortcutBackup
Var OldAppMoved
Var OldUninstallerMoved
Var OldShortcutMoved
Var NewAppPlaced
Var NewUninstallerPlaced
Var NewShortcutPlaced
Var RegistryChanged
Var RegistryCreated
Var OperationFailed
Var RollbackFailed
Var FailureStage
Var OriginalFailure
Var ShortcutState
Var RevalidationFailureHandler

!macro Fail message
  SetErrorLevel 2
  ${If} $RevalidationFailureHandler > 0
    StrCpy $FailureStage "${message}"
    Call $RevalidationFailureHandler
  ${EndIf}
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
  ClearErrors
  ReadRegStr $0 HKCU "${UNINSTKEY}" "PRSniperInstaller"
  ${If} ${Errors}
    !insertmacro Fail "Cannot read installer ownership."
  ${EndIf}
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

; The previous registry state must match this finite schema before any write.
; Consequently PreviousVersion plus the validated schema is an exact snapshot;
; unknown values/subkeys are never included in rollback or removal.
!macro ExpectString name value
  System::Call 'advapi32::RegQueryValueExW(p $Registry, w "${name}", p 0, *i.r1, p 0, *i.r2) i.r0'
  ${If} $0 != 0
  ${OrIf} $1 != 1
    !insertmacro Fail "Unexpected installer value type: ${name}."
  ${EndIf}
  StrLen $R8 "${value}"
  IntOp $R8 $R8 + 1
  IntOp $R8 $R8 * 2
  ${If} $2 != $R8
    !insertmacro Fail "Unexpected installer value byte length: ${name}."
  ${EndIf}
  ClearErrors
  ReadRegStr $0 HKCU "${UNINSTKEY}" "${name}"
  ${If} ${Errors}
    !insertmacro Fail "Cannot read installer value: ${name}."
  ${EndIf}
  ${If} $0 S!= "${value}"
    !insertmacro Fail "Unexpected installer value: ${name}."
  ${EndIf}
!macroend

!macro ExpectDword name
  System::Call 'advapi32::RegQueryValueExW(p $Registry, w "${name}", p 0, *i.r1, p 0, *i.r2) i.r0'
  ${If} $0 != 0
  ${OrIf} $1 != 4
  ${OrIf} $2 != 4
    !insertmacro Fail "Unexpected installer value type: ${name}."
  ${EndIf}
  ClearErrors
  ReadRegDWORD $0 HKCU "${UNINSTKEY}" "${name}"
  ${If} ${Errors}
  ${OrIf} $0 != 1
    !insertmacro Fail "Unexpected installer value: ${name}."
  ${EndIf}
!macroend

!macro SetString name value
  StrLen $R8 "${value}"
  IntOp $R8 $R8 + 1
  IntOp $R8 $R8 * 2
  System::Call 'advapi32::RegSetValueExW(p $Registry, w "${name}", i 0, i 1, w "${value}", i $R8) i.r0'
  ${If} $0 != 0
    StrCpy $OperationFailed 1
    StrCpy $FailureStage "write registry value ${name}"
    Return
  ${EndIf}
!macroend

!macro SetDword name
  System::Call 'advapi32::RegSetValueExW(p $Registry, w "${name}", i 0, i 4, *i 1, i 4) i.r0'
  ${If} $0 != 0
    StrCpy $OperationFailed 1
    StrCpy $FailureStage "write registry value ${name}"
    Return
  ${EndIf}
!macroend

!macro RemoveValue name
  System::Call 'advapi32::RegDeleteValueW(p $Registry, w "${name}") i.r0'
  ${If} $0 != 0
  ${AndIf} $0 != 2
    StrCpy $OperationFailed 1
    StrCpy $FailureStage "remove registry value ${name}"
    Return
  ${EndIf}
  System::Call 'advapi32::RegQueryValueExW(p $Registry, w "${name}", p 0, p 0, p 0, p 0) i.r0'
  ${If} $0 != 2
    StrCpy $OperationFailed 1
    StrCpy $FailureStage "verify removed registry value ${name}"
    Return
  ${EndIf}
!macroend

!macro Fault point label
!ifdef PR_SNIPER_TEST_BUILD
  ReadEnvStr $0 PR_SNIPER_NSIS_TEST_FAIL
  ${If} $0 == "${point}"
    StrCpy $FailureStage "injected ${point}"
    Goto ${label}
  ${EndIf}
!endif
!macroend

!macro MoveOwned from to flag label
  ClearErrors
  Rename "${from}" "${to}"
  ${If} ${Errors}
    StrCpy $FailureStage "move ${from}"
    Goto ${label}
  ${EndIf}
  StrCpy ${flag} 1
!macroend

!macro RestoreFile from to flag
  ${If} ${flag} = 1
    ClearErrors
    Rename "${from}" "${to}"
    ${If} ${Errors}
      StrCpy $RollbackFailed 1
      DetailPrint "Rollback failed: restore ${to}"
    ${EndIf}
  ${EndIf}
!macroend

!macro RemoveNew path flag
  ${If} ${flag} = 1
    ClearErrors
    Delete "${path}"
    ${If} ${Errors}
      StrCpy $RollbackFailed 1
      DetailPrint "Rollback failed: remove new ${path}"
    ${EndIf}
  ${EndIf}
!macroend

!macro CleanupFile path
  ${If} ${FileExists} "${path}"
    ClearErrors
    Delete "${path}"
    ${If} ${Errors}
      !insertmacro Fail "Owned transaction cleanup failed: ${path}. Recovery evidence retained."
    ${EndIf}
  ${EndIf}
!macroend

!macro LifecycleFunctions prefix
Function ${prefix}InspectShortcut
  StrCpy $ShortcutState "unreadable"
  System::Call 'kernel32::GetFileAttributesW(w "$SMPROGRAMS\${PRODUCTNAME}.lnk") i.r0 ?e'
  Pop $1
  ${If} $0 = -1
    ${If} $1 = 2
    ${OrIf} $1 = 3
      StrCpy $ShortcutState "absent"
    ${EndIf}
    Return
  ${EndIf}
  IntOp $1 $0 & 0x10
  ${If} $1 != 0
    Return
  ${EndIf}
  System::Call 'ole32::CoInitializeEx(p 0, i 2) i.r4'
  ${If} $4 != 0
  ${AndIf} $4 != 1
    Return
  ${EndIf}
  StrCpy $0 0
  StrCpy $1 0
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ".r3"
  ${If} $3 = 0
  ${AndIf} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}", .r1).r3'
    ${If} $3 = 0
    ${AndIf} $1 P<> 0
      ${IPersistFile::Load} $1 '("$SMPROGRAMS\${PRODUCTNAME}.lnk", ${STGM_READ}).r3'
      ${If} $3 = 0
        StrCpy $2 ""
        ${IShellLink::GetPath} $0 '(.r2, ${NSIS_MAX_STRLEN}, 0, ${SLGP_RAWPATH}).r3'
        ${If} $3 = 0
        ${AndIf} $2 != ""
          StrCpy $ShortcutState "foreign"
          ${If} $2 == "$INSTDIR\${MAINBINARYNAME}.exe"
            StrCpy $ShortcutState "owned"
          ${EndIf}
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ${If} $1 P<> 0
    ${IUnknown::Release} $1 ""
  ${EndIf}
  ${If} $0 P<> 0
    ${IUnknown::Release} $0 ""
  ${EndIf}
  System::Call 'ole32::CoUninitialize()'
FunctionEnd

Function ${prefix}ReleaseLifecycle
  ${If} $LifecycleMutex != 0
    System::Call 'kernel32::ReleaseMutex(p $LifecycleMutex)'
    System::Call 'kernel32::CloseHandle(p $LifecycleMutex)'
    StrCpy $LifecycleMutex 0
  ${EndIf}
FunctionEnd

!if "${prefix}" == ""
Function .onGUIEnd
!else
Function un.onGUIEnd
!endif
  Call ${prefix}ReleaseLifecycle
FunctionEnd

Function ${prefix}AcquireLifecycle
  ; A Global mutex is cross-session but SID-scoped, not machine-wide product
  ; exclusion. No elevated privilege is needed to create a named mutex.
  System::Call 'kernel32::GetCurrentProcess() p.r0'
  System::Call 'advapi32::OpenProcessToken(p r0, i 8, *p.r1) i.r2'
  ${If} $2 = 0
    !insertmacro Fail "Cannot identify the current installer user."
  ${EndIf}
  System::Call 'advapi32::GetTokenInformation(p r1, i 1, p 0, i 0, *i.r2)'
  System::Alloc $2
  Pop $3
  System::Call 'advapi32::GetTokenInformation(p r1, i 1, p r3, i r2, *i.r2) i.r4'
  System::Call 'kernel32::CloseHandle(p r1)'
  ${If} $4 = 0
    System::Free $3
    !insertmacro Fail "Cannot read the current installer user."
  ${EndIf}
  System::Call '*$3(p.r4)'
  System::Call 'advapi32::ConvertSidToStringSidW(p r4, *p.r5) i.r4'
  System::Free $3
  ${If} $4 = 0
    !insertmacro Fail "Cannot format the current installer identity."
  ${EndIf}
  System::Call '*$5(&w${NSIS_MAX_STRLEN}.r6)'
  System::Call 'kernel32::LocalFree(p r5)'
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Global\${BUNDLEID}.installer.$6") p.r0'
  StrCpy $LifecycleMutex $0
  ${If} $LifecycleMutex = 0
    !insertmacro Fail "Cannot acquire the current-user installer lock."
  ${EndIf}
  System::Call 'kernel32::WaitForSingleObject(p $LifecycleMutex, i 0) i.r0'
  ${If} $0 != 0
  ${AndIf} $0 != 128
    !insertmacro Fail "Another PR Sniper installer/uninstaller owns this user's installation."
  ${EndIf}
FunctionEnd

Function ${prefix}ValidateRegisteredState
  !insertmacro RequireOwner
  ClearErrors
  ReadRegStr $PreviousVersion HKCU "${UNINSTKEY}" "DisplayVersion"
  ${If} ${Errors}
  ${OrIf} $PreviousVersion == ""
    !insertmacro Fail "Cannot read the installed version."
  ${EndIf}
  !insertmacro ExpectString "PRSniperInstaller" "${BUNDLEID}|$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro ExpectString "DisplayName" "${PRODUCTNAME}"
  !insertmacro ExpectString "DisplayVersion" "$PreviousVersion"
  !insertmacro ExpectString "DisplayIcon" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\"'
  !insertmacro ExpectString "Publisher" "Dylan McCurry"
  !insertmacro ExpectString "InstallLocation" "$INSTDIR"
  !insertmacro ExpectString "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  !insertmacro ExpectString "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
  !insertmacro ExpectDword "NoModify"
  !insertmacro ExpectDword "NoRepair"
FunctionEnd

Function ${prefix}PrepareTransaction
  StrCpy $Registry 0
  StrCpy $Transaction "$INSTDIR\.pr-sniper-transaction"
  StrCpy $ShortcutStage "$SMPROGRAMS\${PRODUCTNAME}.pr-sniper-stage.lnk"
  StrCpy $ShortcutBackup "$SMPROGRAMS\${PRODUCTNAME}.pr-sniper-backup.lnk"
  ${If} ${FileExists} "$Transaction"
  ${OrIf} ${FileExists} "$ShortcutStage"
  ${OrIf} ${FileExists} "$ShortcutBackup"
    !insertmacro Fail "A prior transaction/foreign staging path needs exact-path recovery. Nothing was replaced."
  ${EndIf}
  StrCpy $OldAppMoved 0
  StrCpy $OldUninstallerMoved 0
  StrCpy $OldShortcutMoved 0
  StrCpy $NewAppPlaced 0
  StrCpy $NewUninstallerPlaced 0
  StrCpy $NewShortcutPlaced 0
  StrCpy $RegistryChanged 0
  StrCpy $RegistryCreated 0
  ClearErrors
  CreateDirectory "$Transaction"
  ${If} ${Errors}
    !insertmacro Fail "Cannot create the owned transaction directory."
  ${EndIf}
  ClearErrors
  WriteINIStr "$Transaction\recovery.ini" "schema-v1" "owner" "${BUNDLEID}|$INSTDIR"
  WriteINIStr "$Transaction\recovery.ini" "schema-v1" "previous-version" "$PreviousVersion"
  ${If} ${Errors}
    !insertmacro Fail "Cannot persist transaction recovery evidence. Inspect the exact staging directory."
  ${EndIf}
FunctionEnd

Function ${prefix}WriteRegistration
  StrCpy $OperationFailed 0
  !insertmacro SetString "PRSniperInstaller" "${BUNDLEID}|$INSTDIR\${MAINBINARYNAME}.exe"
  !insertmacro SetString "DisplayName" "${PRODUCTNAME}"
  !insertmacro SetString "DisplayIcon" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\"'
  !insertmacro SetString "Publisher" "Dylan McCurry"
  !insertmacro SetString "InstallLocation" "$INSTDIR"
  !insertmacro SetString "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  !insertmacro SetString "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
  !insertmacro SetDword "NoModify"
  !insertmacro SetDword "NoRepair"
  !insertmacro SetString "DisplayVersion" "$RegistrationVersion"
FunctionEnd

Function ${prefix}RemoveRegistration
  StrCpy $OperationFailed 0
  !insertmacro RemoveValue "DisplayName"
  !insertmacro RemoveValue "DisplayIcon"
  !insertmacro RemoveValue "Publisher"
  !insertmacro RemoveValue "QuietUninstallString"
  !insertmacro RemoveValue "NoModify"
  !insertmacro RemoveValue "NoRepair"
  !insertmacro RemoveValue "InstallLocation"
  !insertmacro RemoveValue "UninstallString"
  !insertmacro RemoveValue "DisplayVersion"
  !insertmacro RemoveValue "PRSniperInstaller"
FunctionEnd

Function ${prefix}CloseRegistry
  ${If} $Registry != 0
    System::Call 'advapi32::RegCloseKey(p $Registry)'
    StrCpy $Registry 0
  ${EndIf}
FunctionEnd

Function ${prefix}RemoveEmptyRegistration
  StrCpy $OperationFailed 0
  System::Call 'advapi32::RegQueryInfoKeyW(p $Registry, p 0, p 0, p 0, *i.r1, p 0, p 0, *i.r2, p 0, p 0, p 0, p 0) i.r0'
  ${If} $0 != 0
    StrCpy $OperationFailed 1
    StrCpy $FailureStage "inspect remaining installer container"
    Return
  ${EndIf}
  ${If} $1 != 0
  ${OrIf} $2 != 0
    Return
  ${EndIf}
  Call ${prefix}CloseRegistry
  ClearErrors
  DeleteRegKey /ifempty HKCU "${UNINSTKEY}"
  ; Read back the result instead of treating every /ifempty failure alike.
  ; Reopen with rollback rights while the old files/schema are still available.
  System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${UNINSTKEY}", i 0, i 0x103, *p.r0) i.r1'
  ${If} $1 = 2
    Return
  ${EndIf}
  ${If} $1 = 0
    StrCpy $Registry $0
    System::Call 'advapi32::RegQueryInfoKeyW(p $Registry, p 0, p 0, p 0, *i.r1, p 0, p 0, *i.r2, p 0, p 0, p 0, p 0) i.r0'
    ${If} $0 = 0
      ${If} $1 != 0
      ${OrIf} $2 != 0
        Return
      ${EndIf}
    ${EndIf}
  ${EndIf}
  StrCpy $OperationFailed 1
  StrCpy $FailureStage "remove empty owned installer key"
FunctionEnd

Function ${prefix}Rollback
  StrCpy $OriginalFailure "$FailureStage"
  StrCpy $RollbackFailed 0
  ${If} $RegistryChanged = 1
    ${If} $PreviousVersion == ""
      Call ${prefix}RemoveRegistration
    ${Else}
      StrCpy $RegistrationVersion "$PreviousVersion"
      Call ${prefix}WriteRegistration
    ${EndIf}
    ${If} $OperationFailed = 1
      StrCpy $RollbackFailed 1
      DetailPrint "Rollback failed: $FailureStage"
    ${EndIf}
  ${EndIf}
  !insertmacro RemoveNew "$SMPROGRAMS\${PRODUCTNAME}.lnk" $NewShortcutPlaced
  !insertmacro RemoveNew "$INSTDIR\${MAINBINARYNAME}.exe" $NewAppPlaced
  !insertmacro RemoveNew "$INSTDIR\uninstall.exe" $NewUninstallerPlaced
  !insertmacro RestoreFile "$Transaction\previous-app.exe" "$INSTDIR\${MAINBINARYNAME}.exe" $OldAppMoved
  !insertmacro RestoreFile "$Transaction\previous-uninstall.exe" "$INSTDIR\uninstall.exe" $OldUninstallerMoved
  !insertmacro RestoreFile "$ShortcutBackup" "$SMPROGRAMS\${PRODUCTNAME}.lnk" $OldShortcutMoved
  ${If} $RegistryCreated = 1
  ${AndIf} $RollbackFailed = 0
    Call ${prefix}RemoveEmptyRegistration
    ${If} $OperationFailed = 1
      StrCpy $RollbackFailed 1
      DetailPrint "Rollback failed: $FailureStage"
    ${EndIf}
  ${EndIf}
  Call ${prefix}CloseRegistry
  ${If} $RollbackFailed = 0
    DetailPrint "Original failure: $OriginalFailure"
    Call ${prefix}CleanupTransaction
    DetailPrint "Rolled back: $OriginalFailure"
    !insertmacro Fail "Operation failed ($OriginalFailure). Previous owned state was restored."
  ${EndIf}
  !insertmacro Fail "Operation failed ($OriginalFailure); rollback also failed. Recovery files retained in $Transaction and $ShortcutBackup."
FunctionEnd

Function ${prefix}CleanupTransaction
  ; Only files created/moved by this operation. Never recursively remove a
  ; staging directory, the application directory, or foreign registry contents.
  !insertmacro CleanupFile "$Transaction\new-app.exe"
  !insertmacro CleanupFile "$Transaction\new-uninstall.exe"
  !insertmacro CleanupFile "$Transaction\previous-app.exe"
  !insertmacro CleanupFile "$Transaction\previous-uninstall.exe"
  !insertmacro CleanupFile "$ShortcutStage"
  !insertmacro CleanupFile "$ShortcutBackup"
  ClearErrors
  Delete "$Transaction\recovery.ini"
  RMDir "$Transaction"
  ${If} ${Errors}
    !insertmacro Fail "Owned transaction directory cleanup failed: $Transaction."
  ${EndIf}
FunctionEnd
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

!insertmacro LifecycleFunctions ""
!insertmacro LifecycleFunctions "un."

Function .onInstSuccess
  Call ReleaseLifecycle
FunctionEnd

Function un.onUninstSuccess
  Call un.ReleaseLifecycle
FunctionEnd

Function RollbackRevalidation
  StrCpy $RevalidationFailureHandler 0
  SetOutPath "$INSTDIR"
  Call Rollback
FunctionEnd

Function ValidateInstall
!ifdef PR_SNIPER_TEST_BUILD
  ${If} $RevalidationFailureHandler > 0
    ReadEnvStr $0 PR_SNIPER_NSIS_TEST_FAIL
    ${If} $0 == "install-revalidation"
      !insertmacro Fail "injected post-staging validation refusal"
    ${EndIf}
  ${EndIf}
!endif
  !insertmacro RequireStopped
  ; Distinguish an absent installer key from foreign or unreadable state.
  StrCpy $PreviousVersion ""
  System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${UNINSTKEY}", i 0, i 0x101, *p.r0) i.r1'
  ${If} $1 = 0
    StrCpy $Registry $0
    Call ValidateRegisteredState
    Call CloseRegistry
    nsis_tauri_utils::SemverCompare "${VERSION}" "$PreviousVersion"
    Pop $0
    ${If} $0 != 0
    ${AndIf} $0 != 1
      !insertmacro Fail "Downgrades and unknown installed versions are not supported."
    ${EndIf}
    ${IfNot} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${OrIfNot} ${FileExists} "$INSTDIR\uninstall.exe"
      !insertmacro Fail "Registered application files are incomplete; recover the owned installation first."
    ${EndIf}
  ${ElseIf} $1 = 2
    ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    ${OrIf} ${FileExists} "$INSTDIR\uninstall.exe"
      !insertmacro Fail "An existing application is not owned by this installer."
    ${EndIf}
  ${Else}
    !insertmacro Fail "Cannot inspect existing installer ownership."
  ${EndIf}
  Call InspectShortcut
  ${If} $ShortcutState == "unreadable"
    !insertmacro Fail "Cannot read the Start Menu shortcut; ownership is unknown."
  ${ElseIf} $ShortcutState == "foreign"
    !insertmacro Fail "The Start Menu shortcut belongs to another installation."
  ${EndIf}
FunctionEnd

Function RequireWebView
  ClearErrors
  ReadRegStr $0 HKLM "${WEBVIEW_MACHINE_KEY}" "pv"
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    ClearErrors
    ReadRegStr $0 HKCU "${WEBVIEW_USER_KEY}" "pv"
  ${EndIf}
  ${If} $0 == ""
  ${OrIf} $0 == "0.0.0.0"
    !insertmacro Fail "Install Microsoft Edge WebView2 Evergreen Runtime first, then retry."
  ${EndIf}
  ; A missing machine value is a handled lookup, not an install mutation error.
  ClearErrors
FunctionEnd

Function .onInit
  !insertmacro Context
  Call AcquireLifecycle
  Call ValidateInstall
  Call RequireWebView
FunctionEnd

Section Install
  Call ValidateInstall
  Call RequireWebView
  Call PrepareTransaction
  ClearErrors
  SetOutPath "$Transaction"
  ${If} ${Errors}
    StrCpy $FailureStage "select staging directory"
    Goto install_rollback
  ${EndIf}
  ClearErrors
  File /oname=new-app.exe "{{main_binary_path}}"
  ${If} ${Errors}
    StrCpy $FailureStage "stage application"
    Goto install_rollback
  ${EndIf}
  ClearErrors
  WriteUninstaller "$Transaction\new-uninstall.exe"
  ${If} ${Errors}
    StrCpy $FailureStage "stage uninstaller"
    Goto install_rollback
  ${EndIf}
  ClearErrors
  CreateShortcut "$ShortcutStage" "$INSTDIR\${MAINBINARYNAME}.exe"
  ${If} ${Errors}
    StrCpy $FailureStage "stage shortcut"
    Goto install_rollback
  ${EndIf}
  ; Extraction can take time. Revalidate after it, not just on the welcome page.
  ; Arm cleanup only for our successfully claimed and populated staging paths.
  GetFunctionAddress $RevalidationFailureHandler RollbackRevalidation
  Call ValidateInstall
  StrCpy $RevalidationFailureHandler 0
  ClearErrors
  WriteINIStr "$Transaction\recovery.ini" "schema-v1" "previous-version" "$PreviousVersion"
  ${If} ${Errors}
    StrCpy $FailureStage "refresh prior registration snapshot"
    Goto install_rollback
  ${EndIf}
  ; Reserve write access before touching the current files. All subsequent
  ; registry operations use this handle and check their Win32 return codes.
  System::Call 'advapi32::RegCreateKeyExW(p 0x80000001, w "${UNINSTKEY}", i 0, p 0, i 0, i 0x103, p 0, *p.r0, *i.r1) i.r2'
  ${If} $2 != 0
    StrCpy $FailureStage "open writable installer registration"
    Goto install_rollback
  ${EndIf}
  StrCpy $Registry $0
  ${If} $1 = 1
    StrCpy $RegistryCreated 1
  ${EndIf}
  ${If} $PreviousVersion != ""
    !insertmacro MoveOwned "$INSTDIR\uninstall.exe" "$Transaction\previous-uninstall.exe" $OldUninstallerMoved install_rollback
    !insertmacro MoveOwned "$INSTDIR\${MAINBINARYNAME}.exe" "$Transaction\previous-app.exe" $OldAppMoved install_rollback
  ${EndIf}
  Call InspectShortcut
  ${If} $ShortcutState == "unreadable"
  ${OrIf} $ShortcutState == "foreign"
    StrCpy $FailureStage "revalidate shortcut ownership"
    Goto install_rollback
  ${EndIf}
  ${If} $ShortcutState == "owned"
    !insertmacro MoveOwned "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$ShortcutBackup" $OldShortcutMoved install_rollback
  ${EndIf}
  !insertmacro MoveOwned "$Transaction\new-app.exe" "$INSTDIR\${MAINBINARYNAME}.exe" $NewAppPlaced install_rollback
  !insertmacro MoveOwned "$Transaction\new-uninstall.exe" "$INSTDIR\uninstall.exe" $NewUninstallerPlaced install_rollback
  !insertmacro Fault "install-files" install_rollback
  !insertmacro MoveOwned "$ShortcutStage" "$SMPROGRAMS\${PRODUCTNAME}.lnk" $NewShortcutPlaced install_rollback
  StrCpy $RegistryChanged 1
  StrCpy $RegistrationVersion "${VERSION}"
  Call WriteRegistration
  ${If} $OperationFailed = 1
    Goto install_rollback
  ${EndIf}
  !insertmacro Fault "install-registration" install_rollback
  Call CloseRegistry
  SetOutPath "$INSTDIR"
  Call CleanupTransaction
  SetErrorLevel 0
  Goto install_done
  install_rollback:
    SetOutPath "$INSTDIR"
    Call Rollback
  install_done:
SectionEnd

Function un.ValidateUninstall
  System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${UNINSTKEY}", i 0, i 0x101, *p.r0) i.r1'
  ${If} $1 != 0
    !insertmacro Fail "Cannot read the exact registered installation."
  ${EndIf}
  StrCpy $Registry $0
  Call un.ValidateRegisteredState
  Call un.CloseRegistry
  ${If} $PreviousVersion != "${VERSION}"
    !insertmacro Fail "This uninstaller belongs to a different application version."
  ${EndIf}
  !insertmacro RequireStopped
  Call un.InspectShortcut
  ${If} $ShortcutState == "unreadable"
    !insertmacro Fail "Cannot read the Start Menu shortcut; nothing was removed."
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro Context
  Call un.AcquireLifecycle
  Call un.ValidateUninstall
FunctionEnd

Section Uninstall
  Call un.ValidateUninstall
  Call un.PrepareTransaction
  System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${UNINSTKEY}", i 0, i 0x103, *p.r0) i.r1'
  ${If} $1 != 0
    StrCpy $FailureStage "open removable installer registration"
    Goto uninstall_rollback
  ${EndIf}
  StrCpy $Registry $0
  ; NSIS normally runs a detached copy. _?= runs the original instead, whose
  ; self-deletion belongs to the receipt-checking wrapper after process exit.
  ${If} $EXEPATH != "$INSTDIR\uninstall.exe"
    !insertmacro MoveOwned "$INSTDIR\uninstall.exe" "$Transaction\previous-uninstall.exe" $OldUninstallerMoved uninstall_rollback
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    !insertmacro MoveOwned "$INSTDIR\${MAINBINARYNAME}.exe" "$Transaction\previous-app.exe" $OldAppMoved uninstall_rollback
  ${EndIf}
  Call un.InspectShortcut
  ${If} $ShortcutState == "unreadable"
    StrCpy $FailureStage "revalidate shortcut readability"
    Goto uninstall_rollback
  ${EndIf}
  ${If} $ShortcutState == "owned"
    !insertmacro MoveOwned "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$ShortcutBackup" $OldShortcutMoved uninstall_rollback
  ${EndIf}
  !insertmacro Fault "uninstall-files" uninstall_rollback
  StrCpy $RegistryChanged 1
  Call un.RemoveRegistration
  ${If} $OperationFailed = 1
    Goto uninstall_rollback
  ${EndIf}
  !insertmacro Fault "uninstall-registration" uninstall_rollback
  Call un.RemoveEmptyRegistration
  ${If} $OperationFailed = 1
    Goto uninstall_rollback
  ${EndIf}
  Call un.CloseRegistry
  Call un.CleanupTransaction
  ; Nonempty foreign directories and the _?= self file are intentional.
  RMDir "$INSTDIR"
  SetErrorLevel 0
  Goto uninstall_done
  uninstall_rollback:
    Call un.Rollback
  uninstall_done:
SectionEnd
