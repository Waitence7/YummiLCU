# Legacy C# agent

This WPF/C# agent is retained only for existing installations and emergency rollback.

- New installations must use `agent-tauri`.
- New releases must publish the `tauri` target in `deploy/agent-version.json`.
- Do not add new features to this directory.

The Legacy update target remains available only while existing C# installations need a migration path.
