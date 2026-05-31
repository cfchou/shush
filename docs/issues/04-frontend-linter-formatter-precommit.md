# Frontend linter + formatter setup with pre-commit integration

Status: ready-for-agent

Priority: high

## What to build

Set up a consistent frontend linting and formatting toolchain, and wire it into the existing pre-commit flow so frontend changes are checked automatically before commit.

This issue is about **tooling and enforcement**, not redesigning frontend code.

**Frontend tooling:**
- Add a frontend linter appropriate for the current TypeScript/Vite stack
- Add a frontend formatter if not already covered adequately
- Prefer the smallest practical toolchain that gives reliable TS/JS/CSS coverage
- Configure the tools for the existing frontend directory structure

**package.json scripts:**
- Add explicit scripts such as:
  - `lint`
  - `format`
  - `format:check` or equivalent
- Keep script names conventional and easy to invoke locally

**Config files:**
- Add the necessary config files for the chosen linter/formatter
- Scope them so they apply to `frontend/` sources and related config files
- Ignore generated build output and dependency directories

**Pre-commit integration:**
- Update the repo's pre-commit configuration to run the frontend checks
- Make sure the new hooks run from the correct directory and fail clearly
- Prefer fast checks in pre-commit; avoid unnecessarily expensive tasks

**Validation:**
- Run the linter successfully on the current frontend code
- Run the formatter check successfully on the current frontend code
- Confirm pre-commit runs the frontend tooling as expected

## Acceptance criteria

- [x] Frontend lint command exists and passes on the current codebase
- [x] Frontend format command exists and can rewrite code consistently
- [x] Frontend format-check command exists and passes on the current codebase
- [x] Pre-commit is updated to run frontend lint/format validation
- [x] Generated frontend output is excluded from lint/format scope where appropriate
- [x] The setup is documented through config and package scripts clearly enough for routine local use

## Blocked by

- Existing frontend scaffold in `frontend/`

## References

- `frontend/package.json`
- `.pre-commit-config.yaml`
- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md`
