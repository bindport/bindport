# Security Policy

## Reporting A Vulnerability

Please report suspected vulnerabilities privately through GitHub Security
Advisories when available. Do not open a public issue for security-sensitive
reports.

Include enough detail to reproduce the issue, the affected version or commit,
and any known mitigations.

## Project Boundary

BindPort is a local development tool. It must not require root privileges, bind
80/443 by default, install certificates, mutate DNS, edit `/etc/hosts`, or run
a system daemon as part of its default workflow.
