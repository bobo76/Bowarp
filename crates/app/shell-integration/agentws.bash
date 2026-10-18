# agent-workspace shell integration for bash, started as `bash --rcfile <this file>`.
# Loads the user's ~/.bashrc first (as bash would without --rcfile), then marks the prompt with
# OSC 133 so the app knows where the command line starts and when a command runs:
# A prompt start, B command start (end of PS1), C command executed (PS0, with the command line
# as bash saved it to its history), D command finished. Each mark carries the session's token
# (AGENTWS_MARK_TOKEN), so the app ignores marks printed by programs. Needs bash 4.4 (PS0).

__agentws_token=${AGENTWS_MARK_TOKEN-}
unset AGENTWS_MARK_TOKEN

if [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi

if [ -z "${__agentws_loaded-}" ] && [ -n "$__agentws_token" ] &&
    { [ "${BASH_VERSINFO[0]:-0}" -gt 4 ] ||
        { [ "${BASH_VERSINFO[0]:-0}" -eq 4 ] && [ "${BASH_VERSINFO[1]:-0}" -ge 4 ]; }; }; then
    __agentws_loaded=1
    __agentws_a='\['$'\e]133;A;aw='"$__agentws_token"$'\a''\]'
    __agentws_b='\['$'\e]133;B;aw='"$__agentws_token"$'\a''\]'

    # First in PROMPT_COMMAND: report the end of the last command, keeping $? for the rest.
    # HISTCMD is the history number the next command line gets.
    __agentws_precmd() {
        local status=$?
        printf '\e]133;D;aw=%s\a' "$__agentws_token"
        __agentws_histcmd=$HISTCMD
        return "$status"
    }

    # Last in PROMPT_COMMAND, after anything that rewrites PS1: wrap it in the A and B marks.
    __agentws_wrap_ps1() {
        local status=$?
        case "$PS1" in
            *"$__agentws_b"*) ;;
            *) PS1="$__agentws_a$PS1$__agentws_b" ;;
        esac
        return "$status"
    }

    # Run from PS0 (in a subshell): the C mark, with the command line hex-encoded when bash
    # saved it to its history as entry number $__agentws_histcmd (not when HISTCONTROL or
    # HISTIGNORE left it out).
    __agentws_ps0() {
        local entry number command hex= byte i LC_ALL=C
        entry=$(HISTTIMEFORMAT='' builtin history 1)
        entry=${entry#"${entry%%[! ]*}"}
        number=${entry%%[!0-9]*}
        if [ -n "$number" ] && [ "$number" = "${__agentws_histcmd-}" ]; then
            # "<number><* if modified> <command>"
            command=${entry:${#number}+2}
            if [ "${#command}" -le 4096 ]; then
                for ((i = 0; i < ${#command}; i++)); do
                    printf -v byte '%02x' "'${command:i:1}"
                    hex+=$byte
                done
            fi
        fi
        printf '\e]133;C;aw=%s%s\a' "$__agentws_token" "${hex:+;cmd=$hex}"
    }

    if [[ "$(declare -p PROMPT_COMMAND 2>/dev/null)" == "declare -a"* ]]; then
        PROMPT_COMMAND=(__agentws_precmd "${PROMPT_COMMAND[@]}" __agentws_wrap_ps1)
    else
        PROMPT_COMMAND=$'__agentws_precmd\n'"${PROMPT_COMMAND-}"$'\n__agentws_wrap_ps1'
    fi
    # PS0 is printed after a command line is read, before it runs.
    PS0='$(__agentws_ps0)'"${PS0-}"
else
    unset __agentws_token
fi
