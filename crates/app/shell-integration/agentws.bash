# agent-workspace shell integration for bash, started as `bash --rcfile <this file>`.
# Loads the user's ~/.bashrc first (as bash would without --rcfile), then marks the prompt with
# OSC 133 so the app knows where the command line starts and when a command runs:
# A prompt start, B command start (end of PS1), C command executed (PS0, with the command line
# as bash saved it to its history), D command finished (with the shell's folder). Each mark
# carries the session's token (AGENTWS_MARK_TOKEN), so the app ignores marks printed by
# programs. Needs bash 4.4 (PS0). With AGENTWS_BASH_LOGIN set (Git Bash, whose shortcut starts
# a login shell, which ignores --rcfile) it loads the login files instead of ~/.bashrc.

__agentws_token=${AGENTWS_MARK_TOKEN-}
__agentws_login=${AGENTWS_BASH_LOGIN-}
unset AGENTWS_MARK_TOKEN AGENTWS_BASH_LOGIN

if [ -n "$__agentws_login" ]; then
    if [ -f /etc/profile ]; then
        . /etc/profile
    fi
    # The first one found, as a login shell reads them.
    for __agentws_file in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
        if [ -f "$__agentws_file" ]; then
            . "$__agentws_file"
            break
        fi
    done
    unset __agentws_file
elif [ -f "$HOME/.bashrc" ]; then
    . "$HOME/.bashrc"
fi
unset __agentws_login

if [ -z "${__agentws_loaded-}" ] && [ -n "$__agentws_token" ] &&
    { [ "${BASH_VERSINFO[0]:-0}" -gt 4 ] ||
        { [ "${BASH_VERSINFO[0]:-0}" -eq 4 ] && [ "${BASH_VERSINFO[1]:-0}" -ge 4 ]; }; }; then
    __agentws_loaded=1
    __agentws_a='\['$'\e]133;A;aw='"$__agentws_token"$'\a''\]'
    __agentws_b='\['$'\e]133;B;aw='"$__agentws_token"$'\a''\]'

    # __agentws_reply: the bytes of $1 in hex; fails when $1 is longer than 4096 bytes.
    __agentws_hex() {
        local text=$1 byte i LC_ALL=C
        __agentws_reply=
        [ "${#text}" -le 4096 ] || return 1
        for ((i = 0; i < ${#text}; i++)); do
            printf -v byte '%02x' "'${text:i:1}"
            __agentws_reply+=$byte
        done
    }

    # __agentws_cwd: the D mark's ";cwd=<hex>" for $PWD, as a native path: a Windows one in
    # Git Bash (MSYS) and Cygwin. Only redone when $PWD changes, cygpath being slow to start.
    __agentws_update_cwd() {
        local dir=$PWD
        __agentws_pwd=$PWD
        __agentws_cwd=
        case "$OSTYPE" in
            msys* | cygwin*) dir=$(cygpath -w "$PWD" 2>/dev/null) || dir= ;;
        esac
        if [ -n "$dir" ] && __agentws_hex "$dir"; then
            __agentws_cwd=";cwd=$__agentws_reply"
        fi
    }

    # First in PROMPT_COMMAND: report the end of the last command and the folder, keeping $?
    # for the rest. HISTCMD is the history number the next command line gets.
    __agentws_precmd() {
        local status=$?
        if [ "$PWD" != "${__agentws_pwd-}" ]; then
            __agentws_update_cwd
        fi
        printf '\e]133;D;aw=%s%s\a' "$__agentws_token" "$__agentws_cwd"
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
        local entry number hex= LC_ALL=C
        entry=$(HISTTIMEFORMAT='' builtin history 1)
        entry=${entry#"${entry%%[! ]*}"}
        number=${entry%%[!0-9]*}
        if [ -n "$number" ] && [ "$number" = "${__agentws_histcmd-}" ]; then
            # "<number><* if modified> <command>"
            if __agentws_hex "${entry:${#number}+2}"; then
                hex=$__agentws_reply
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
