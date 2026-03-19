# React + TypeScript + Vite

## Development with `npm run dev` + `macc web`

Run the backend from the repository root:

```bash
cargo run -p macc-cli --bin macc -- web
```

In a second terminal, start the Vite dev server:

```bash
cd web
npm install
npm run dev
```

During development the frontend defaults to relative `/api/v1/*` URLs, so `/api/v1/status` and `/api/v1/events` flow through the proxy defined in [`vite.config.ts`](./vite.config.ts) to `http://localhost:3450`.

If you need to bypass the proxy and point the frontend directly at another backend origin, set `VITE_API_BASE_URL` before starting Vite:

```bash
cd web
VITE_API_BASE_URL=http://localhost:3450 npm run dev
```

## Production build with `macc web`

Build the SPA into `web/dist`, then start the Axum server from the repository root:

```bash
npm install
npm run build
cargo run -p macc-cli --bin macc -- web
```

The `macc web` command serves the compiled frontend on `http://localhost:3450` by default, keeps `/api/v1/*` on the same server, and falls back to `web/dist/index.html` for client-side routes.

Asset mode selection:

```bash
# Development default: serve files directly from web/dist
cargo run -p macc-cli --bin macc -- web --assets dist

# Production-style self-contained binary: serve embedded assets
cargo run -p macc-cli --bin macc -- web --assets embedded
```

You can also set `settings.web_assets: dist` or `settings.web_assets: embedded` in the canonical config. When unset, debug builds default to `dist` and release builds default to `embedded`.

This template provides a minimal setup to get React working in Vite with HMR and some ESLint rules.

Currently, two official plugins are available:

- [@vitejs/plugin-react](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react) uses [Oxc](https://oxc.rs)
- [@vitejs/plugin-react-swc](https://github.com/vitejs/vite-plugin-react/blob/main/packages/plugin-react-swc) uses [SWC](https://swc.rs/)

## React Compiler

The React Compiler is not enabled on this template because of its impact on dev & build performances. To add it, see [this documentation](https://react.dev/learn/react-compiler/installation).

## Expanding the ESLint configuration

If you are developing a production application, we recommend updating the configuration to enable type-aware lint rules:

```js
export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      // Other configs...

      // Remove tseslint.configs.recommended and replace with this
      tseslint.configs.recommendedTypeChecked,
      // Alternatively, use this for stricter rules
      tseslint.configs.strictTypeChecked,
      // Optionally, add this for stylistic rules
      tseslint.configs.stylisticTypeChecked,

      // Other configs...
    ],
    languageOptions: {
      parserOptions: {
        project: ['./tsconfig.node.json', './tsconfig.app.json'],
        tsconfigRootDir: import.meta.dirname,
      },
      // other options...
    },
  },
])
```

You can also install [eslint-plugin-react-x](https://github.com/Rel1cx/eslint-react/tree/main/packages/plugins/eslint-plugin-react-x) and [eslint-plugin-react-dom](https://github.com/Rel1cx/eslint-react/tree/main/packages/plugins/eslint-plugin-react-dom) for React-specific lint rules:

```js
// eslint.config.js
import reactX from 'eslint-plugin-react-x'
import reactDom from 'eslint-plugin-react-dom'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      // Other configs...
      // Enable lint rules for React
      reactX.configs['recommended-typescript'],
      // Enable lint rules for React DOM
      reactDom.configs.recommended,
    ],
    languageOptions: {
      parserOptions: {
        project: ['./tsconfig.node.json', './tsconfig.app.json'],
        tsconfigRootDir: import.meta.dirname,
      },
      // other options...
    },
  },
])
```
