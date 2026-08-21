# Regression test for the installer's PATH editing.
#
# History: an earlier build appended to PATH from NSIS directly. NSIS strings
# are capped at NSIS_MAX_STRLEN (1024 in stock builds), so on a machine with a
# longer PATH `ReadRegStr` returned a truncated/empty value and the
# read-append-write cycle replaced the user's entire PATH with it. PATH edits
# now go through a PowerShell helper instead.
#
# This test does not re-implement that helper: it extracts the real script text
# out of the WriteDshPathPs1 macro in installer.nsi, so the logic under test is
# exactly the logic that ships. Only the registry location is redirected, to a
# throwaway key that is removed afterwards -- the real PATH is never touched.
#
# Usage:  pwsh -File scripts/test-dsh-path.ps1
param([string]$NsiPath)
$ErrorActionPreference = 'Stop'

$nsi = if ($NsiPath) { $NsiPath } else { Join-Path $PSScriptRoot "..\shell\src-tauri\nsis\installer.nsi" }
if (-not (Test-Path $nsi)) { throw "installer.nsi not found at $nsi" }
$lines = Get-Content $nsi

# Pull the FileWrite payloads out of the macro and undo the NSIS escaping.
$start = ($lines | Select-String -Pattern '^\s*!macro\s+WriteDshPathPs1' | Select-Object -First 1)
if (-not $start) { throw "WriteDshPathPs1 macro not found -- did the macro get renamed?" }
$body = @()
for ($i = $start.LineNumber; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '^\s*!macroend') { break }
    if ($lines[$i] -match '^\s*FileWrite\s+\$0\s+"(.*)"\s*$') {
        $t = $matches[1]
        $t = $t -replace '\$\\r\$\\n', ''      # trailing newline marker
        $t = $t -replace '\$\\"', '"'          # escaped quote
        $t = $t -replace '\$\$', '$'           # escaped dollar
        $body += $t
    }
}
if ($body.Count -lt 10) { throw "extracted only $($body.Count) lines from the macro; extraction is broken" }
$script = $body -join "`r`n"
if ($script -notmatch 'DoNotExpandEnvironmentNames') { throw "extracted helper lacks DoNotExpandEnvironmentNames" }
if ($script -notmatch 'ExpandString') { throw "extracted helper lacks ExpandString write-back" }

# Redirect both scopes at the throwaway key so the real PATH is never written.
$testSub = 'Software\DshPathTest'
$script = $script -replace "'SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment'", "'$testSub'"
$script = $script -replace "'Environment'", "'$testSub'"
if ($script -notmatch [regex]::Escape($testSub)) { throw "failed to redirect the helper to the test key" }

$helper = Join-Path ([System.IO.Path]::GetTempPath()) "dsh-path-under-test.ps1"
Set-Content -Path $helper -Value $script -Encoding ascii

function Invoke-Helper {
    param([string]$Dir, [string]$Action)
    & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $helper -Dir $Dir -Scope User -Action $Action | Out-Null
    return $LASTEXITCODE
}
function Get-TestPath {
    $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($testSub, $false)
    $v = [string]$k.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $kind = $k.GetValueKind('Path'); $k.Close()
    return @{ Value = $v; Kind = $kind }
}

# A realistic PATH: unexpanded vars, and long enough to trip the NSIS cap.
$entries = @('%SystemRoot%\system32', '%SystemRoot%', '%SystemRoot%\System32\Wbem',
             '%USERPROFILE%\AppData\Local\Microsoft\WindowsApps')
1..40 | ForEach-Object { $entries += "C:\Program Files\SomeVendor\PaddingDirectoryNumber$_\bin" }
$original = $entries -join ';'
$dir = 'C:\Users\test\AppData\Local\DeepSeek Harness\bin'

New-Item -Path "HKCU:\$testSub" -Force | Out-Null
try {
    $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($testSub, $true)
    $k.SetValue('Path', $original, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    $k.Close()

    Write-Host ("helper extracted: {0} lines" -f $body.Count)
    Write-Host ("test PATH       : {0} chars, {1} entries (NSIS cap 1024 exceeded: {2})" -f `
        $original.Length, $entries.Count, ($original.Length -gt 1024))
    Write-Host ""

    $fail = 0
    function Check([bool]$ok, [string]$msg) {
        if ($ok) { Write-Host "PASS $msg" } else { Write-Host "FAIL $msg"; $script:fail++ }
    }

    Check ((Invoke-Helper -Dir $dir -Action 'Add') -eq 0) "Add exits 0"
    $r = Get-TestPath
    Check ($r.Value -eq "$original;$dir")                  "append preserved every original entry"
    Check ($r.Value -like '*%SystemRoot%\system32*')       "%SystemRoot% left unexpanded"
    Check ($r.Kind -eq [Microsoft.Win32.RegistryValueKind]::ExpandString) "value kind still REG_EXPAND_SZ"

    $before = (Get-TestPath).Value
    Invoke-Helper -Dir $dir -Action 'Add' | Out-Null
    Check ((Get-TestPath).Value -eq $before)               "re-install does not duplicate the entry"

    Check ((Invoke-Helper -Dir $dir -Action 'Remove') -eq 0) "Remove exits 0"
    Check ((Get-TestPath).Value -eq $original)             "uninstall restores PATH exactly"

    Invoke-Helper -Dir 'C:\not\on\path' -Action 'Remove' | Out-Null
    Check ((Get-TestPath).Value -eq $original)             "removing an absent entry is a no-op"
}
finally {
    Remove-Item -Path "HKCU:\$testSub" -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -Path $helper -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($fail -eq 0) { Write-Host "ALL CHECKS PASSED" } else { Write-Host "$fail CHECK(S) FAILED"; exit 1 }
