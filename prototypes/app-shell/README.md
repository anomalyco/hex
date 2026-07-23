# Archived App-Shell Prototype

**Status:** Historical design input. The production shell has been selected and
implemented. This prototype remains runnable for comparison, but its supporting
documents do not define current behavior or active work.

The prototype compares three earlier app-shell directions spanning Meetings,
Commands, Activity/Debug, and Settings.

Supporting material:

- [`DECISIONS.md`](DECISIONS.md): decisions made after the comparison.
- [`SYNTHESIS.md`](SYNTHESIS.md): combined design direction.
- [`ACTIVITY_DATA.md`](ACTIVITY_DATA.md): historical data audit.

```sh
bun prototypes/app-shell/serve.ts
```

Open <http://localhost:4173>. Switch variants with the floating controls or
`?variant=A`, `?variant=B`, and `?variant=C`.
