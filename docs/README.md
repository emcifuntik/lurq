# Documentation maintenance

The guides in `src/content/docs` describe lurq **0.53.0**. The site uses Astro Starlight and deploys under `/lurq/` on GitHub Pages. `astro.config.mjs` controls the sidebar; `src/content/docs/index.md` is the guide index.

## Build and check

Use Node.js 22.12+ and Yarn 1.22.22. The link checker requires Python 3.9+.

```powershell
cd docs
yarn install --frozen-lockfile
yarn build
yarn check:links
```

For development, run `yarn dev`. For the built site, run `yarn preview` and open `/lurq/`.

`yarn check:links` checks the generated HTML's internal targets and anchors. From a guide at `/lurq/components/`, a sibling guide is `../ctx/`, not `./ctx/`. The site index can use `./getting-started/`. External URLs are outside this check.

Astro renders Rust fences but does not compile them. Check complete examples against the crate with the documented features, and run Rustdoc checks from the repository root:

```powershell
cargo test -p lurq --all-features --doc --locked
cargo doc -p lurq --all-features --no-deps --locked
git diff --check
```

Ignored Rustdoc examples are not validated by `cargo test --doc`; guide fragments may also require surrounding application types and state.

## Sources of truth

- Version, features, and dependency combinations: [crate manifest](../crates/lurq/Cargo.toml).
- Public API and behavior: [source](../crates/lurq/src) and [regression tests](../crates/lurq/tests).
- Demo commands: [demo manifest](../crates/demo/Cargo.toml) and [entry point](../crates/demo/src/main.rs).
- Release checks and publishing: [publish workflow](../.github/workflows/publish-crates.yml).
- Deployment: [docs workflow](../.github/workflows/deploy-docs.yml).

When releasing, update installation snippets in the README and guides together. `devtools` and `perf_profile` are independent features; desktop examples must select a render engine explicitly.

## Historical material

`migration-v0-13.md`, dated `text-*.md` reports, [design proposals](../design), [bug reports](../bugreports), and the [changelog](../CHANGELOG.md) retain their original context. Update their status and links without rewriting old benchmark numbers or treating old test counts as current validation. Local artifact paths in those records are provenance, not files included in a fresh checkout.

The text benchmarks include the root README as a fixture. Preserve its contents across before/after implementation comparisons and record the fixture hash or size; editing documentation can change that workload.
