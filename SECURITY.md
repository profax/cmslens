# Security policy

## Supported versions

Only the latest release gets fixes.

## What counts as a vulnerability

cmslens fetches addresses that users supply and parses pages it doesn't
control, and a public web service runs on it. These count as vulnerabilities:

- an address, a redirect, or a DNS answer that makes it connect to a private,
  loopback, link-local, or other reserved address;
- a page or a response that crashes it, keeps it running past its timeout, or
  makes it read past its size limit.

## Reporting a vulnerability

Please report privately, not in a public issue:

- on GitHub: [Report a vulnerability](https://github.com/profax/cmslens/security/advisories/new);
- by email: info@armilen.ru.

Include the cmslens version (`cmslens -version`) and, if you can, the address
or the response that triggers the problem.
