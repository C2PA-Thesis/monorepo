# Continuous integration

Every top level folder has its own workflow. A folder does not merge without a
check that exercises it for real.

Filter by path so a workflow only runs when its folder changes:

```yaml
on:
  push:
    paths: ['<folder>/**', '.github/workflows/<folder>.yml']
  pull_request:
    paths: ['<folder>/**', '.github/workflows/<folder>.yml']
```

The check has to fail when something breaks. A script that always exits zero is
not a check, it is decoration. Prefer a job that runs the real entry point of
the folder over one that only lints or builds.

Test the workflow logic locally before opening the pull request where it first
runs. Steps that depend on network access or on another repository will fail in
ways that are slow to debug from CI logs alone.
