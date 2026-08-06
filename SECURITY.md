# Security Policy

## Supported versions

OxiText follows the COOLJAPAN ecosystem's rolling-release model. Only
the **latest released 0.x line** is supported with security fixes.
Older 0.x releases do not receive backported patches; please upgrade
to the latest release before reporting an issue to confirm it is
still reproducible.

## Reporting a vulnerability

Please **do not** file a public GitHub issue for a suspected security
vulnerability. Instead, report it privately by emailing:

**info@kitasan.io**

Include as much detail as you can: affected crate and version, a
minimal reproduction (code or test case), the observed impact, and —
if known — a suggested fix or mitigation.

Reports are triaged privately by the maintainer. We will acknowledge
receipt as soon as practical, investigate, and coordinate a fix and
disclosure timeline with the reporter before any public disclosure.

## Scope

This policy covers the OxiText crates published from this repository
(https://github.com/cool-japan/oxitext): `oxitext`, `oxitext-core`,
`oxitext-shape`, `oxitext-layout`, `oxitext-raster`, `oxitext-sdf`,
`oxitext-icu`, and `oxitext-swash`. Vulnerabilities in upstream
dependencies should be reported to those projects directly, though we
welcome a heads-up so we can track and update our dependency pins.

**`oxitext-swash` is vendored third-party code carrying inherited
`unsafe`.** It is a fork of `swash` 0.2.10 by Chad Brokaw and is the
only crate in this workspace without `#![forbid(unsafe_code)]`: it
inherits 54 `unsafe` sites (37 blocks, 17 `unsafe fn`) across 8 files,
essentially all unchecked reads over untrusted font bytes in the table
parsers. None of that code was written or rewritten by OxiText — see
`crates/oxitext-swash/PROVENANCE.md` for the per-file inventory, the
known-unfixed upstream defects it carries (dfrg/swash #105, #123–#126,
#133), and the 0.2.3 de-unsafing backlog. Upstream is dormant, so
there is no upstream to escalate to; report such findings to us. Treat
font data reaching that crate as untrusted input, and note that
`fuzz/fuzz_targets/shape_untrusted_font.rs` exercises the shaping half
of it.

## Maintainer

COOLJAPAN OU (Team Kitasan)
