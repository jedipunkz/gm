package repo

import (
	"errors"
	"fmt"
	"net/url"
	"os"
	"os/exec"
	"regexp"
	"strings"
)

var (
	schemeRe  = regexp.MustCompile(`^[A-Za-z][A-Za-z0-9+.\-]*://`)
	scpLikeRe = regexp.MustCompile(`^(?:([^@/]+)@)?([^:/]+):(/?.+)$`)
	hostRe    = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9.\-]*\.[A-Za-z]{2,}(?::\d{1,5})?$`)
)

// NormalizeURL turns every shorthand gm accepts into a real URL:
//
//	https://github.com/u/r  -> as-is
//	git@github.com:u/r.git  -> ssh://git@github.com/u/r.git
//	example.com/u/r         -> https://example.com/u/r
//	u/r                     -> https://github.com/u/r
//	r                       -> https://github.com/<you>/r
//
// With ssh set, https refs are rewritten to ssh://git@...
func NormalizeURL(ref string, ssh bool) (*url.URL, error) {
	ref = strings.TrimRight(strings.TrimSpace(ref), "/")
	if ref == "" {
		return nil, errors.New("empty repository reference")
	}

	if !schemeRe.MatchString(ref) {
		if m := scpLikeRe.FindStringSubmatch(ref); m != nil && hostRe.MatchString(m[2]) {
			user := ""
			if m[1] != "" {
				user = m[1] + "@"
			}
			ref = "ssh://" + user + m[2] + "/" + strings.TrimPrefix(m[3], "/")
		} else {
			parts := strings.Split(ref, "/")
			switch {
			case len(parts) >= 3 && hostRe.MatchString(parts[0]):
				ref = "https://" + ref
			case len(parts) == 2:
				ref = "https://github.com/" + ref
			case len(parts) == 1:
				me, err := selfName()
				if err != nil {
					return nil, err
				}
				ref = "https://github.com/" + me + "/" + ref
			default:
				return nil, fmt.Errorf("cannot make a repository URL out of %q", ref)
			}
		}
	}

	u, err := url.Parse(ref)
	if err != nil {
		return nil, err
	}
	if u.Hostname() == "" || strings.Trim(u.Path, "/") == "" {
		return nil, fmt.Errorf("cannot make a repository URL out of %q", ref)
	}
	if ssh && u.Scheme == "https" {
		u.Scheme = "ssh"
		u.User = url.User("git")
	}
	return u, nil
}

// selfName is the GitHub account used for one-word refs like "gm create foo".
func selfName() (string, error) {
	if out, err := exec.Command("git", "config", "--get", "github.user").Output(); err == nil {
		if s := strings.TrimSpace(string(out)); s != "" {
			return s, nil
		}
	}
	if u := os.Getenv("USER"); u != "" {
		return u, nil
	}
	return "", errors.New("cannot guess your account name; set `git config --global github.user <name>` or use <user>/<repo>")
}

// RelPathOf is where a URL lands under a root: host/path, minus any .git
// suffix.
func RelPathOf(u *url.URL) string {
	p := strings.Trim(u.Path, "/")
	p = strings.TrimSuffix(p, ".git")
	return strings.Join(append([]string{u.Hostname()}, strings.Split(p, "/")...), "/")
}
