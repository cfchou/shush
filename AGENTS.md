
# Rules

- DO NOT read anything in 'tmp/' unless user explicitly say so

- DO NOT commit unless user explicitly say so


# Guideline

- Only implement one issue at a time.

- Before implementation, 
    * Plan for TDD (use /tdd skill if available)
    
- During implementation, 
    * if spotting technical gaps, inconsistency or ambiguity in Plans:
        - If it's a big gap, stop and then present the problem to the user.
        - Otherwise, launch subagents to research, write an ADR, then continue.
    * Tick the '[ ]' box of Acceptance Criteria in the issue when one is done.
    * Always write unit tests.

- After implementation:
    * Request to update *Plans* after implementation to close any gap.


# Plans

*MUST READ*

- docs/shush-v01-implementation-plan-2026-05-28-approved.md
- docs/diagrams.md

# ADRs

ADR is for deep research and decisions, read/write if you need to.

- docs/adr/*

# Issues

Usually only need to read the issue you are dealing with.

Smaller number in the filename represents higher priority.


- docs/issues/*

# Scripts

- One-command remote E2E run with auto-stop:
    * `./scripts/run_remote_e2e.sh`
