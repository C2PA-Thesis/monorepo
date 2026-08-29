# Git and pull requests

## Branches

`main` is protected. Never push to it directly.

Work on a branch, open a pull request, get one approval. Branch names come from
the Linear issue, which links the pull request to the issue automatically.

Force pushes and branch deletion on `main` are blocked. Stale approvals are
dismissed when new commits land, so a review always covers what will merge.

## Merging

Squash and merge only. Merge commits and rebase merges are disabled.

The squash commit takes its title from the pull request title and its body from
the pull request description. That makes the description the durable record, so
write it for someone reading `git log` a year from now with no access to GitHub.

## Commit messages

Conventional commits, enforced by commitlint on every commit and on the pull
request title.

```
feat(c2pa): add capture and verify pipeline
fix(zklp): correct the scalar field modulus
docs(readme): document the PEM certificate gotcha
chore(ci): pin the c2patool version
```

Allowed scopes are the top level folders plus `docs`, `ci` and `repo`. Adding a
folder means adding its scope to `commitlint.config.mjs`.

## Signing

All commits are signed. Verify with:

```bash
git log --format='%G? %h %s'
```

`G` means a good signature. Anything else means the commit is unsigned or the
key is not trusted locally.

Signing keys live in Secretive, backed by the Secure Enclave. The key used to
sign must also be registered as a signing key on the GitHub account, otherwise
GitHub shows the commit as Unverified even though the signature is valid.

## Authorship

Commits are authored by the person who made them. Do not add co-authors for
tooling, editors or assistants.
