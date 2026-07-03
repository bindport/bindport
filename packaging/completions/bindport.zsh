#compdef bindport

local -a commands
commands=(
  'run:run a command or configured service command with an assigned port'
  'reserve:hold a port without running a child process'
  'release:release a reserved port'
  'status:show registry status'
  'open:print or open the best service URL'
  'clean:remove stopped and stale registry entries'
  'config:explain or validate configuration'
  'hooks:inspect or manage hook trust'
  'doctor:show bootstrap diagnostics'
  'dashboard:serve or control the local dashboard'
  'render:render configured output files'
  'templates:list, show, or export output templates'
  'init:create project or user config'
)

local -a run_opts clean_opts dashboard_opts render_opts
run_opts=(
  '--env[add a templated child environment variable]:NAME=VALUE:'
  '--hostname[set route hostname metadata]:template:'
  '--route-url[set route URL metadata]:template:'
  '--health-url[set service health check URL metadata]:template:'
)
clean_opts=(
  '--dry-run[show what would be removed without deleting entries]'
  '--stopped[remove stopped entries only]'
  '--stale[remove stale entries only]'
  '--all[remove stopped and stale entries]'
  '--json[print machine-readable cleanup counts]'
  '(-y --yes)'{-y,--yes}'[confirm stale entry deletion without prompting]'
)
dashboard_opts=(
  '--host[bind IP address]:ip:'
  '--port[preferred dashboard port]:port:'
  '--auth[authentication mode]:mode:(required disabled)'
  '--auth-required[require bearer token access]'
  '--no-auth[disable bearer token checks]'
  '--register-service[record the dashboard in BindPort status]'
  '--no-register-service[do not record the dashboard in BindPort status]'
  '--token[bearer token value]:token:'
  '--token-env[environment variable containing the token]:name:'
  '--allowed-host[additional accepted HTTP Host header]:host:'
  '--static-dir[read dashboard assets from a local directory]:directory:_files -/'
)
render_opts=('--all[render every enabled output]' '--dry-run[print targets without writing files]' '--repair[reconcile DB-owned files]')

_arguments -C \
  '(-h --help)'{-h,--help}'[show help]' \
  '--version[print version]' \
  '1:command:->command' \
  '*::arg:->args'

case "$state" in
  command)
    _describe -t commands 'bindport command' commands
    ;;
  args)
    case "$words[2]" in
      run|reserve)
        _arguments $run_opts
        ;;
      release)
        _arguments '1:service or port:'
        ;;
      status)
        _arguments '--json[print machine-readable status]'
        ;;
      open)
        _arguments '--project[disambiguate project]:project:' '--browser[open the URL with the system browser]' '--print[print without launching a browser]' '1:service:'
        ;;
      clean)
        _arguments $clean_opts
        ;;
      config)
        _arguments '1:config command:(explain validate)'
        ;;
      hooks)
        _arguments '1:hook command:(status trust deny reset)' '--scope[trust scope]:scope:(worktree repo)' '--all[select every configured hook]' '2:hook:'
        ;;
      doctor)
        _arguments '1:doctor command:(outputs)'
        ;;
      dashboard)
        _arguments '1:dashboard command:(serve start status stop)' $dashboard_opts
        ;;
      render)
        _arguments '1:output:' $render_opts
        ;;
      templates)
        _arguments '1:template command:(list show export)' '--source[template source]:source:(project global built-in)' '2:template:'
        ;;
      init)
        _arguments '--project[create .bindport.toml in the current directory]' '--user[create optional user fallback config]'
        ;;
    esac
    ;;
esac
