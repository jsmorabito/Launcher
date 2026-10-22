---
order: 1
---

## 2. Prerequisites & Environment Setup

### Runtime requirements

| Requirement | Minimum Version          |
| ----------- | ------------------------ |
| Node.js     | 18 or later              |
| pnpm        | 8 or later (recommended) |
| Launcher app   | Installed and running    |

### Install the Launcher CLI

The `launcher-sdk` npm package provides both the runtime SDK and the `launcher` CLI used throughout your entire development workflow.

```bash
npm install -g launcher-sdk
```

Verify installation:

```bash
launcher --version
```

### Per-project dependencies

Every extension project needs at minimum:

```bash
pnpm add launcher-sdk svelte
pnpm add -D vite @sveltejs/vite-plugin-svelte typescript
```

The CLI scaffolder installs these automatically. You only need to run this manually if you are setting up a project by hand.
