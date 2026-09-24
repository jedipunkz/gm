package repo

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
)

// PullRequest is an open pull request, as much of it as a list row and the
// details pane need.
type PullRequest struct {
	Number int    `json:"number"`
	Title  string `json:"title"`
	Branch string `json:"headRefName"`
	Draft  bool   `json:"isDraft"`
	Fork   bool   `json:"isCrossRepository"`
	Author struct {
		Login string `json:"login"`
	} `json:"author"`
	HeadOwner struct {
		Login string `json:"login"`
	} `json:"headRepositoryOwner"`
}

// Label is how the pull request reads in a list: number first, so typing it
// finds the row, and a draft says so before its title.
func (p PullRequest) Label() string {
	draft := ""
	if p.Draft {
		draft = "[draft] "
	}
	return fmt.Sprintf("#%d %s%s", p.Number, draft, p.Title)
}

// Checkout is the name the pull request's worktree is filed under. A fork's
// branch is often called main, so it goes under its owner's name instead of
// next to the repository's own main.
func (p PullRequest) Checkout() string {
	if p.Fork && p.HeadOwner.Login != "" {
		return p.HeadOwner.Login + "/" + p.Branch
	}
	return p.Branch
}

// prLimit is how many open pull requests gh is asked for, newest first.
const prLimit = 100

// PullRequests asks gh for the repository's open pull requests. gh talks to
// GitHub, so this is slow next to git and fails without a network, gh or a
// login; the error says which.
func PullRequests(dir string) ([]PullRequest, error) {
	cmd := exec.Command("gh", "pr", "list", "--state", "open", "--limit", strconv.Itoa(prLimit),
		"--json", "number,title,headRefName,isDraft,isCrossRepository,author,headRepositoryOwner")
	cmd.Dir = dir
	cmd.Env = append(os.Environ(), "GH_PROMPT_DISABLED=1")
	out, err := cmd.Output()
	if err != nil {
		return nil, ghError(err)
	}
	return parsePullRequests(out)
}

func parsePullRequests(out []byte) ([]PullRequest, error) {
	var prs []PullRequest
	if err := json.Unmarshal(out, &prs); err != nil {
		return nil, fmt.Errorf("gh pr list: %w", err)
	}
	return prs, nil
}

// CheckOutPullRequest has gh check the pull request out as a worktree at dir.
// gh names the branch and, for a fork, sets up where it pushes to.
func CheckOutPullRequest(repoDir, dir string, number int) error {
	if err := os.MkdirAll(filepath.Dir(dir), 0o755); err != nil {
		return err
	}
	cmd := exec.Command("gh", "pr", "checkout", strconv.Itoa(number), "--worktree", dir)
	cmd.Dir = repoDir
	cmd.Env = append(os.Environ(), "GH_PROMPT_DISABLED=1")
	// Everything gh prints stays off the terminal: the finder is drawn there.
	if out, err := cmd.CombinedOutput(); err != nil {
		if msg := gitMessage(out); msg != "" {
			return errors.New(msg)
		}
		return ghError(err)
	}
	return nil
}

// ghError turns a failed gh into the one line worth reading.
func ghError(err error) error {
	if errors.Is(err, exec.ErrNotFound) {
		return errors.New("gh is not installed: pull requests come from the GitHub CLI")
	}
	var ee *exec.ExitError
	if errors.As(err, &ee) {
		if msg := gitMessage(ee.Stderr); msg != "" {
			return errors.New(msg)
		}
	}
	return err
}
