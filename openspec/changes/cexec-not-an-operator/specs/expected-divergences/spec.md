## REMOVED Requirements

### Requirement: cexec-defined

**Reason**: `cexec` is no longer defined, so nothing diverges from the
reference converter, which also raises `undefined` for it. The
divergence's own justification — that the guarded download is native
code an `exec` of a literal string leaves untouched — held for the
string but not for the probe around it: the driver needs the error.

**Migration**: the registry entry and the `% divergence: cexec-defined`
corpus marker are removed; the identity corpus covers the probe instead.
