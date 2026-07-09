; PetPhrase NSIS 安装脚本 —— 每用户安装,含标准卸载器(uninstall.exe + 控制面板卸载入口)
Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!define APP_NAME "PetPhrase"
!define APP_VERSION "0.6.1"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"

Name "${APP_NAME}"
OutFile "target\PetPhrase_${APP_VERSION}_x64-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
; 覆盖安装时默认回到上次安装目录(读卸载注册表 InstallLocation)
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma

!define MUI_ABORTWARNING
!define MUI_ICON "assets\icon.ico"
!define MUI_UNICON "assets\icon.ico"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "SimpChinese"

Section "Install"
  ; 关闭正在运行的实例;Sleep 给进程对象销毁留余量,防 exe 文件锁未释放导致覆盖失败
  ; (一键更新路径:应用先起本安装器再自退,taskkill 时进程可能仍在收尾)
  nsExec::Exec 'taskkill /F /IM PetPhrase.exe'
  Sleep 400

  SetOutPath "$INSTDIR"
  File "target\release\PetPhrase.exe"
  SetOutPath "$INSTDIR\pets\default"
  File "pets\default\pet.json"
  File "pets\default\spritesheet.png"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; 快捷方式
  CreateShortcut "$DESKTOP\${APP_NAME}.lnk" "$INSTDIR\PetPhrase.exe"
  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\PetPhrase.exe"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\卸载 ${APP_NAME}.lnk" "$INSTDIR\uninstall.exe"

  ; 控制面板「应用和功能」卸载入口
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "PetPhrase"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\PetPhrase.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ; 估算大小(KB)
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"

  ; 静默安装(一键更新 / /S 手动)装完自启新版
  IfSilent 0 +2
    Exec '"$INSTDIR\PetPhrase.exe"'
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /F /IM PetPhrase.exe'

  Delete "$INSTDIR\PetPhrase.exe"
  RMDir /r "$INSTDIR\pets"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  Delete "$DESKTOP\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\卸载 ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"

  ; 清理开机自启与卸载注册表项
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${APP_NAME}"
  DeleteRegKey HKCU "${UNINST_KEY}"

  ; 用户数据按需删除;静默卸载一律保留
  IfSilent skip_data
  MessageBox MB_YESNO|MB_DEFBUTTON2 "是否同时删除常用语数据?$\n(%APPDATA%\PetPhrase)" IDNO skip_data
    RMDir /r "$APPDATA\PetPhrase"
  skip_data:
SectionEnd
