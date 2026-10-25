# New Version Checklist

Use this checklist for each release. Asking to run it authorizes the full release:
prepare, verify, stage, commit, push, tag and publish, without a separate authorization
prompt. Failed tests or automated checks block release progression until resolved.

## Prepare

- [ ] Review the local changes and select only the work intended for this release.
      Keep unrelated changes out of the release commit.
- [ ] Check build prerequisites before running validation: Cargo, rustfmt, Clippy and
      the platform's native build tools (MSVC C++ tools and `link.exe` on Windows).
      If native tools are missing, use an available Docker/WSL environment as described
      in [Linux build on Windows](docs/linux-build-on-windows.md). Linux checks do not
      verify Windows-only code or native UI behavior.
- [ ] Increase the workspace version in `Cargo.toml` and refresh `Cargo.lock` using
      `scripts\bump-version.sh` from Git Bash or WSL. Use `patch` for each pushed
      feature, `minor` for a completed milestone, or `major` for a breaking release.
      The script refuses uncommitted changes to `Cargo.toml`; resolve those deliberately
      before running it. Finish the version bump and lockfile refresh before starting
      validation; do not run them in parallel with Cargo checks.
- [ ] Update [PLAN.md](PLAN.md): remove fully implemented and verified work and keep
      outstanding work. Move implemented items awaiting native UI checks from planned
      work into "Fixed, not tested yet", retaining their manual checks. Publishing does
      not count as verification. New limitations of built features go to
      [docs/known-limitations.md](docs/known-limitations.md).
- [ ] Update [FEATURES.md](FEATURES.md) if the intended behavior or scope changed.
      It describes the target experience, not a list of completed work.
- [ ] Update [CHANGELOG.md](CHANGELOG.md): move the released entries from Unreleased
      into a heading matching the new version, keeping an Unreleased section for future work.
- [ ] Update [README.md](README.md), including its release-notes link and any changed
      user-facing behavior or installation instructions.

## Verify

Run from the repository root after the version and documentation updates are complete,
using an environment with the required build tools:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --release --workspace --locked
```

- [ ] In the existing Docker build image, run Cargo directly or use `bash -c` for chained
      commands. Avoid `bash -lc`: a login shell can replace the image's Cargo `PATH`.
- [ ] If a check fails, inspect its diagnostic before retrying. Do not repeat other
      commands known to be blocked by the same missing prerequisite; record the blocker
      and use a supported alternative where available.
- [ ] Record verification results in the PR description, distinguishing Linux automated
      checks, native Windows checks, UI checks and packaged-app checks. Keep blocked or
      unrun checks explicitly pending.
- [ ] Complete the relevant Windows and Linux UI checks. Record unverified behavior under
      "Fixed, not tested yet" in `PLAN.md` rather than marking it done.
- [ ] Review the final diff, including version consistency and documentation accuracy.
- [ ] Stop before committing or pushing if tests or automated checks failed. A missing
      local prerequisite is a verification blocker, not a passing check; use a supported
      alternative or the CI checks below, and keep unverified native UI checks pending.

## Commit and Publish

- [ ] Stage only the intended release files and review `git diff --cached`.
- [ ] Commit with a message identifying the version and the required trailer:
      `Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>`.
- [ ] Push the release commit to the intended branch; get it onto `main` through the
      normal review process and wait for both Windows and Ubuntu workflows to pass,
      including the separate Ubuntu Clippy job.
- [ ] Tag the exact release commit as `v<version>` matching `Cargo.toml`, and push
      that specific tag only after the required workflows pass. Do not tag or publish
      a commit with failing tests or automated checks. A pushed `v*` tag automatically
      publishes a GitHub release.
- [ ] Check the tag workflows, release notes, and all three release assets:
      Windows setup executable, Windows portable ZIP, and Linux tarball.
      Check Clippy explicitly: the current release job does not wait for it.
- [ ] Smoke-test the packaged app and confirm the About dialog shows the expected
      version and commit.
