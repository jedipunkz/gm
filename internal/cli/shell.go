package cli

import (
	"fmt"
	"strings"

	"github.com/jedipunkz/gm/internal/config"
)

// DefaultKeybind is the shell key gm binds when gm.toml says nothing.
const DefaultKeybind = "ctrl-g"

func (a *app) shell(args []string) error {
	if len(args) != 1 {
		return fmt.Errorf("usage: gm shell <fish|zsh|bash>")
	}
	tmpl, ok := shellSnippets[args[0]]
	if !ok {
		return fmt.Errorf("unsupported shell %q (fish, zsh, bash)", args[0])
	}
	k, err := config.ParseChord(a.cfg.Keybind, DefaultKeybind)
	if err != nil {
		return err
	}
	fmt.Print(strings.NewReplacer(
		"{{key}}", string(k.Letter),
		"{{name}}", k.Display,
	).Replace(tmpl))
	return nil
}

// shellSnippets are the bindings gm prints. {{key}} is the chord's letter and
// {{name}} its human spelling; each shell writes the Ctrl prefix its own way.
var shellSnippets = map[string]string{
	"fish": `# gm: {{name}} jumps to a repository. Add to ~/.config/fish/config.fish:
#   gm shell fish | source
function __gm_jump
    set -l dir (gm)
    if test -n "$dir"
        cd $dir
        commandline -f repaint
    end
end
bind \c{{key}} __gm_jump
if bind -M insert >/dev/null 2>&1
    bind -M insert \c{{key}} __gm_jump
end
`,
	"zsh": `# gm: {{name}} jumps to a repository. Add to ~/.zshrc:
#   eval "$(gm shell zsh)"
__gm_jump() {
  local dir
  dir=$(gm) || return
  [[ -n $dir ]] && cd -- "$dir"
  zle reset-prompt
}
zle -N __gm_jump
bindkey '^{{key}}' __gm_jump
`,
	"bash": `# gm: {{name}} jumps to a repository. Add to ~/.bashrc:
#   eval "$(gm shell bash)"
__gm_jump() {
  local dir
  dir=$(gm) || return
  [ -n "$dir" ] && cd -- "$dir"
}
bind -x '"\C-{{key}}": __gm_jump'
`,
}
