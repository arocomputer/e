# Security Policy

## Reporting security issues

The ulo team takes security issues seriously. We appreciate your efforts to
responsibly disclose your findings and will make every effort to acknowledge
your contributions.

To report a security issue, please email
[security@aro.computer](mailto:security@aro.computer) and include the word
"SECURITY" in the subject line. Do not open a public issue for anything
security-sensitive.

We'll endeavor to respond quickly and keep you updated throughout the process.

## What is not a vulnerability

ulo runs model-directed tools as your user without a permission prompt by
default — that is the documented design (see the SAFETY section of the
[README](README.md)), not a flaw. Use a container, VM, or OS sandbox when
work needs containment.
