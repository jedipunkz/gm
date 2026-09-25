use crate::{Result, err};

/// Url is the part of net/url gm needs. Every URL it builds or reads is
/// scheme://[user@]host[:port]/path, so a full parser would only add ways for
/// it to rewrite what the user typed.
#[derive(Debug, Clone, PartialEq)]
pub struct Url {
    pub scheme: String,
    pub user: String,
    pub host: String, // with the port, as it was written
    pub path: String,
    pub rest: String, // ?query#fragment, kept verbatim
}

impl Url {
    /// parse splits a URL that has a scheme. A missing scheme, or a host with
    /// a character no host has, is an error, as it was for url.Parse.
    pub fn parse(s: &str) -> Result<Url> {
        let (scheme, after) = s
            .split_once("://")
            .ok_or_else(|| err!("parse {s:?}: missing scheme"))?;
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        let (authority, tail) = after.split_at(end);
        let (user, host) = match authority.rfind('@') {
            Some(i) => (&authority[..i], &authority[i + 1..]),
            None => ("", authority),
        };
        if let Some(c) = host.chars().find(|c| c.is_whitespace() || c.is_control()) {
            return Err(err!("parse {s:?}: invalid character {c:?} in host name"));
        }
        let split = tail.find(['?', '#']).unwrap_or(tail.len());
        Ok(Url {
            scheme: scheme.to_string(),
            user: user.to_string(),
            host: host.to_string(),
            path: tail[..split].to_string(),
            rest: tail[split..].to_string(),
        })
    }

    /// hostname is the host without its port.
    pub fn hostname(&self) -> &str {
        if let Some(v6) = self.host.strip_prefix('[') {
            return v6.split(']').next().unwrap_or("");
        }
        self.host.split(':').next().unwrap_or("")
    }
}

impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}://", self.scheme)?;
        if !self.user.is_empty() {
            write!(f, "{}@", self.user)?;
        }
        write!(f, "{}{}{}", self.host, self.path, self.rest)
    }
}

/// normalize_url turns every shorthand gm accepts into a real URL:
///
///   https://github.com/u/r  -> as-is
///   git@github.com:u/r.git  -> ssh://git@github.com/u/r.git
///   example.com/u/r         -> https://example.com/u/r
///   u/r                     -> https://github.com/u/r
///   r                       -> https://github.com/<you>/r
///
/// With ssh set, https refs are rewritten to ssh://git@...
pub fn normalize_url(reference: &str, ssh: bool) -> Result<Url> {
    let mut r = reference.trim().trim_end_matches('/').to_string();
    if r.is_empty() {
        return Err("empty repository reference".into());
    }

    if !has_scheme(&r) {
        match scp_like(&r).filter(|(_, host, _)| is_host(host)) {
            Some((user, host, path)) => {
                let user = user.map(|u| format!("{u}@")).unwrap_or_default();
                r = format!("ssh://{user}{host}/{}", path.trim_start_matches('/'));
            }
            None => {
                let parts: Vec<&str> = r.split('/').collect();
                r = match parts.len() {
                    n if n >= 3 && is_host(parts[0]) => format!("https://{r}"),
                    2 => format!("https://github.com/{r}"),
                    1 => format!("https://github.com/{}/{r}", self_name()?),
                    _ => return Err(err!("cannot make a repository URL out of {r:?}")),
                };
            }
        }
    }

    let mut u = Url::parse(&r)?;
    if u.hostname().is_empty() || u.path.trim_matches('/').is_empty() {
        return Err(err!("cannot make a repository URL out of {r:?}"));
    }
    // The host and the path become directories under the root, and a "." or
    // ".." among them would put the clone somewhere else, outside the root
    // included. Every command that turns a reference into a place comes
    // through here, so this is the one check they need.
    let segments = std::iter::once(u.hostname()).chain(u.path.split('/'));
    if segments.into_iter().any(|seg| seg == "." || seg == "..") {
        return Err(err!("{reference:?} has a \".\" or \"..\" in its path"));
    }
    if ssh && u.scheme == "https" {
        u.scheme = "ssh".into();
        u.user = "git".into();
    }
    Ok(u)
}

/// has_scheme is ^[A-Za-z][A-Za-z0-9+.-]*://
fn has_scheme(s: &str) -> bool {
    let Some(i) = s.find("://") else { return false };
    let mut cs = s[..i].chars();
    cs.next().is_some_and(|c| c.is_ascii_alphabetic())
        && cs.all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
}

/// scp_like is ^(?:([^@/]+)@)?([^:/]+):(/?.+)$, the user@host:path form ssh
/// and git accept: the user, the host and the path, the way that regular
/// expression would have split them.
fn scp_like(s: &str) -> Option<(Option<&str>, &str, &str)> {
    let host_and_path = |s: &str| -> Option<(usize, usize)> {
        let end = s.find([':', '/'])?;
        let rest = &s[end + 1..];
        (end > 0 && s[end..].starts_with(':') && !rest.is_empty() && !rest.contains('\n'))
            .then_some((end, end + 1))
    };
    if let Some(at) = s.find('@').filter(|&at| at > 0 && !s[..at].contains('/')) {
        let rest = &s[at + 1..];
        if let Some((end, from)) = host_and_path(rest) {
            return Some((Some(&s[..at]), &rest[..end], &rest[from..]));
        }
    }
    let (end, from) = host_and_path(s)?;
    Some((None, &s[..end], &s[from..]))
}

/// is_host is ^[A-Za-z0-9][A-Za-z0-9.-]*\.[A-Za-z]{2,}(?::\d{1,5})?$: a name
/// with a dot in it and a top-level label of letters, and maybe a port.
fn is_host(s: &str) -> bool {
    let (name, port) = match s.split_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (s, None),
    };
    if let Some(p) = port
        && (p.is_empty() || p.len() > 5 || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let Some(dot) = name.rfind('.') else {
        return false;
    };
    let tld = &name[dot + 1..];
    dot >= 1
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// self_name is the GitHub account used for one-word refs like "gm create foo".
fn self_name() -> Result<String> {
    if let Ok(out) = crate::repo::git_command()
        .args(["config", "--get", "github.user"])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !s.is_empty() {
            return Ok(s);
        }
    }
    match std::env::var("USER") {
        Ok(u) if !u.is_empty() => Ok(u),
        _ => Err("cannot guess your account name; set `git config --global github.user <name>` or use <user>/<repo>".into()),
    }
}

/// rel_path_of is where a URL lands under a root: host/path, minus any .git
/// suffix.
pub fn rel_path_of(u: &Url) -> String {
    let p = u.path.trim_matches('/');
    let p = p.strip_suffix(".git").unwrap_or(p);
    format!("{}/{p}", u.hostname())
}

/// browse_url turns a git remote into the https URL a browser can open:
///
///   ssh://git@github.com/u/r.git  -> https://github.com/u/r
///   git@github.com:u/r.git        -> https://github.com/u/r
///   https://github.com/u/r.git    -> https://github.com/u/r
///
/// The port a git URL may carry is dropped: it addresses the git service, not
/// the web one.
pub fn browse_url(remote: &str) -> Result<String> {
    let mut u = normalize_url(remote, false)?;
    u.scheme = "https".into();
    u.user.clear();
    u.host = u.hostname().to_string();
    if let Some(p) = u.path.strip_suffix(".git") {
        u.path = p.to_string();
    }
    Ok(u.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_reads_every_shorthand() {
        for (reference, ssh, want, rel) in [
            (
                "https://github.com/x-motemen/ghq",
                false,
                "https://github.com/x-motemen/ghq",
                "github.com/x-motemen/ghq",
            ),
            (
                "https://github.com/x-motemen/ghq.git",
                false,
                "https://github.com/x-motemen/ghq.git",
                "github.com/x-motemen/ghq",
            ),
            (
                "git@github.com:x-motemen/ghq.git",
                false,
                "ssh://git@github.com/x-motemen/ghq.git",
                "github.com/x-motemen/ghq",
            ),
            (
                "x-motemen/ghq",
                false,
                "https://github.com/x-motemen/ghq",
                "github.com/x-motemen/ghq",
            ),
            (
                "gitlab.com/g/p",
                false,
                "https://gitlab.com/g/p",
                "gitlab.com/g/p",
            ),
            (
                "git.example.com:2222/g/p",
                false,
                "ssh://git.example.com/2222/g/p",
                "git.example.com/2222/g/p",
            ),
            (
                "x-motemen/ghq",
                true,
                "ssh://git@github.com/x-motemen/ghq",
                "github.com/x-motemen/ghq",
            ),
            (
                "https://github.com/x-motemen/ghq/",
                false,
                "https://github.com/x-motemen/ghq",
                "github.com/x-motemen/ghq",
            ),
        ] {
            let u = normalize_url(reference, ssh).unwrap();
            assert_eq!(u.to_string(), want, "normalize_url({reference:?})");
            assert_eq!(rel_path_of(&u), rel, "rel_path_of({reference:?})");
        }
        for bad in ["", "   ", "https://github.com"] {
            assert!(normalize_url(bad, false).is_err(), "{bad:?}");
        }
    }

    // A "." or ".." in the host or the path would put the repository
    // somewhere other than host/user/repo under the root, or outside the root
    // altogether, once the path is joined onto it.
    #[test]
    fn normalize_url_refuses_dot_segments() {
        for bad in [
            "github.com/../../evil",
            "example.com/u/../../../evil",
            "u/..",
            "..",
            "https://github.com/u/./r",
            "https://../u/r",
            "git@github.com:../../evil.git",
            "ssh://git@github.com/u/../../evil",
        ] {
            assert!(normalize_url(bad, false).is_err(), "{bad:?} was accepted");
        }
        // Dots inside a name are only a name.
        let u = normalize_url("u/foo..bar", false).unwrap();
        assert_eq!(rel_path_of(&u), "github.com/u/foo..bar");
        assert_eq!(
            rel_path_of(&normalize_url("u/.dotfiles", false).unwrap()),
            "github.com/u/.dotfiles"
        );
    }

    #[test]
    fn browse_url_is_https_without_the_git_bits() {
        let want = "https://github.com/x-motemen/ghq";
        for remote in [
            "https://github.com/x-motemen/ghq",
            "https://github.com/x-motemen/ghq.git",
            "ssh://git@github.com/x-motemen/ghq.git",
            "git@github.com:x-motemen/ghq.git",
        ] {
            assert_eq!(browse_url(remote).unwrap(), want, "{remote:?}");
        }
        // A git URL's port addresses the git service, not the web one.
        assert_eq!(
            browse_url("ssh://git@git.example.com:2222/g/p.git").unwrap(),
            "https://git.example.com/g/p"
        );
        assert!(browse_url("").is_err());
    }

    #[test]
    fn host_and_scp_forms() {
        for ok in ["github.com", "git.example.com:2222", "a-b.co"] {
            assert!(is_host(ok), "{ok}");
        }
        for bad in [
            "github",
            ".com",
            "x.c",
            "x.com:",
            "x.com:123456",
            "x_y.com",
            "x.c0m",
        ] {
            assert!(!is_host(bad), "{bad}");
        }
        assert_eq!(
            scp_like("git@h.com:u/r"),
            Some((Some("git"), "h.com", "u/r"))
        );
        assert_eq!(scp_like("h.com:/u/r"), Some((None, "h.com", "/u/r")));
        assert_eq!(scp_like("u/r"), None);
        assert_eq!(scp_like("h.com:"), None);
    }
}
