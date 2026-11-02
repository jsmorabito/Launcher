### 8.17 `ConnectorsService` — Accounts connected by Launcher

**Runs in:** both worker and view.

**Permission required:** `connectors:use`.

Use connectors when your extension talks to a third-party service on the
user's behalf. You declare the provider in `manifest.json`; **Launcher runs
the whole sign-in flow in Settings → Extensions** (a **Connect** button, a list
of connected accounts, **Disconnect**). Your code never opens a browser, holds a
client secret, or stores a token. It asks for the accounts and a token when it
needs to make a request.

Compare with [`OAuthService`](./oauth-service.md), where the extension drives the
flow itself. Prefer connectors for new extensions: tokens are refreshed by the
host, accounts have names, and the user manages connections in one place.

#### Declaring a connector

```jsonc
{
  "permissions": ["network", "connectors:use"],
  "connectors": [
    {
      "id": "linear",
      "name": "Linear",
      "description": "Connect each workspace you want to create issues in.",
      "required": true,
      "multiAccount": true,
      "oauth": {
        "clientId": "YOUR_PUBLIC_CLIENT_ID",
        "authorizationUrl": "https://linear.app/oauth/authorize",
        "tokenUrl": "https://api.linear.app/oauth/token",
        "scopes": ["read", "issues:create"],
      },
      "account": {
        "url": "https://api.linear.app/graphql",
        "method": "POST",
        "body": "{\"query\":\"{ organization { id name } }\"}",
        "idPath": "data.organization.id",
        "namePath": "data.organization.name",
      },
    },
  ],
}
```

| Field          | Description                                                                                                                                              |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`           | Unique in the extension, `/^[a-z][a-z0-9-]*$/`.                                                                                                          |
| `name`         | Shown in Settings and in the connect prompt.                                                                                                             |
| `required`     | When `true`, running any command of the extension first prompts the user to connect, then continues the command.                                         |
| `multiAccount` | When `true`, several accounts (e.g. workspaces) can be connected. Otherwise connecting again replaces the current account.                               |
| `oauth`        | OAuth 2.0 **PKCE** endpoints and scopes. `clientId` is public and must be hardcoded; there is no client secret.                                          |
| `account`      | One HTTPS request Launcher makes with the new token to learn who signed in. `idPath` / `namePath` are dot paths into the JSON body.                      |
| `server`       | Instead of `oauth` and `account`: a server the user supplies. See [Connecting to a server the user supplies](#connecting-to-a-server-the-user-supplies). |

Validation (by `launcher validate` and at discovery) requires HTTPS endpoints
and that `account.url` uses the same host as `oauth.authorizationUrl` or
`oauth.tokenUrl`, so a token can only be sent to the provider's own servers.

Register these redirect URLs with the provider:

- `launcher://oauth/callback` (release build)
- `launcher-dev://oauth/callback` (development build)

#### Connecting to a server the user supplies

Some services have no single address or client id: a self-hosted app, or one of
many deployments. Declare `server` instead of `oauth` and `account`:

```jsonc
{
  "permissions": ["connectors:use"],
  "connectors": [
    {
      "id": "tasks",
      "name": "Task app",
      "required": true,
      "multiAccount": true,
      "server": {
        "placeholder": "https://tasks.example.com",
        "scopes": ["tasks"],
        "clientName": "Launcher",
      },
    },
  ],
}
```

The user types the server's address once in the connect dialog. Launcher then:

1. reads `/.well-known/oauth-authorization-server` (RFC 8414) and checks the
   server supports PKCE `S256` and dynamic client registration;
2. registers itself as a public client (RFC 7591) for a one-off
   `http://127.0.0.1:<port>/oauth/callback` redirect, and listens on that
   loopback port only until the browser returns;
3. runs the usual PKCE flow and stores the tokens.

The **account is the server**: its address is `account.accountId` and
`account.serverUrl`. All endpoints must be on the address the user typed, and the
address must be `https://` (`http://` only for `localhost`, `127.0.0.1` or `[::1]`).
The manifest declares no client id because each server issues its own.

| `server` field | Description                                                  |
| -------------- | ------------------------------------------------------------ |
| `defaultUrl`   | Optional. Prefilled in the connect dialog.                   |
| `placeholder`  | Optional. Hint text for the address field.                   |
| `scopes`       | Scopes to request and register.                              |
| `clientName`   | Optional. Shown on the server's consent screen (`Launcher`). |

Use [`fetch`](#fetch) to call these servers.

#### API

```typescript
interface ConnectorAccount {
  accountId: string; // stable provider-side id
  displayName: string; // e.g. the workspace name
  serverUrl?: string; // origin of the server, for `server` connectors
}

interface IConnectorsService {
  getAccounts(connectorId: string): Promise<ConnectorAccount[]>;
  getToken(connectorId: string, accountId: string): Promise<string>;
  fetch(
    connectorId: string,
    accountId: string,
    path: string,
    options?: {
      method?: string;
      headers?: Record<string, string>;
      body?: string;
      timeout?: number;
    },
  ): Promise<{ status: number; ok: boolean; body: string }>;
  onChange(callback: (connectorId: string) => void): () => void;
}
```

```typescript
import type { IConnectorsService, INetworkService } from 'launcher-sdk/contracts';

const connectors = context.getService<IConnectorsService>('connectors');
const network = context.getService<INetworkService>('network');

const [workspace] = await connectors.getAccounts('linear');
const token = await connectors.getToken('linear', workspace.accountId);

const res = await network.fetch('https://api.linear.app/graphql', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
  body: JSON.stringify({ query: '{ viewer { name } }' }),
});
```

- `getToken()` **never opens UI**. The host refreshes an expired token with the
  stored refresh token. If that is impossible it rejects.
- `onChange()` fires when the user connects or disconnects in Settings, so an
  open view can refresh its account list.
- All calls only work for connectors **your** manifest declares.

##### `fetch`

For `server` connectors, `fetch(connectorId, accountId, path, options)` makes the
request **in Launcher** and returns `{ status, ok, body }`:

```typescript
const res = await connectors.fetch('tasks', account.accountId, '/mcp', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(rpc),
});
```

- The token is added by the host. **Your extension never sees it**, and any
  `Authorization`, `Cookie`, `Host` or similar header you pass is ignored.
- It goes only to the address the user connected. `path` must start with `/`;
  `//`, `..`, backslashes and control characters are rejected.
- Redirects are **not followed**, so a token cannot be carried to another host.
- Because the user entered the address, it can be `localhost` or a LAN address.
  `network.fetch` refuses those, so use this instead.
- `timeout` defaults to 30 s (max 60 s). Responses over 5 MB are rejected.
- A non-2xx status resolves normally; check `ok`. A `401` usually means the
  user should reconnect in Settings.

#### Errors

`getToken()` rejects with an `Error` whose message contains one of:

| Message                   | Meaning                                                                                            |
| ------------------------- | -------------------------------------------------------------------------------------------------- |
| `connector_not_connected` | No such account (never connected, or since disconnected).                                          |
| `reconnect_required`      | The token expired and could not be refreshed. Tell the user to reconnect in Settings → Extensions. |
| `connector_not_declared`  | The connector id is not in your manifest (also thrown by `getAccounts()`).                         |

#### Under the hood

- Accounts are stored in `connector_accounts` (SQLite); the tokens live in the
  encrypted `oauth_tokens` store under `connector:<connectorId>:<accountId>`.
- Uninstalling the extension deletes its accounts and tokens.
- The sign-in runs in Launcher's main window, so Settings can start it from its
  own window.
