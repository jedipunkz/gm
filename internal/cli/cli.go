// Package cli is gm's command line: one table of subcommands, and the plumbing
// they share. A new subcommand is one entry in commands plus its run function.
package cli

import (
	"bufio"
	"fmt"
	"os"
	"strings"

	"github.com/jedipunkz/gm/internal/config"
	"github.com/jedipunkz/gm/internal/repo"
)

// app is what every subcommand is handed: the settings, and the repository
// tree resolved once for the whole run.
type app struct {
	cfg  config.Config
	tree *repo.Tree
}

// command is one subcommand. usage is the argument spec shown in the help
// text; args is the flag set and the work itself.
type command struct {
	name    string
	aliases []string
	usage   string
	run     func(*app, []string) error
}

var commands = []command{
	{name: "get", aliases: []string{"clone"}, usage: "get [-u] [-p] [--shallow] [-b <branch>] [-s] [-l] <repo>...", run: (*app).get},
	{name: "list", aliases: []string{"ls"}, usage: "list [-p] [-e] [--unique] [<query>]", run: (*app).list},
	{name: "rm", aliases: []string{"remove"}, usage: "rm [--dry-run] [-y] <repo>...", run: (*app).remove},
	{name: "create", aliases: []string{"new"}, usage: "create [-p] <repo>", run: (*app).create},
	{name: "migrate", usage: "migrate [--dry-run] [-y] [--scan] <directory>...", run: (*app).migrate},
	{name: "root", usage: "root [--all]", run: (*app).root},
	{name: "shell", usage: "shell <fish|zsh|bash>          print the Ctrl-G key binding", run: (*app).shell},
}

// Run dispatches one command line. It returns an error for gm to report, or
// errUsage when the arguments made no sense.
func Run(args []string) error {
	cfg, err := config.Load()
	if err != nil {
		return err
	}
	tree, err := repo.Open(cfg)
	if err != nil {
		return err
	}
	a := &app{cfg: cfg, tree: tree}

	if len(args) == 0 {
		return a.finder()
	}
	switch args[0] {
	case "-h", "--help", "help":
		fmt.Print(Usage())
		return nil
	}
	for _, c := range commands {
		if args[0] == c.name || contains(c.aliases, args[0]) {
			return c.run(a, args[1:])
		}
	}
	fmt.Fprint(os.Stderr, Usage())
	return errUnknownCommand
}

// errUnknownCommand asks for exit status 2 without printing anything more:
// the usage text has already been shown.
var errUnknownCommand = fmt.Errorf("unknown command")

// IsUsageError reports whether an error means "the arguments were wrong",
// which gm answers with exit status 2 rather than a message.
func IsUsageError(err error) bool { return err == errUnknownCommand }

// Usage is gm's help text, built from the command table so it cannot drift
// away from what actually runs.
func Usage() string {
	var b strings.Builder
	b.WriteString("gm — keep every repository in one predictable tree.\n\nusage:\n")
	b.WriteString("  gm                                open the fuzzy finder (prints the chosen path)\n")
	for _, c := range commands {
		fmt.Fprintf(&b, "  gm %s\n", c.usage)
	}
	b.WriteString(`
<repo> is a URL, host/user/repo, user/repo, or just repo.
The root is $GM_ROOT, ~/.config/gm/gm.toml, git config gm.root,
$GHQ_ROOT, git config ghq.root, or ~/ghq.
The finder's colors come from theme in ~/.config/gm/gm.toml.
`)
	return b.String()
}

func contains(ss []string, s string) bool {
	for _, v := range ss {
		if v == s {
			return true
		}
	}
	return false
}

// confirm asks before anything destructive. A non-tty answers "no".
func confirm(prompt string) bool {
	fmt.Fprintf(os.Stderr, "%s [y/N]: ", prompt)
	s := bufio.NewScanner(os.Stdin)
	if !s.Scan() {
		return false
	}
	a := strings.ToLower(strings.TrimSpace(s.Text()))
	return a == "y" || a == "yes"
}
