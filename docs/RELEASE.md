# Release Process

This project uses Semantic Versioning (`MAJOR.MINOR.PATCH`) and Git tags (`vX.Y.Z`).

## Versioning Rules

- `PATCH`: backward-compatible fixes and minor internal improvements.
- `MINOR`: backward-compatible features.
- `MAJOR`: breaking changes.

## Release Checklist

1. Ensure CI is green on `main`.
2. Bump versions and promote `CHANGELOG.md` (`[Unreleased]` -> `[X.Y.Z]`):
   - `make release-bump VERSION=X.Y.Z`
   - `make check-version TAG_VERSION=vX.Y.Z`
3. Run quality baseline locally:
   - `make check`
   - `make test-contract`
   - `./automat/tests/run.sh`
4. Commit release prep:
   - `chore(release): vX.Y.Z`
5. Tag:
   - `git tag vX.Y.Z`
   - `git push origin vX.Y.Z`
6. Create GitHub Release:
   - title: `vX.Y.Z`
   - notes: from `CHANGELOG.md` section for `vX.Y.Z` (plus generated notes if desired).

## Hotfix Flow

For urgent fixes:

1. Branch from last release tag.
2. Apply fix + tests.
3. Release as next patch version.
4. Merge back into `main`.

## Changelog Policy

- Keep entries user-visible and action-oriented.
- Group by `Added`, `Changed`, `Fixed`, `Removed`, `Security` where relevant.
- Avoid duplicate bullets between sections.
