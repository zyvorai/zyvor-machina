# Machina docs site

Built with [Docusaurus](https://docusaurus.io/). Serves the marketing site and docs at https://zyvorai.github.io/zyvor-machina/.

## Local development

```bash
npm install
npm start
```

## Build

```bash
npm run build
npm run serve   # preview the production build locally
```

The build fails on broken internal links (`onBrokenLinks: 'throw'`).

## Images

Screenshots and cards are not duplicated into `website/static/`. `docusaurus.config.ts` serves `../docs/ux` and
`../docs/social` through `staticDirectories`, so the README and this site use the same files. Add new screenshots to
`docs/ux/` (`web/scripts/capture-readme-screens.mjs`) and cards to `docs/social/`.

## Downloads

The PDFs on `/resources` live in `website/static/resources/`. They are copies of `docs/machina-customer-feature-guide.pdf`
and `docs/customer/pdf/*.pdf`; refresh both when the guides change.

## Deployment

`.github/workflows/pages.yml` builds and publishes to GitHub Pages on every push to `main` that touches `website/`,
`docs/ux/` or `docs/social/`. Don't use Docusaurus's `deploy` script; it targets a `gh-pages` branch this repo doesn't use.
