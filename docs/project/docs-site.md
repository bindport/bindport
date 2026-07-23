# Docs Site and AI Discovery

BindPort keeps canonical docs as Markdown in `docs/` so they are readable in
GitHub and can also build into a static mdBook site.

The docs site is a work in progress. The current structure is meant to be good
enough to publish and improve in place, not a final information architecture.
As BindPort grows, docs should keep moving toward clearer onboarding, deeper
examples, better reference pages, and stronger agent-readable discovery files.

## Local Preview

Install mdBook:

```sh
cargo install mdbook --locked
```

Serve locally:

```sh
scripts/docs-serve.sh
```

For a remote dev box, opt into a wider bind explicitly:

```sh
scripts/docs-serve.sh -n 0.0.0.0 -p 4321
```

Build:

```sh
scripts/docs-build.sh
```

The generated site is written to `dist/docs`.

## Navigation

Edit [SUMMARY.md](../SUMMARY.md) to change the sidebar. Pages do not need to live
in the same order as the sidebar, but the sidebar should keep user-facing setup
and daily-use docs before project-internal release material.

Keep public URLs readable. Prefer clean paths such as
`getting-started/why-bindport.html`; let mdBook's sidebar numbering carry the
reading order instead of baking numbers into page filenames.

## Static Discovery Files

The docs source includes:

- [llms.txt](../llms.txt): curated LLM entrypoint.
- [llms-full.txt](../llms-full.txt): expanded LLM context.
- [config.schema.json](../config.schema.json): v1-candidate config contract.
- [status.schema.json](../status.schema.json): status JSON contract.
- [robots.txt](../robots.txt): basic crawler policy.

The docs scripts copy these text files into `dist/docs` after mdBook writes the
site. Plain `mdbook build` does not copy unlisted `.txt` files.

## Sitemap

mdBook does not generate a sitemap by default. The build wrapper generates one
when `--base-url` is passed:

```sh
scripts/docs-build.sh --base-url https://example.com/docs/
```

When running through mise:

```sh
mise exec -- scripts/docs-build.sh --base-url https://example.com/docs/
```

The generated `robots.txt` includes the matching `Sitemap:` entry.

Until then, keep `site-url` in `book.toml` accurate for the deployment path so
mdBook's generated 404 page and relative assets work correctly.

## SEO Notes

mdBook provides a reliable static HTML baseline: semantic headings, readable
URLs, search index, print output, and GitHub source links. The BindPort build
wrapper adds page-specific meta descriptions by extracting the first content
paragraph from each rendered page.

For stronger SEO after the hosting target is known, add:

- a canonical base URL.
- `sitemap.xml` generated from the built HTML pages.
- custom social metadata only when there is a real docs domain and brand asset.

Treat this as iterative polish. The generated descriptions, sidebar labels,
landing-page copy, `llms.txt`, and sitemap behavior should keep improving as
the docs move from bootstrap coverage toward a public product documentation
site.
