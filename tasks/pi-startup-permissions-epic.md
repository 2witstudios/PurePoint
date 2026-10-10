# Native Pi startup compatibility

Point Guard must reuse normal terminal Pi installations without changing canonical credentials, settings, or sessions.

- Given an existing user-owned Pi directory readable or traversable by others but not writable by others, should start provider setup with private valid credentials and preserve directory permissions and native data.
- Given no Pi directory, should create an owner-only directory for native setup.
- Given a symlink, non-directory, foreign-owned, or group/other-writable Pi directory, should reject startup with sanitized actionable recovery and preserve existing data.
- Given insecure or malformed native credentials, should reject startup without modifying them.
- Given Point Guard managed or trust state, should continue requiring owner-only permissions.

Validation: production nativeProviderSetup boundary with disposable filesystem fixtures; mobile npm test and npm run check; scoped formatting. Packaged replacement proof also exercises an existing 0755 Pi directory with private credentials. No real credentials or installed application are modified.
