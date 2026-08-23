# Security policy

## Supported versions

Security fixes are provided for the latest released version of nMosh. Older
releases are not actively maintained.

## Reporting a vulnerability

Do not report security vulnerabilities through public issues, discussions, or
pull requests.

Use [GitHub private vulnerability reporting](https://github.com/ponkis/nmosh/security/advisories/new)
or email **ponkis@ponkis.xyz**. Include the nMosh version, Windows version,
impact, reproduction steps, and a proof of concept when possible.

Reports involving dynamic NDI library loading, unsafe FFI boundaries, malformed
frame handling, or unintended access to local settings are in scope. Problems
in the separately installed NDI runtime itself should also be reported to its
vendor.

Please allow reasonable time to investigate and prepare a fix before publicly
disclosing a vulnerability. Credit will be given when a fix is released unless
the reporter prefers to remain anonymous.
