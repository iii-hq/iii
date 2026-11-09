# subscribe

The iii.dev email signup API, deployed on iii Cloud. One function, `subscribe::create`, on
`POST /api/subscribe`:

1. Looks the address up in Resend Contacts.
2. New address: creates the contact and sends the welcome template.
3. Known address: sends nothing (an earlier unsubscribe is cleared), so nobody gets the welcome
   twice. The send also carries an `Idempotency-Key`, so a double submit can't send two.

Requests must come from an iii.dev origin (`ALLOWED_ORIGINS`). The answer is `{"ok": true}`, a 400
for an invalid email, a 403 for another origin, or a 502 when Resend fails (logged).

## Configuration

Set in `worker-compose.yaml`, except the key:

| Variable             | Where                                  |
| -------------------- | -------------------------------------- |
| `RESEND_API_KEY`     | iii Cloud secret (full-access key)     |
| `RESEND_SEGMENT_ID`  | Optional iii Cloud variable            |
| `RESEND_TEMPLATE_ID` | `worker-compose.yaml`                  |
| `RESEND_FROM`        | `worker-compose.yaml`                  |
| `RESEND_SUBJECT`     | `worker-compose.yaml`                  |
| `ALLOWED_ORIGINS`    | `worker-compose.yaml`                  |

## Develop

Bun, not pnpm: this directory is deployed on its own (iii Cloud installs at the environment's root
directory), so it keeps its own `bun.lock` outside the pnpm workspace.

```bash
bun install
bun test
RESEND_API_KEY=re_... iii compose --up   # engine + http worker + this worker
```

## Deploy

iii Cloud project on `iii-hq/iii`, environment `production`: branch `main`, root directory
`services/subscribe`, secret `RESEND_API_KEY`. A push that changes this directory deploys it.
iii.dev reaches it through CloudFront, which proxies `iii.dev/api/subscribe` to the environment's
address.
