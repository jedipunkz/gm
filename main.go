package main

import (
	"fmt"
	"log"
	"os"

	tea "charm.land/bubbletea/v2"
)

const usage = `gm — keep every repository in one predictable tree.

usage:
  gm                                open the fuzzy finder (prints the chosen path)
  gm get [-u] [-p] [--shallow] [-b <branch>] [-s] [-l] <repo>...
  gm list [-p] [-e] [--unique] [<query>]
  gm rm [--dry-run] [-y] <repo>...
  gm create [-p] <repo>
  gm migrate [--dry-run] [-y] <directory>...
  gm root [--all]
  gm shell <fish|zsh|bash>          print the Ctrl-G key binding

<repo> is a URL, host/user/repo, user/repo, or just repo.
The root is $GM_ROOT, ~/.config/gm/gm.toml, git config gm.root,
$GHQ_ROOT, git config ghq.root, or ~/ghq.
The finder's colors come from theme in ~/.config/gm/gm.toml.
`

func main() {
	log.SetFlags(0)
	log.SetPrefix("gm: ")

	args := os.Args[1:]
	if len(args) == 0 {
		if err := runTUI(); err != nil {
			log.Fatal(err)
		}
		return
	}

	var err error
	switch args[0] {
	case "get", "clone":
		err = cmdGet(args[1:])
	case "list", "ls":
		err = cmdList(args[1:])
	case "rm", "remove":
		err = cmdRm(args[1:])
	case "create", "new":
		err = cmdCreate(args[1:])
	case "migrate":
		err = cmdMigrate(args[1:])
	case "root":
		err = cmdRoot(args[1:])
	case "shell":
		err = cmdShell(args[1:])
	case "-h", "--help", "help":
		fmt.Print(usage)
		return
	default:
		fmt.Fprint(os.Stderr, usage)
		os.Exit(2)
	}
	if err != nil {
		log.Fatal(err)
	}
}

// runTUI draws on stderr and prints only the chosen path on stdout, so the
// shell binding can capture it with $(gm).
func runTUI() error {
	repos, err := List()
	if err != nil {
		return err
	}
	if len(repos) == 0 {
		root, _ := PrimaryRoot()
		return fmt.Errorf("no repositories under %s; try `gm get <repo>`", root)
	}

	name, err := configTheme()
	if err != nil {
		return err
	}
	if err := applyTheme(name); err != nil {
		return err
	}

	// Draw on the terminal itself, never on stdout: stdout carries the chosen
	// path back to the shell binding.
	opts := []tea.ProgramOption{tea.WithOutput(os.Stderr)}
	if tty, err := os.OpenFile("/dev/tty", os.O_RDWR, 0); err == nil {
		defer tty.Close()
		opts = []tea.ProgramOption{tea.WithInput(tty), tea.WithOutput(tty)}
	}
	p := tea.NewProgram(newModel(repos), opts...)
	res, err := p.Run()
	if err != nil {
		return err
	}
	m, ok := res.(model)
	if !ok || m.chosen == "" {
		return nil
	}
	if err := Bump(m.chosen); err != nil {
		fmt.Fprintf(os.Stderr, "gm: could not record visit: %v\n", err)
	}
	fmt.Println(m.chosen)
	return nil
}

func cmdShell(args []string) error {
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
