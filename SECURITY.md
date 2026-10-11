# Security Policy

## Supported versions

Only the latest release gets security fixes:

[![Latest release](https://img.shields.io/github/v/release/jedipunkz/gm)](https://github.com/jedipunkz/gm/releases/latest)

gm is small and is released often, so a fix usually ships in the next patch
release rather than appearing as a backport in an older version. Upgrading
is the way to take a fix; there are no long-lived branches to chase.

## Reporting a vulnerability

Use [GitHub's private vulnerability reporting](https://github.com/jedipunkz/gm/security/advisories/new),
which is enabled on this repository. It reaches the maintainer without
opening a public issue, so a report cannot be used by anyone before the fix
is ready.

Please do not open a public issue. An issue is public on the day it is
written, and gm has dangerous paths: it moves repositories and their
worktrees around, and it deletes things, so a report that was meant to be
private is a handout to anyone running the version with the flaw. Something
that turns out not to be a vulnerability is never harmed by being a report.

Please include:

- the gm version (`gm --version`) and how it was installed,
- what you did, what you got, and what harm it reached,
- a way to see it happen: commands, or a small failing test. `cargo test`
  runs the whole suite, and a test that shows the crash moving work the
  walk never touched is the fastest way to be believed.

## What happens next

- A report is answered within about 5 days, saying whether it is accepted
  and what is being done next.
- An accepted report gets a fix and a release, and a CVE under GitHub's
  advisory flow where the severity asks for one. If a report took work, the
  version carrying the fix gives credit when the reporter wants it.
- A report that is not a vulnerability gets an explanation of what stopped
  it, so the reporter knows the reasoning, not just the answer.
