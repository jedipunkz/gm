package cli

import "fmt"

func (a *app) shell(args []string) error {
	if len(args) != 1 {
		return fmt.Errorf("usage: gm shell <fish|zsh|bash>")
	}
	snippet, ok := shellSnippets[args[0]]
	if !ok {
		return fmt.Errorf("unsupported shell %q (fish, zsh, bash)", args[0])
	}
	fmt.Print(snippet)
	return nil
}

var shellSnippets = map[string]string{
	"fish": `# gm: Ctrl-G jumps to a repository. Add to ~/.config/fish/config.fish:
#   gm shell fish | source
function __gm_jump
    set -l dir (gm)
    if test -n "$dir"
        cd $dir
        commandline -f repaint
    end
end
bind \cg __gm_jump
if bind -M insert >/dev/null 2>&1
    bind -M insert \cg __gm_jump
end
`,
	"zsh": `# gm: Ctrl-G jumps to a repository. Add to ~/.zshrc:
#   eval "$(gm shell zsh)"
__gm_jump() {
  local dir
  dir=$(gm) || return
  [[ -n $dir ]] && cd -- "$dir"
  zle reset-prompt
}
zle -N __gm_jump
bindkey '^g' __gm_jump
`,
	"bash": `# gm: Ctrl-G jumps to a repository. Add to ~/.bashrc:
#   eval "$(gm shell bash)"
__gm_jump() {
  local dir
  dir=$(gm) || return
  [ -n "$dir" ] && cd -- "$dir"
}
bind -x '"\C-g": __gm_jump'
`,
}
