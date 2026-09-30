export const isWindows = navigator.userAgent.includes("Windows");
export const platformName = isWindows ? "Windows" : "macOS";
export const trayLocation = isWindows ? "system tray" : "menu bar";
export const trayAdjective = isWindows ? "system-tray" : "menu-bar";
export const secureStoreName = isWindows
  ? "Windows Credential Manager"
  : "macOS Keychain";
export const startupSettingsName = isWindows
  ? "Windows Startup Apps"
  : "macOS Login Items";
