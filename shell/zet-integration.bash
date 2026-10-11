# zet-integration.bash — ZET shell integration for bash.
# Source from ~/.bashrc:  source /path/to/zet-integration.bash
# Emits FinalTerm-style OSC 133 markers so ZET can delimit prompt /
# command / exit-code blocks. Requires nothing but bash + printf.

# Guard against double-sourcing.
if [[ -n "${__ZET_SOURCED:-}" ]]; then
    return 0 2>/dev/null || exit 0
fi
__ZET_SOURCED=1

__zet_precmd() {
    local code=$?
    printf '\e]133;D;%s\a' "$code"
    printf '\e]133;A\a'
}

if [[ -z "${PROMPT_COMMAND:-}" ]]; then
    PROMPT_COMMAND="__zet_precmd"
else
    PROMPT_COMMAND="__zet_precmd; $PROMPT_COMMAND"
fi

# Mark end-of-prompt (command input begins). \[ \] keep readline width right.
if [[ "$PS1" != *"]133;B"* ]]; then
    PS1+='\[\e]133;B\a\]'
fi

# Mark pre-execution (command is about to run). PS0 expands after readline
# accepts a line and before bash executes it. Static text only — no
# command substitution, so sourcing this cannot execute anything.
if [[ "${PS0:-}" != *"]133;C"* ]]; then
    PS0+='\e]133;C\a'
fi

# Repair markers every prompt. Frameworks like starship rebuild PS1 (and
# sometimes PROMPT_COMMAND/PS0) on each render, wiping statically appended
# text. This runs LAST so it fixes what they rebuilt; it preserves $?.
__zet_fix_markers() {
    local code=$?
    if [[ "$PS1" != *"]133;B"* ]]; then
        PS1+='\[\e]133;B\a\]'
    fi
    if [[ "${PS0:-}" != *"]133;C"* ]]; then
        PS0+='\e]133;C\a'
    fi
    return "$code"
}

if [[ -z "${PROMPT_COMMAND:-}" ]]; then
    PROMPT_COMMAND="__zet_fix_markers"
elif [[ "$PROMPT_COMMAND" != *"__zet_fix_markers"* ]]; then
    PROMPT_COMMAND="$PROMPT_COMMAND; __zet_fix_markers"
fi
