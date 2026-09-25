package repo

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"strings"
	"syscall"
	"time"
)

// Timeouts for the calls that go to a remote. Listing moves only ref names;
// fetching one branch moves its objects, which can take a while.
const (
	listTimeout  = 20 * time.Second
	fetchTimeout = 5 * time.Minute
)

// gitRemote runs git for a call that talks to a remote, with every way it
// could ask for a password shut. The finder owns the terminal: a prompt would
// be drawn over it and wait for input that never comes. What does not need
// typing — an ssh agent, a credential helper — still works.
func gitRemote(timeout time.Duration, dir string, args ...string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, "git", append([]string{"-C", dir}, args...)...)
	cmd.Env = append(os.Environ(), "GIT_TERMINAL_PROMPT=0")
	// ssh prompts on the controlling terminal, not on stdin; batch mode
	// makes it fail instead. A user's own ssh command is left alone.
	if os.Getenv("GIT_SSH_COMMAND") == "" && gitConfig(dir, "core.sshCommand") == "" {
		cmd.Env = append(cmd.Env, "GIT_SSH_COMMAND=ssh -o BatchMode=yes")
	}
	// A new session has no controlling terminal, so nothing git starts can
	// open /dev/tty to ask, whatever it is.
	cmd.SysProcAttr = &syscall.SysProcAttr{Setsid: true}
	out, err := cmd.Output()
	if ctx.Err() != nil {
		return "", errors.New("git " + args[0] + " took longer than " + timeout.String())
	}
	if err != nil {
		var ee *exec.ExitError
		if errors.As(err, &ee) {
			if msg := gitMessage(ee.Stderr); msg != "" {
				return "", errors.New(msg)
			}
		}
		return "", err
	}
	return string(out), nil
}

func gitConfig(dir, key string) string {
	out, _ := GitIn(dir, "config", "--get", key)
	return out
}

// RemoteBranches asks every remote which branches it has right now, without
// fetching anything. The answer holds only what the last fetch did not bring:
// a branch already in refs/remotes is in Branches, with its date. A remote
// that cannot be reached is skipped, and the first such error is returned
// beside what the others said.
func RemoteBranches(dir string) ([]Branch, error) {
	names, _ := remotes(dir)
	fetched := map[string]bool{}
	if out, err := GitIn(dir, "for-each-ref", "--format=%(refname:short)", "refs/remotes"); err == nil {
		for _, r := range strings.Split(out, "\n") {
			fetched[r] = true
		}
	}
	var list []Branch
	var first error
	for _, r := range names {
		out, err := gitRemote(listTimeout, dir, "ls-remote", "--heads", r)
		if err != nil {
			if first == nil {
				first = errors.New(r + ": " + err.Error())
			}
			continue
		}
		for _, b := range parseLsRemote(out, r) {
			if !fetched[b.Remote] {
				list = append(list, b)
			}
		}
	}
	return list, first
}

// parseLsRemote reads "<hash>\trefs/heads/<name>" lines into branches of the
// remote r that have not been fetched.
func parseLsRemote(out, r string) []Branch {
	var list []Branch
	for _, l := range strings.Split(out, "\n") {
		_, ref, ok := strings.Cut(l, "\t")
		if !ok {
			continue
		}
		name, ok := strings.CutPrefix(ref, "refs/heads/")
		if !ok || name == "" {
			continue
		}
		list = append(list, Branch{Name: name, Remote: r + "/" + name, Unfetched: true})
	}
	return list
}

// FetchBranch brings one branch of a remote into refs/remotes, so a worktree
// can start from it. The name must already have passed ValidBranch: it goes
// into a refspec.
func FetchBranch(dir string, b Branch) error {
	r, ok := strings.CutSuffix(b.Remote, "/"+b.Name)
	if !ok || !ValidBranch(b.Name) {
		return errors.New(b.Remote + " is not a remote branch")
	}
	_, err := gitRemote(fetchTimeout, dir, "fetch", "--no-tags", r,
		"refs/heads/"+b.Name+":refs/remotes/"+b.Remote)
	return err
}
