# agent-workspace: `claude` and `copilot` typed at a bash or zsh prompt run in place with the
# tab's status hooks. Sourced by agentws.bash and the zsh .zshenv once the user's startup files
# ran and only with a mark token. Each agent becomes an alias of a function (a user alias that
# starts with the agent, `alias claude='claude --model opus'`, gets the function in its place;
# another user alias, or a user function, is kept), so `\claude` and `command claude` run the
# program unchanged. An interactive run prints the agent's N mark, runs the program with its
# hooks put before the user's arguments, then prints the end-of-agent mark X with its exit
# status, which the function returns. Everything else runs unchanged: no hooks in the tab's
# environment, stdin or stdout not a terminal, the agent not on PATH (the shell says so), and
# one-shot runs (print/prompt, version, help, a subcommand).

# Whether $1 is a program on PATH (not a function or an alias).
__agentws_on_path() {
    if [ -n "${ZSH_VERSION-}" ]; then
        whence -p -- "$1" >/dev/null 2>&1
    else
        type -P -- "$1" >/dev/null 2>&1
    fi
}

# Whether agent $1 with the arguments after it starts interactively: no subcommand first and
# no option for a one-shot run, the version or the help (before `--`), and for Claude no
# `--settings` of the user's own (it takes one value, so ours would be dropped).
__agentws_interactive() {
    local agent=$1 arg
    shift
    case "$agent:${1-}" in
        claude:agents | claude:attach | claude:auth | claude:auto-mode | claude:config | \
            claude:doctor | claude:gateway | claude:import | claude:install | claude:logs | \
            claude:mcp | claude:migrate-installer | claude:plugin | claude:plugins | \
            claude:purge | claude:respawn | claude:rm | claude:setup-token | claude:stop | \
            claude:kill | claude:ultrareview | claude:update | claude:upgrade)
            return 1
            ;;
        copilot:app | copilot:login | copilot:logout | copilot:help | copilot:init | \
            copilot:config | copilot:update | copilot:version | copilot:workflow | \
            copilot:sessions | copilot:memories | copilot:plugin | copilot:mcp | \
            copilot:skill | copilot:instruction | copilot:lsp | copilot:sandbox | \
            copilot:completion)
            return 1
            ;;
    esac
    for arg in "$@"; do
        case "$arg" in
            --) break ;;
            --settings | --settings=*)
                [ "$agent" = claude ] && return 1
                ;;
            -p | --print | --print=* | --prompt | --prompt=* | -v | --version | -h | --help)
                return 1
                ;;
        esac
    done
    return 0
}

# Run agent $1 with the arguments after $3, in place with its hooks ($3 $2) when it starts
# interactively, and return its exit status.
__agentws_run_agent() {
    if [ -n "${ZSH_VERSION-}" ]; then
        emulate -L zsh
    fi
    local agent=$1 hooks=$2 flag=$3 code
    shift 3
    if [ -z "$hooks" ] || [ ! -t 0 ] || [ ! -t 1 ] || ! __agentws_on_path "$agent" ||
        ! __agentws_interactive "$agent" "$@"; then
        command "$agent" "$@"
        return
    fi
    printf '\033]133;N;aw=%s;agent=%s\007' "$__agentws_token" "$agent"
    if [ "$agent" = copilot ]; then
        # Copilot CLI only reports progress (OSC 9;4) to a terminal it recognizes.
        ConEmuANSI=ON command "$agent" "$flag" "$hooks" "$@"
    else
        command "$agent" "$flag" "$hooks" "$@"
    fi
    code=$?
    printf '\033]133;X;aw=%s;exit=%s\007' "$__agentws_token" "$code"
    return "$code"
}

__agentws_claude() {
    __agentws_run_agent claude "${AGENTWS_CLAUDE_SETTINGS-}" --settings "$@"
}

__agentws_copilot() {
    __agentws_run_agent copilot "${AGENTWS_COPILOT_PLUGIN-}" --plugin-dir "$@"
}

# Gives `$1` (claude, copilot) the tab's hooks: as an alias of its function, or by putting
# that function in place of the agent at the start of the user's own alias
# (`alias claude='claude --model opus'`). A user function, or an alias that runs something
# else first, is kept as it is.
__agentws_wrap() {
    local agent=$1 expansion
    if typeset -f "$agent" >/dev/null 2>&1; then
        return
    fi
    if ! alias "$agent" >/dev/null 2>&1; then
        alias "$agent=__agentws_$agent"
        return
    fi
    if [ -n "${ZSH_VERSION-}" ]; then
        expansion=${aliases[$agent]-}
    else
        expansion=${BASH_ALIASES[$agent]-}
    fi
    case "$expansion" in
        "$agent" | "command $agent") expansion=__agentws_$agent ;;
        "$agent "*) expansion="__agentws_$agent ${expansion#"$agent "}" ;;
        "command $agent "*) expansion="__agentws_$agent ${expansion#"command $agent "}" ;;
        *) return ;;
    esac
    alias "$agent=$expansion"
}
__agentws_wrap claude
__agentws_wrap copilot
unset -f __agentws_wrap
