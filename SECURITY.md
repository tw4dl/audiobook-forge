# Security policy

## Supported versions

| Version | Supported |
| --- | --- |
| `0.1.x` | Yes |

## Reporting a vulnerability

Please do not open a public issue for a security vulnerability.

Use [GitHub private vulnerability reporting](https://github.com/tw4dl/audiobook-forge/security/advisories/new) when it is available. If private reporting is unavailable, contact the repository owner through GitHub and include `audiobook-forge security` in the subject.

Include:

- A short description and impact.
- Affected version, commit, platform, and provider.
- Reproduction steps or a minimal test input that you are allowed to share.
- Logs with book text, credentials, tokens, and local paths removed.

Do not upload books, model weights, cache directories, API keys, or personal data. We will acknowledge a report when practical, investigate it, and coordinate a fix and disclosure timeline with the reporter.

## Security boundaries

The tool processes local, untrusted document files. It does not provide a cloud service or send book text to a TTS API. DRM removal is intentionally unsupported. See [README.md](README.md) for parser limits and local-runtime behavior.
