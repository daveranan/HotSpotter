# Diagnostics and Privacy

Hot Trimmer is offline by default. Phase 0 does not send telemetry, crash reports, project metadata, paths,
or image content over the network.

## Local Directories

The native shell resolves these locations through the operating system at startup:

- Application data for future project coordination and settings.
- Cache data for disposable derived assets.
- Logs for local structured diagnostics.
- Recovery data for future crash-safe project snapshots.

Failure to create an approved directory blocks startup rather than falling back to the current working directory.

## Shareable Diagnostics Policy

Shareable diagnostics must not contain source pixels, generated maps, project database contents, usernames, or
absolute paths. Paths below the user's home directory are represented as `<home>/…`; other roots become
`<external-path>`. The redaction behavior is covered by a native unit test.

Phase 1 will add a bounded support-bundle command and an in-app review screen before any bundle can be shared.

