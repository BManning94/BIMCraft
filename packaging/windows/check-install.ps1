# Installs the MSI into a folder of the user's choosing, as the setup wizard's folder page does, then upgrades it with a
# second build of the same source, as an update does, and checks the folder is kept; then removes it. Needs an
# administrator (the installer is for every user), and installs for real: use a CI runner or a throwaway machine.
#
#   packaging\windows\check-install.ps1 -Msi bimcraft-0.7.0-windows-x64.msi -Upgrade rebuilt.msi
#
# A rebuild of the same version has a product code of its own and the same upgrade code, so Windows Installer takes it for
# an upgrade (the installer allows same-version upgrades).
param(
  [Parameter(Mandatory)][string]$Msi,
  [Parameter(Mandatory)][string]$Upgrade
)
$ErrorActionPreference = 'Stop'
$msiPath = (Resolve-Path $Msi).Path
$upgradePath = (Resolve-Path $Upgrade).Path
$work = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
# Not Program Files, and with a space in the name, as a folder somebody types would have.
$dir = Join-Path $work 'BIMCraft elsewhere'
$programFiles = Join-Path $env:ProgramFiles 'BIMCraft'
$log = Join-Path $work 'check-install.log'

function Run-Msiexec([string]$what) {
  # 3010 is "done, restart needed", which nothing here needs but isn't a failure.
  $p = Start-Process msiexec.exe -ArgumentList "$what /qn /l*v `"$log`"" -Wait -PassThru
  if ($p.ExitCode -notin 0, 3010) { throw "msiexec $what exited with $($p.ExitCode)" }
}
function Expect([bool]$ok, [string]$what) { if (-not $ok) { throw "expected: $what" } }

try {
  Run-Msiexec "/i `"$msiPath`" INSTALLFOLDER=`"$dir`""
  Expect (Test-Path "$dir\bimcraft.exe") 'bimcraft.exe in the folder that was chosen'
  Expect (Test-Path "$dir\bimcraft-cli.exe") 'bimcraft-cli.exe in the folder that was chosen'
  Expect (-not (Test-Path $programFiles)) 'nothing in Program Files'
  $appPath = (Get-Item 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\bimcraft.exe').GetValue('')
  Expect ($appPath -eq "$dir\bimcraft.exe") "App Paths names the chosen folder, not '$appPath'"

  # No folder given this time: the upgrade has to find where BIMCraft is.
  Run-Msiexec "/i `"$upgradePath`""
  Expect (Test-Path "$dir\bimcraft.exe") 'the upgrade kept the chosen folder'
  Expect (-not (Test-Path $programFiles)) 'the upgrade did not move BIMCraft to Program Files'

  Run-Msiexec "/x `"$upgradePath`""
  Expect (-not (Test-Path "$dir\bimcraft.exe")) 'the uninstall removed the program'
  Expect (-not (Test-Path 'HKLM:\SOFTWARE\BIMCraft')) 'the uninstall removed the remembered folder'
  Write-Host "ok: installed to '$dir', kept it through an upgrade, removed it"
} catch {
  Write-Host "msiexec log, last lines:"
  if (Test-Path $log) { Get-Content $log -Tail 60 }
  # Leave nothing installed behind a failure.
  foreach ($m in $upgradePath, $msiPath) { Start-Process msiexec.exe -ArgumentList "/x `"$m`" /qn" -Wait }
  throw
}
