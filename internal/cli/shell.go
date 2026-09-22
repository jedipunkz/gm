package cli

import (
	"fmt"
	"strings"
)

// DefaultKeybind is the key gm binds when gm.toml says nothing.
const DefaultKeybind = "ctrl-g"

// keybind is one Ctrl-<letter> chord, in the spellings each shell wants.
type keybind struct {
	letter  byte   // lowercase, e.g. 'g'
	display string // "Ctrl-G", for the comment at the top of the snippet
}

// parseKeybind accepts ctrl-g, ctrl+g, c-g and ^g, in any case. Only Ctrl
// chords are supported: they are what a shell can bind to a widget without
// fighting the terminal over escape sequences.
func parseKeybind(s string) (keybind, error) {
	fail := func() (keybind, error) {
		return keybind{}, fmt.Errorf("cannot bind %q: use a Ctrl chord such as ctrl-g", s)
	}
	t := strings.ToLower(strings.TrimSpace(s))
	switch {
	case strings.HasPrefix(t, "ctrl-"), strings.HasPrefix(t, "ctrl+"):
		t = t[5:]
	case strings.HasPrefix(t, "c-"):
		t = t[2:]
	case strings.HasPrefix(t, "^"):
		t = t[1:]
	default:
		return fail()
	}
	if len(t) != 1 || t[0] < 'a' || t[0] > 'z' {
		return fail()
	}
	return keybind{letter: t[0], display: "Ctrl-" + strings.ToUpper(t)}, nil
}

func (a *app) shell(args []string) error {
	if len(args) != 1 {
		return fmt.Errorf("usage: gm shell <fish|zsh|bash>")
	}
	tmpl, ok := shellSnippets[args[0]]
	if !ok {
		return fmt.Errorf("unsupported shell %q (fish, zsh, bash)", args[0])
	}
	name := a.cfg.Keybind
	if name == "" {
		name = DefaultKeybind
	}
	k, err := parseKeybind(name)
	if err != nil {
		return err
	}
	fmt.Print(strings.NewReplacer(
		"{{key}}", string(k.letter),
		"{{name}}", k.display,
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
