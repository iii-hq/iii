# subscribe

The iii.dev email signup API, deployed on iii Cloud, with double opt-in. Two functions, each bound
to a route by observing the http worker's `http::on-request` (iii 0.26):

1. `subscribe::create`, `POST /api/subscribe` with `{"email": "...", "source": "website"}`:
   - Already subscribed: sends nothing (a new source still adds its segment).
   - Otherwise: emails a confirmation link (template `RESEND_CONFIRM_TEMPLATE_ID`, which reads it
     from `{{{CONFIRM_URL}}}`). Nothing is stored yet, so a typo or someone else's address never
     reaches the list.
2. `subscribe::confirm`, `GET /api/subscribe/confirm?token=`, the link's target:
   - A good link: sends the welcome (`RESEND_TEMPLATE_ID`, once per address via
     `Idempotency-Key`), creates the contact in its source's segment, and redirects to
     `SITE_URL/subscribed`. An unsubscribed contact is re-subscribed, with no second welcome.
   - A bad, forged or expired link, or a Resend failure: redirects to `SITE_URL/subscribe-expired`,
     which asks for the email again.

The link carries the address and source, signed (HMAC-SHA256 with a key derived from
`RESEND_API_KEY`) and valid for 3 days, so no database holds pending signups. A new Resend key
retires every link sent before it.

`POST` must come from an iii.dev origin (`ALLOWED_ORIGINS`). `Origin` is easy to forge from a
script, so at most `MAX_EMAILS_PER_MINUTE` (default 20, per replica) confirmation emails go out per
minute. The answer is `{"ok": true}` whether or not the address was already on the list, a 400 for
an invalid email or source, a 403 for another origin, a 429 past the cap, or a 502 when Resend
fails (logged).

## Sources and segments

A body may name its `source` (`{"email": "...", "source": "ade"}`); without one it is `website`.
`RESEND_SEGMENTS` maps each source to the Resend segment its contacts join
(`website=bf68b570-…,ade=<segment id>`). A source it doesn't list gets a 400 `invalid_source`, so a
caller can never pick a segment by ID. A new contact joins its source's segment when it confirms; a
subscribed one is added to the segment of the source it signs up from again, with no email.

## Configuration

Set in `worker-compose.yaml`, except the key:

| Variable             | Where                                  |
| -------------------- | -------------------------------------- |
| `RESEND_API_KEY`     | iii Cloud secret (full-access key)     |
| `RESEND_SEGMENTS`    | `worker-compose.yaml`                  |
| `RESEND_TEMPLATE_ID` | `worker-compose.yaml`                  |
| `RESEND_FROM`        | `worker-compose.yaml`                  |
| `RESEND_SUBJECT`     | `worker-compose.yaml`                  |
| `RESEND_CONFIRM_TEMPLATE_ID` | `worker-compose.yaml` (template ID or alias) |
| `RESEND_CONFIRM_SUBJECT` | `worker-compose.yaml`              |
| `SITE_URL`           | `worker-compose.yaml` (default `https://iii.dev`) |
| `ALLOWED_ORIGINS`    | `worker-compose.yaml`                  |
| `MAX_EMAILS_PER_MINUTE` | Optional, default 20                |

## Develop

Bun, not pnpm: the worker is deployed on its own, so it keeps its own `package.json` and
`bun.lock` in `workers/subscribe/`, outside the pnpm workspace. Its start command installs from
that lockfile before running, both on iii Cloud and in a local compose VM (which sees only the
worker's directory).

```bash
cd workers/subscribe && bun install && bun test
RESEND_API_KEY=re_... iii compose --up   # from services/subscribe: engine + http worker + this worker
```

## Deploy

iii Cloud project on `iii-hq/iii`, environment `production`: branch `main`, root directory
`services/subscribe`, secret `RESEND_API_KEY`. A push that changes this directory deploys it.
iii.dev reaches it through CloudFront, which proxies `iii.dev/api/subscribe*` (the form and the
confirmation link) to the environment's address.
