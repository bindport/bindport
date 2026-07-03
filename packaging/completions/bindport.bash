# bash completion for bindport
#
# shellcheck disable=SC2207
# Completion candidates below are static flag/subcommand word lists, matching
# the conventional bash-completion compgen + COMPREPLY idiom.

_bindport()
{
    local cur prev words cword
    _init_completion -n : || return

    local commands="run reserve release status open clean config hooks doctor dashboard render templates init"
    local global_opts="--help -h --version"
    local run_opts="--env --hostname --route-url --health-url --help -h"
    local clean_opts="--dry-run --stopped --stale --all --json --yes -y --help -h"
    local dashboard_opts="serve start status stop --host --port --auth --auth-required --no-auth --register-service --no-register-service --token --token-env --allowed-host --static-dir --help -h"
    local hook_commands="status trust deny reset"
    local hook_opts="--scope --all --help -h"
    local render_opts="--all --dry-run --repair --help -h"
    local template_commands="list show export"
    local template_opts="--source --help -h"
    local source_values="project global built-in"
    local config_commands="explain validate"
    local doctor_commands="outputs"
    local auth_values="required disabled"
    local scope_values="worktree repo"

    case "$prev" in
        --auth)
            COMPREPLY=($(compgen -W "$auth_values" -- "$cur"))
            return
            ;;
        --scope)
            COMPREPLY=($(compgen -W "$scope_values" -- "$cur"))
            return
            ;;
        --source)
            COMPREPLY=($(compgen -W "$source_values" -- "$cur"))
            return
            ;;
        --env | --hostname | --route-url | --health-url | --host | --port | --token | --token-env | --allowed-host | --static-dir)
            return
            ;;
    esac

    local command="${words[1]}"
    case "$command" in
        "" | -*)
            COMPREPLY=($(compgen -W "$commands $global_opts" -- "$cur"))
            ;;
        run | reserve)
            COMPREPLY=($(compgen -W "$run_opts" -- "$cur"))
            ;;
        release)
            COMPREPLY=($(compgen -W "--help -h" -- "$cur"))
            ;;
        clean)
            COMPREPLY=($(compgen -W "$clean_opts" -- "$cur"))
            ;;
        config)
            COMPREPLY=($(compgen -W "$config_commands --help -h" -- "$cur"))
            ;;
        hooks)
            if [[ "$cword" -eq 2 ]]; then
                COMPREPLY=($(compgen -W "$hook_commands --help -h" -- "$cur"))
            else
                COMPREPLY=($(compgen -W "$hook_opts" -- "$cur"))
            fi
            ;;
        doctor)
            COMPREPLY=($(compgen -W "$doctor_commands --help -h" -- "$cur"))
            ;;
        dashboard)
            COMPREPLY=($(compgen -W "$dashboard_opts" -- "$cur"))
            ;;
        render)
            COMPREPLY=($(compgen -W "$render_opts" -- "$cur"))
            ;;
        templates)
            if [[ "$cword" -eq 2 ]]; then
                COMPREPLY=($(compgen -W "$template_commands --help -h" -- "$cur"))
            else
                COMPREPLY=($(compgen -W "$template_opts" -- "$cur"))
            fi
            ;;
        init)
            COMPREPLY=($(compgen -W "--project --user --help -h" -- "$cur"))
            ;;
        status | open)
            COMPREPLY=($(compgen -W "--json --project --browser --print --help -h" -- "$cur"))
            ;;
        *)
            COMPREPLY=($(compgen -W "$commands $global_opts" -- "$cur"))
            ;;
    esac
}

complete -F _bindport bindport
