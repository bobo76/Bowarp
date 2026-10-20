# agent-workspace shell integration for PowerShell, run with `-NoExit -Command`. PowerShell has
# already loaded the user's profile. Marks the prompt with OSC 133 so the app knows where the
# command line starts and when a command runs: A prompt start, B command start, C command
# executed (Enter, with the command line hex-encoded), D command finished (with the folder, when
# the location is one). Each mark carries the session's token (AGENTWS_MARK_TOKEN), so the app
# ignores marks printed by programs. Needs PSReadLine, which reports Enter. Not verified yet.
$global:__agentws_token = $env:AGENTWS_MARK_TOKEN
$global:__agentws_cd_file = $env:AGENTWS_CD_FILE
Remove-Item Env:AGENTWS_MARK_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:AGENTWS_CD_FILE -ErrorAction SilentlyContinue

if (-not $global:__agentws_loaded -and $global:__agentws_token -and (Get-Module PSReadLine)) {
    $global:__agentws_loaded = $true
    $global:__agentws_prompt = $function:prompt

    function global:prompt {
        $esc = [char]27
        $bel = [char]7
        $mark = ";aw=$global:__agentws_token$bel"
        # Set-Location does not change the process's folder, so the app cannot read it there.
        $cwd = ''
        if ($PWD.Provider.Name -eq 'FileSystem') {
            $bytes = [System.Text.Encoding]::UTF8.GetBytes($PWD.ProviderPath)
            $cwd = ';cwd=' + [System.BitConverter]::ToString($bytes).Replace('-', '')
        }
        $text = & $global:__agentws_prompt
        "$esc]133;D;aw=$global:__agentws_token$cwd$bel$esc]133;A$mark$text$esc]133;B$mark"
    }

    Set-PSReadLineKeyHandler -Chord Enter -ScriptBlock {
        $line = $null
        $cursor = $null
        [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
        $command = ''
        if ($line.Length -le 4096) {
            $bytes = [System.Text.Encoding]::UTF8.GetBytes($line)
            $hex = -join ($bytes | ForEach-Object { $_.ToString('x2') })
            $command = ";cmd=$hex"
        }
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
        [Console]::Write("$([char]27)]133;C;aw=$global:__agentws_token$command$([char]7)")
    }

    # Sent by the app to change folder without typing a command: go to the folder it wrote to
    # the file (read as text, never run), then redraw the prompt, keeping the line being typed.
    if ($global:__agentws_cd_file) {
        Set-PSReadLineKeyHandler -Chord 'Ctrl+Alt+Shift+F12' -ScriptBlock {
            $file = $global:__agentws_cd_file
            if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
                return
            }
            $dir = [System.IO.File]::ReadAllText($file)
            Remove-Item -LiteralPath $file -ErrorAction SilentlyContinue
            Set-Location -LiteralPath $dir -ErrorAction SilentlyContinue
            [Microsoft.PowerShell.PSConsoleReadLine]::InvokePrompt()
        }
    }
}
