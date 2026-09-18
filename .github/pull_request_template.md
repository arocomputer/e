<!-- Title: type(scope): what changed, in plain language.
     e.g. fix(tui): tool trees stay connected after compaction
     Scopes: core, tui, sdk, infra, docs. The title becomes the squash commit on main. -->

## Checklist

- [ ] `./x check` passes — the parity suite is the visual spec; if a rendering change fails it, fix the code, don't loosen the test
- [ ] The guard is unchanged, or this PR says why it moved (new network host, new write path, new unsafe block)
- [ ] User-visible changes and migration steps are described for the release notes
- [ ] Comments and docs touched by this change are updated
- [ ] Persisted/extension/CLI contract changes include compatibility fixtures and migration behavior
- [ ] Performance-sensitive changes pass `./x bench` and include before/after evidence when claiming an improvement
- [ ] Regression tests fail against the unfixed implementation for the intended reason
