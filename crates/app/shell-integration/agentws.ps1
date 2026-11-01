# agent-workspace shell integration for PowerShell, run with `-NoExit -Command`. PowerShell has
# already loaded the user's profile. Marks the prompt with OSC 133 so the app knows where the
# command line starts and when a command runs: A prompt start, B command start, C command
# executed (Enter, with the command line hex-encoded), D command finished (with its exit status
# and the folder, when the location is one). Each mark carries the session's token
# (AGENTWS_MARK_TOKEN), so the app ignores marks printed by programs. Needs PSReadLine, which
# reports Enter. Also defines the `claude` and `copilot` functions (see below). Keep it Windows
# PowerShell 5.1 syntax: no `&&`, `||`, `??` or ternary. Not verified yet.
$global:__agentws_token = $env:AGENTWS_MARK_TOKEN
$global:__agentws_cd_file = $env:AGENTWS_CD_FILE
Remove-Item Env:AGENTWS_MARK_TOKEN -ErrorAction SilentlyContinue
Remove-Item Env:AGENTWS_CD_FILE -ErrorAction SilentlyContinue

if (-not $global:__agentws_loaded -and $global:__agentws_token -and (Get-Module PSReadLine)) {
    $global:__agentws_loaded = $true
    $global:__agentws_prompt = $function:prompt

    function global:prompt {
        # First, before any statement here resets it: whether the last command succeeded. The
        # exit status is 0 on success, else the last program's exit code, else 1 (a cmdlet
        # failed; the code of a program that failed earlier is reported again then, PowerShell
        # keeping $LASTEXITCODE until the next program).
        $ok = $global:?
        # A strict mode from the profile would fail on $LASTEXITCODE before any program ran.
        Set-StrictMode -Off
        $code = $global:LASTEXITCODE
        $exit = 0
        if (-not $ok) {
            $exit = 1
            if ($code -is [int] -and $code -ne 0) {
                $exit = $code
            }
        }
        $esc = [char]27
        $bel = [char]7
        $mark = ";aw=$global:__agentws_token$bel"
        # Set-Location does not change the process's folder, so the app cannot read it there.
        $cwd = ''
        if ($PWD.Provider.Name -eq 'FileSystem') {
            $bytes = [System.Text.Encoding]::UTF8.GetBytes($PWD.ProviderPath)
            $cwd = ';cwd=' + [System.BitConverter]::ToString($bytes).Replace('-', '')
        }
        # The user's prompt sees the last command's $? again (as VS Code's integration does).
        if (-not $ok) {
            Write-Error 'failure' -ErrorAction Ignore
        }
        $text = & $global:__agentws_prompt
        $finished = "$esc]133;D;aw=$global:__agentws_token;exit=$exit$cwd$bel"
        "$finished$esc]133;A$mark$text$esc]133;B$mark"
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

    # `claude` and `copilot` run in place with the tab's status hooks, as in bash and zsh
    # (agentws-agents.sh). Each becomes a function (a user function of that name, from the
    # profile, is kept, and so is an alias to anything but the agent program; an alias to the
    # agent program is replaced, and its target run) that runs the program on PATH, the
    # first `Get-Command` finds (`claude.exe`, npm's `claude.ps1` or `claude.cmd`); typing one
    # of those names, or `& (Get-Command claude -CommandType Application)`, runs it unchanged.
    # An interactive run prints the agent's N mark, runs the program with its hooks put before
    # the user's arguments, then prints the end-of-agent mark X with its exit status, which
    # stays in $LASTEXITCODE. Everything else runs unchanged: no hooks in the tab's
    # environment, input piped in or output piped on (pipeline input is passed through), the
    # console redirected, and one-shot runs (print/prompt, version, help, a subcommand). The
    # agent not on PATH gets PowerShell's own "not recognized" error.

    # Whether stdin and stdout are the console (a function, so a test can stand in for one).
    function global:__agentws_on_terminal {
        -not [Console]::IsInputRedirected -and -not [Console]::IsOutputRedirected
    }

    # Whether $Agent with $AgentArgs starts interactively: no subcommand first and no option
    # for a one-shot run, the version or the help (before `--`), and for Claude no
    # `--settings` of the user's own (it takes one value, so ours would be dropped).
    function global:__agentws_interactive([string]$Agent, [object[]]$AgentArgs) {
        $subcommands = @{
            claude  = @('agents', 'attach', 'auth', 'auto-mode', 'config', 'doctor', 'gateway',
                'import', 'install', 'logs', 'mcp', 'migrate-installer', 'plugin', 'plugins',
                'purge', 'respawn', 'rm', 'setup-token', 'stop', 'kill', 'ultrareview',
                'update', 'upgrade')
            copilot = @('app', 'login', 'logout', 'help', 'init', 'config', 'update', 'version',
                'workflow', 'sessions', 'memories', 'plugin', 'mcp', 'skill', 'instruction',
                'lsp', 'sandbox', 'completion')
        }
        if ($AgentArgs.Count -gt 0 -and $subcommands[$Agent] -ccontains "$($AgentArgs[0])") {
            return $false
        }
        foreach ($arg in $AgentArgs) {
            $text = "$arg"
            if ($text -ceq '--') {
                break
            }
            if ($Agent -ceq 'claude' -and $text -cmatch '(?s)^--settings(=.*)?$') {
                return $false
            }
            if ($text -cmatch '(?s)^(-p|--print(=.*)?|--prompt(=.*)?|-v|--version|-h|--help)$') {
                return $false
            }
        }
        $true
    }

    # The program each agent function runs: the name looked up on PATH, or the target of the
    # user's alias for the agent (below).
    $global:__agentws_programs = @{ claude = 'claude'; copilot = 'copilot' }

    # A user alias $Agent whose target is the agent program itself (its name, with .exe, .cmd
    # or .ps1, or a path to one) is replaced by the agent function, which runs that target
    # (aliases outrank functions, so it is removed). Any other alias is kept. Remove-Alias
    # needs PowerShell 6, hence Remove-Item.
    function global:__agentws_take_alias([string]$Agent) {
        $alias = Get-Alias -Name $Agent -ErrorAction Ignore
        if (-not $alias) {
            return
        }
        $target = $alias.Definition
        if ((Split-Path -Leaf $target) -notmatch "^$Agent(\.(exe|cmd|ps1))?$") {
            return
        }
        Remove-Item -LiteralPath "Alias:$Agent" -Force
        $global:__agentws_programs[$Agent] = $target
    }

    # Runs $Agent with $AgentArgs, in place with its hooks ($Flag $Hooks) when it starts
    # interactively. $Invocation and $PipeInput are the calling function's.
    function global:__agentws_run_agent($Agent, $Hooks, $Flag, $Invocation, $PipeInput,
        $AgentArgs) {
        Set-StrictMode -Off
        $name = $global:__agentws_programs[$Agent]
        $program = Get-Command -Name $name -CommandType Application, ExternalScript `
            -TotalCount 1 -ErrorAction SilentlyContinue
        if (-not $program) {
            Get-Command -Name $name -CommandType Application, ExternalScript
            return
        }
        $piped = $Invocation.ExpectingInput -or
            $Invocation.PipelinePosition -lt $Invocation.PipelineLength
        if (-not $Hooks -or $piped -or -not (__agentws_on_terminal) -or
            -not (__agentws_interactive $Agent $AgentArgs)) {
            if ($Invocation.ExpectingInput) {
                $PipeInput | & $program @AgentArgs
            } else {
                & $program @AgentArgs
            }
            return
        }
        [Console]::Write("$([char]27)]133;N;aw=$global:__agentws_token;agent=$Agent$([char]7)")
        # Copilot CLI only reports progress (OSC 9;4) to a terminal it recognizes.
        $conEmu = $env:ConEmuANSI
        try {
            if ($Agent -eq 'copilot') {
                $env:ConEmuANSI = 'ON'
            }
            # A script that doesn't `exit` leaves the previous program's code otherwise.
            $global:LASTEXITCODE = 0
            & $program $Flag $Hooks @AgentArgs
        } finally {
            # Also after Ctrl+C stopped the command line, which then prints no X: its D ends
            # the run.
            if ($Agent -eq 'copilot') {
                $env:ConEmuANSI = $conEmu
            }
        }
        $code = $global:LASTEXITCODE
        [Console]::Write("$([char]27)]133;X;aw=$global:__agentws_token;exit=$code$([char]7)")
    }

    __agentws_take_alias 'claude'
    if (-not (Get-Command -Name claude -CommandType Alias, Function -ErrorAction Ignore)) {
        function global:claude {
            __agentws_run_agent 'claude' $env:AGENTWS_CLAUDE_SETTINGS '--settings' `
                $MyInvocation $input $args
        }
    }
    __agentws_take_alias 'copilot'
    if (-not (Get-Command -Name copilot -CommandType Alias, Function -ErrorAction Ignore)) {
        function global:copilot {
            __agentws_run_agent 'copilot' $env:AGENTWS_COPILOT_PLUGIN '--plugin-dir' `
                $MyInvocation $input $args
        }
    }
}
