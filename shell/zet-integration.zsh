# zet-integration.zsh — ZET shell integration for zsh.
# Source from ~/.zshrc:  source /path/to/zet-integration.zsh
# Emits FinalTerm-style OSC 133 markers so ZET can delimit prompt /
# command / exit-code blocks.

if (( ${+__ZET_SOURCED} )); then
    return 0
fi
typeset -g __ZET_SOURCED=1

__zet_precmd() {
    local code=$?
    printf '\e]133;D;%s\a' "$code"
    printf '\e]133;A\a'
    # Report aliases/functions (once per change): ZET routes unknown
    # first words to the model, so it must know shell-defined names.
    # Parameter expansion only: no fork. Change-gated: silent prompt.
    local names
    names="${(j: :)${(k)aliases}} ${(j: :)${(k)functions}}"
    if [[ "$names" != "${__ZET_CMDS_SENT:-}" ]]; then
        __ZET_CMDS_SENT="$names"
        printf '\e]133;X;%s\a' "$names"
    fi
}

__zet_preexec() {
    printf '\e]133;C\a'
}

precmd_functions+=(__zet_precmd)
preexec_functions+=(__zet_preexec)

# Mark end-of-prompt (command input begins). %{ %} keep ZLE width right.
if [[ "$PS1" != *"]133;B"* ]]; then
    PS1+='%{'$'\e]133;B\a''%}'
fi
