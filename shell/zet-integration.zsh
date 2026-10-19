# zet-integration.zsh — ZET shell integration for zsh.
# Source from ~/.zshrc:  source /path/to/zet-integration.zsh
# Emits FinalTerm-style OSC 133 markers so ZET can delimit prompt /
# command / exit-code blocks.

if (( ${+__ZET_SOURCED} )); then
    return 0
fi
typeset -g __ZET_SOURCED=1

# Best-effort: names reporting needs zsh/parameter. Guarded so a missing
# module never aborts precmd (D/A markers are the critical part).
#
# zsh(1): `zmodload <mod>` returns 0 when the module loads and 1 when it
# does not, and prints "failed to load module" to stderr on failure --
# `2>/dev/null` silences that. But the ${+aliases} parameter expansion
# below RE-TRIGGERS the load on every prompt, and that re-trigger is not
# silenced, so a shell without zsh/parameter printed the error on every
# single prompt. Cache the load result once and gate the expansion on it.
if zmodload zsh/parameter 2>/dev/null; then
    typeset -g __ZET_NAMES_OK=1
else
    typeset -g __ZET_NAMES_OK=0
fi

__zet_precmd() {
    local code=$?
    printf '\e]133;D;%s\a' "$code"
    printf '\e]133;A\a'
    # Report aliases/functions (once per change): ZET routes unknown
    # first words to the model, so it must know shell-defined names.
    # Parameter expansion only: no fork. Change-gated: silent prompt.
    if (( ${__ZET_NAMES_OK} )); then
        local names
        names="${(j: :)${(k)aliases}} ${(j: :)${(k)functions}}"
        if [[ "$names" != "${__ZET_CMDS_SENT:-}" ]]; then
            __ZET_CMDS_SENT="$names"
            printf '\e]133;X;%s\a' "$names"
        fi
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
