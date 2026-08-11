# Security policy

## Reporting a vulnerability

Please report security issues privately through
[GitHub's advisory form](https://github.com/kristoferlund/ratcn/security/advisories/new)
rather than opening a public issue.

You should get an acknowledgement within a few days. If a fix is needed, it will
be released and credited unless you would rather stay anonymous.

## Supported versions

ratcn is a preview release. Fixes land on the latest published version only —
there are no maintained release branches yet.

## Scope

ratcn draws terminal user interfaces and routes input events. It performs no
network access, reads no files, and executes no processes, so the realistic
surface is small. Things worth reporting:

- Input that causes a panic in the library rather than being ignored, since a
  panic in a terminal application can leave the terminal in a broken state.
- Escape sequences or untrusted text that a component renders in a way that
  lets it write outside its own area, or that could be used to manipulate the
  terminal.
- Anything in the published crate that behaves differently from what the source
  in this repository describes.
