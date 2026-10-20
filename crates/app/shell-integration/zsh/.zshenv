# agent-workspace shell integration for zsh. The app starts zsh with ZDOTDIR set to this folder,
# so zsh reads this file first. It puts the user's ZDOTDIR back (AGENTWS_USER_ZDOTDIR, unset
# meaning $HOME) and loads the user's .zshenv; zsh then reads their .zprofile, .zshrc and
# .zlogin itself, in the usual order. The OSC 133 marks are set up on the first prompt, once
# every startup file ran: A prompt start, B command start (end of PS1), C command executed
# (preexec, with the command line), D command finished (precmd, with the folder), and L, the
# command line being edited (zle). Each mark carries the session's token (AGENTWS_MARK_TOKEN),
# so the app ignores marks printed by programs. Text is sent hex-encoded.
if [[ -n ${AGENTWS_USER_ZDOTDIR+x} ]]; then
    ZDOTDIR=$AGENTWS_USER_ZDOTDIR
else
    unset ZDOTDIR
fi
__agentws_token=${AGENTWS_MARK_TOKEN-}
__agentws_cd_file=${AGENTWS_CD_FILE-}
unset AGENTWS_USER_ZDOTDIR AGENTWS_MARK_TOKEN AGENTWS_CD_FILE
if [[ -f ${ZDOTDIR:-$HOME}/.zshenv ]]; then
    source "${ZDOTDIR:-$HOME}/.zshenv"
fi

if [[ -o interactive && -n $__agentws_token && -z ${__agentws_loaded-} ]]; then
    __agentws_loaded=1
    __agentws_a=$'%{\e]133;A;aw='$__agentws_token$'\a%}'
    __agentws_b=$'%{\e]133;B;aw='$__agentws_token$'\a%}'

    # Print mark $1 with the parameters $2 (";key=value...").
    __agentws_mark() {
        builtin print -rn -- $'\e]133;'"$1;aw=$__agentws_token$2"$'\a'
    }

    # REPLY: the bytes of $1 in hex; empty when $1 is too long to be worth sending.
    __agentws_hex() {
        emulate -L zsh
        setopt no_multibyte
        local text=$1 c i
        REPLY=
        (( $#text > 4096 )) && return 1
        for (( i = 1; i <= $#text; i++ )); do
            c=${text[i]}
            REPLY+=${(l:2::0:)$(( [##16] #c ))}
        done
    }

    __agentws_wrap_ps1() {
        [[ $PS1 == *$__agentws_b* ]] && return 1
        PS1=$__agentws_a$PS1$__agentws_b
    }

    # Runs last, so it sees the PS1 that prompt themes set in their own precmd.
    __agentws_precmd() {
        if __agentws_hex "$PWD"; then
            __agentws_mark D ";cwd=$REPLY"
        else
            __agentws_mark D
        fi
        __agentws_wrap_ps1
        if [[ ${precmd_functions[-1]} != __agentws_precmd ]]; then
            precmd_functions=(${precmd_functions:#__agentws_precmd} __agentws_precmd)
        fi
    }

    __agentws_preexec() {
        if __agentws_hex "$1"; then
            __agentws_mark C ";cmd=$REPLY"
        else
            __agentws_mark C
        fi
    }

    # The command line and the cursor in it (characters); post=1 when zsh shows text after it
    # (zsh-autosuggestions).
    __agentws_report_line() {
        local post=
        [[ -n $POSTDISPLAY ]] && post=';post=1'
        if __agentws_hex "$BUFFER"; then
            __agentws_mark L ";cur=$CURSOR;buf=$REPLY$post"
        else
            __agentws_mark L
        fi
    }

    # On the first prompt a precmd hook that ran after ours may have replaced PS1.
    __agentws_line_init() {
        __agentws_wrap_ps1 && zle reset-prompt
        __agentws_report_line
    }

    # Bound to Ctrl+Alt+Shift+F12, which the app sends to change folder without typing a command:
    # cd to the folder the app wrote to $__agentws_cd_file (read as text, never run), then
    # redo the prompt hooks and redraw the prompt, keeping the line being typed.
    __agentws_cd() {
        local file=$__agentws_cd_file dir hook
        [[ -n $file && -f $file ]] || return 0
        dir=$(<$file)
        command rm -f -- $file
        builtin cd -- $dir 2>/dev/null || return 0
        for hook in $precmd_functions; do
            $hook
        done
        zle reset-prompt
    }

    # The first precmd: every startup file has run, so hooks added now run after theirs.
    __agentws_install() {
        add-zsh-hook -d precmd __agentws_install
        add-zsh-hook precmd __agentws_precmd
        add-zsh-hook preexec __agentws_preexec
        if autoload -Uz add-zle-hook-widget 2>/dev/null; then
            add-zle-hook-widget line-init __agentws_line_init
            add-zle-hook-widget line-pre-redraw __agentws_report_line
        fi
        if [[ -n $__agentws_cd_file ]]; then
            zle -N __agentws_cd
            local keymap
            for keymap in emacs viins vicmd; do
                bindkey -M $keymap $'\e[24;8~' __agentws_cd
            done
        fi
        __agentws_wrap_ps1
    }

    autoload -Uz add-zsh-hook
    add-zsh-hook precmd __agentws_install
else
    unset __agentws_token
fi
