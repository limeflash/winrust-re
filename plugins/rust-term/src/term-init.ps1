# Preserve the existing prompt and emit OSC 133 around it. PSReadLine's wrapper emits C
# after the accepted line; without PSReadLine C is emitted at the end of the prompt.
$global:WinRustOriginalPrompt = (Get-Item Function:prompt).ScriptBlock
$global:WinRustFirstPrompt = $true
$global:WinRustHasReadLine = $false
Import-Module PSReadLine -ErrorAction SilentlyContinue
if (Get-Command PSConsoleHostReadLine -ErrorAction SilentlyContinue) {
    $global:WinRustOriginalReadLine = (Get-Item Function:PSConsoleHostReadLine).ScriptBlock
    $global:WinRustHasReadLine = $true
    function global:PSConsoleHostReadLine {
        $line = & $global:WinRustOriginalReadLine
        [Console]::Write("$([char]27)]133;C$([char]7)")
        return $line
    }
}
function global:prompt {
    $success = $?
    $nativeExit = $global:LASTEXITCODE
    $esc = [char]27
    $bel = [char]7
    $out = ''
    if (-not $global:WinRustFirstPrompt) {
        $code = if ($success) { 0 } elseif ($null -ne $nativeExit -and $nativeExit -ne 0) { $nativeExit } else { 1 }
        $out = "$esc]133;D;$code$bel"
    }
    $global:WinRustFirstPrompt = $false
    $cwd = (Get-Location).Path -replace '"', ''
    $original = & $global:WinRustOriginalPrompt
    # the marks travel inside the prompt string (as in Windows Terminal's integration): an empty return value would make
    # the host print its fallback "PS>" after them
    $out += "$esc]9;9;`"$cwd`"$bel$esc]133;A$bel$original$esc]133;B$bel"
    if (-not $global:WinRustHasReadLine) { $out += "$esc]133;C$bel" }
    return $out
}
