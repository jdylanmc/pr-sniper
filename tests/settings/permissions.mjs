import { execFile } from "node:child_process";
import { chmod } from "node:fs/promises";
import { promisify } from "node:util";

const execute = promisify(execFile);
const script = `
$ErrorActionPreference = 'Stop'
$path = $env:PR_SNIPER_READ_FIXTURE
$acl = [System.IO.File]::GetAccessControl($path)
if ($env:PR_SNIPER_RESTORE_ACL) {
  $acl.SetSecurityDescriptorSddlForm($env:PR_SNIPER_RESTORE_ACL, 'Access')
} else {
  $acl.GetSecurityDescriptorSddlForm('Access')
  $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
  $rule = [System.Security.AccessControl.FileSystemAccessRule]::new($sid, 'ReadData', 'Deny')
  $acl.AddAccessRule($rule)
}
[System.IO.File]::SetAccessControl($path, $acl)
`;

export async function denyRead(path) {
  if (process.platform !== "win32") {
    await chmod(path, 0o000);
    return () => chmod(path, 0o600);
  }
  const env = { ...process.env, PR_SNIPER_READ_FIXTURE: path };
  const { stdout } = await execute(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", script],
    { env },
  );
  return () =>
    execute(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", script],
      {
        env: { ...env, PR_SNIPER_RESTORE_ACL: stdout.trim() },
      },
    );
}
