#Requires -Version 7.4
#Requires -RunAsAdministrator

[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$InstallerPath,
  [Parameter(Mandatory = $true)]
  [ValidatePattern('^[0-9a-f]{40}$')]
  [string]$Commit,
  [string]$EvidencePath,
  [ValidateSet('Any', 'Valid', 'NotSigned')]
  [string]$ExpectedAuthenticodeStatus = 'Any',
  [ValidateRange(1, 60)]
  [int]$LaunchSeconds = 5
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $IsWindows) { throw 'the Windows clean-user verifier must run on Windows' }
$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) { throw "installer is not a file: $installer" }
if ($ExpectedAuthenticodeStatus -ne 'Any' -and (Get-AuthenticodeSignature -LiteralPath $installer).Status.ToString() -ne $ExpectedAuthenticodeStatus) {
  throw "installer Authenticode status does not match $ExpectedAuthenticodeStatus"
}

$suffix = [Guid]::NewGuid().ToString('N').Substring(0, 8)
$username = "ssci_$suffix"
$passwordText = "Ss!$([Guid]::NewGuid().ToString('N'))9z"
$securePassword = ConvertTo-SecureString $passwordText -AsPlainText -Force
$credential = [System.Management.Automation.PSCredential]::new("$env:COMPUTERNAME\$username", $securePassword)
$user = $null
$profile = $null
$applicationProcess = $null

try {
  $user = New-LocalUser -Name $username -Password $securePassword -AccountNeverExpires -PasswordNeverExpires -Description 'Ephemeral SimShredder clean-user verification account'
  $usersMemberSids = @(Get-LocalGroupMember -SID 'S-1-5-32-545' | ForEach-Object { $_.SID.Value })
  if ($usersMemberSids -notcontains $user.SID.Value) { Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $user }
  $usersMemberSids = @(Get-LocalGroupMember -SID 'S-1-5-32-545' | ForEach-Object { $_.SID.Value })
  if ($usersMemberSids -notcontains $user.SID.Value) { throw 'ephemeral verification account is not a member of Users' }
  $administratorSids = @(Get-LocalGroupMember -SID 'S-1-5-32-544' | ForEach-Object { $_.SID.Value })
  if ($administratorSids -contains $user.SID.Value) { throw 'ephemeral verification account unexpectedly belongs to Administrators' }

  $profileBootstrap = Start-Process -FilePath "$env:SystemRoot\System32\cmd.exe" -ArgumentList @('/d', '/c', 'exit 0') -Credential $credential -LoadUserProfile -WorkingDirectory "$env:SystemRoot\System32" -Wait -PassThru
  if ($profileBootstrap.ExitCode -ne 0) { throw "failed to initialize the standard-user profile: $($profileBootstrap.ExitCode)" }
  $profile = Get-CimInstance Win32_UserProfile -Filter "SID = '$($user.SID.Value)'"
  if ($null -eq $profile -or [string]::IsNullOrWhiteSpace($profile.LocalPath)) { throw 'standard-user profile path was not created' }
  $tokenGroupsPath = Join-Path $profile.LocalPath 'simshredder-token-groups.csv'
  $tokenProbe = Start-Process -FilePath "$env:SystemRoot\System32\whoami.exe" -ArgumentList @('/groups', '/fo', 'csv', '/nh') -Credential $credential -LoadUserProfile -WorkingDirectory $profile.LocalPath -RedirectStandardOutput $tokenGroupsPath -Wait -PassThru
  if ($tokenProbe.ExitCode -ne 0) { throw "failed to inspect the standard-user token: $($tokenProbe.ExitCode)" }
  $tokenGroups = Get-Content -LiteralPath $tokenGroupsPath -Raw
  if ($tokenGroups -match 'S-1-5-32-544' -or $tokenGroups -match 'S-1-16-12288') { throw 'ephemeral verification process has an administrator or high-integrity token' }
  if ($tokenGroups -notmatch 'S-1-16-8192') { throw 'ephemeral verification process does not have the expected medium-integrity token' }
  $installRoot = Join-Path $profile.LocalPath 'AppData\Local\Programs\SimShredder'

  $install = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$installRoot") -Credential $credential -LoadUserProfile -WorkingDirectory $profile.LocalPath -Wait -PassThru
  if ($install.ExitCode -ne 0) { throw "standard-user installer failed: $($install.ExitCode)" }
  $installedApp = Join-Path $installRoot 'simshredder-desktop.exe'
  if (-not (Test-Path -LiteralPath $installedApp -PathType Leaf)) { throw 'standard-user installed application is missing' }
  if ($ExpectedAuthenticodeStatus -ne 'Any' -and (Get-AuthenticodeSignature -LiteralPath $installedApp).Status.ToString() -ne $ExpectedAuthenticodeStatus) {
    throw "installed application Authenticode status does not match $ExpectedAuthenticodeStatus"
  }
  foreach ($license in @('LICENSE', 'NOTICE', 'PRIVACY.md', 'THIRD_PARTY_NOTICES.md', 'rust-third-party-licenses.md', 'node-third-party-licenses.md')) {
    if (-not (Test-Path -LiteralPath (Join-Path $installRoot "licenses/$license") -PathType Leaf)) { throw "standard-user installed license resource is missing: $license" }
  }

  $applicationProcess = Start-Process -FilePath $installedApp -Credential $credential -LoadUserProfile -WorkingDirectory $installRoot -PassThru
  Start-Sleep -Seconds $LaunchSeconds
  if ($applicationProcess.HasExited) { throw "standard-user installed GUI exited during launch: $($applicationProcess.ExitCode)" }
  Stop-Process -Id $applicationProcess.Id -Force
  $applicationProcess.WaitForExit()
  $applicationProcess = $null

  $uninstaller = Join-Path $installRoot 'uninstall.exe'
  if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) { throw 'standard-user uninstaller is missing' }
  $uninstall = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Credential $credential -LoadUserProfile -WorkingDirectory $installRoot -Wait -PassThru
  if ($uninstall.ExitCode -ne 0 -or (Test-Path -LiteralPath $installedApp)) { throw 'standard-user uninstall failed' }

  $evidence = [ordered]@{
    schema = 1
    platform = 'windows-x64'
    commit = $Commit
    users_member = $true
    administrators_member = $false
    token_integrity = 'medium'
    installer_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash.ToLowerInvariant()
    authenticode_status = (Get-AuthenticodeSignature -LiteralPath $installer).Status.ToString()
    install_root = '%USERPROFILE%\\AppData\\Local\\Programs\\SimShredder'
    launch_seconds = $LaunchSeconds
    install_exit_code = $install.ExitCode
    uninstall_exit_code = $uninstall.ExitCode
  }
  $json = $evidence | ConvertTo-Json -Depth 3
  if ($EvidencePath) {
    $evidenceParent = Split-Path -Parent $EvidencePath
    if ($evidenceParent) { New-Item -ItemType Directory -Path $evidenceParent -Force | Out-Null }
    Set-Content -LiteralPath $EvidencePath -Value $json -Encoding utf8NoBOM
  }
  $json
}
finally {
  $cleanupErrors = [System.Collections.Generic.List[string]]::new()
  if ($null -ne $applicationProcess -and -not $applicationProcess.HasExited) { Stop-Process -Id $applicationProcess.Id -Force -ErrorAction SilentlyContinue }
  if ($null -ne $profile) {
    try {
      for ($attempt = 0; $attempt -lt 10; $attempt += 1) {
        $currentProfile = Get-CimInstance Win32_UserProfile -Filter "SID = '$($user.SID.Value)'"
        if ($null -eq $currentProfile -or -not $currentProfile.Loaded) { break }
        Start-Sleep -Milliseconds 500
      }
      $currentProfile = Get-CimInstance Win32_UserProfile -Filter "SID = '$($user.SID.Value)'"
      if ($null -ne $currentProfile) {
        if ($currentProfile.Special -or $currentProfile.SID -ne $user.SID.Value) { throw 'refusing to remove an unexpected Windows profile' }
        $currentProfile | Remove-CimInstance
      }
    }
    catch {
      $cleanupErrors.Add("profile cleanup failed: $($_.Exception.Message)")
    }
  }
  if ($null -ne $user) {
    try {
      Remove-LocalUser -SID $user.SID
      if (@(Get-LocalUser | Where-Object { $_.SID.Value -eq $user.SID.Value }).Count -ne 0) { throw 'ephemeral verification account still exists' }
    }
    catch {
      $cleanupErrors.Add("account cleanup failed: $($_.Exception.Message)")
    }
  }
  $passwordText = $null
  $securePassword = $null
  $credential = $null
  if ($cleanupErrors.Count -ne 0) { throw ($cleanupErrors -join '; ') }
}
