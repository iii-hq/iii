# subscribe

The iii.dev email signup API, deployed on iii Cloud, with double opt-in. Two functions, each bound
to a `POST` route by observing the http worker's `http::on-request` (iii 0.26), both posted to by
iii.dev pages:

1. `subscribe::create`, `POST /api/subscribe` with `{"email": "...", "source": "website"}`:
   - Already subscribed: nothing changes and nothing is sent.
   - Otherwise: emails a confirmation link (template `RESEND_CONFIRM_TEMPLATE_ID`, which reads it
     from `{{{CONFIRM_URL}}}`): `SITE_URL/subscribe-confirm#token=…`. Nothing is stored yet, so a
     typo or someone else's address never reaches the list.
2. `subscribe::confirm`, `POST /api/subscribe/confirm` with `{"token": "..."}`, from the Confirm
   button of that page:
   - A good token: sends the welcome (`RESEND_TEMPLATE_ID`, once per address via
     `Idempotency-Key`) and creates the contact in its source's segment. The page then goes to
     `/subscribed`.
   - A bad, altered or expired token is a 400 `invalid_link`; the page goes to
     `/subscribe-expired`, which asks for the email again.

A button, not the link, confirms: mail scanners that open every link in an email can't subscribe
anyone. The token rides after `#`, so it never reaches a server or CDN log.

The token is the pending signup (address, source, expiry, and whether it was sent to an
unsubscribed address) encrypted and authenticated with AES-256-GCM, under a key that is HMAC-SHA256
of a fixed label under `RESEND_API_KEY`. It hides the address, can't be forged or altered, and
expires after 3 days, so no database holds pending signups. The key derivation is one-way: a token
reveals nothing about the API key, and a new API key retires every link sent before it. Only a link
sent while an address was unsubscribed can re-subscribe it, so the link of a first signup can't
bring back someone who unsubscribed later.

An `Origin` header that is present must be an iii.dev origin (`ALLOWED_ORIGINS`), on both routes.
`subscribe` also accepts a request with no `Origin` at all: a server-to-server caller sends none,
such as ade's worker (`{"email": "...", "source": "ade"}`). Its confirmation link still opens
iii.dev, and `confirm` requires an allowed `Origin`. `Origin` is easy to forge from a
script, so at most `MAX_EMAILS_PER_MINUTE` (default 20, per replica) confirmation emails go out per
minute. `subscribe` answers `{"ok": true}` whether or not the address was already on the list, a
400 for an invalid email or source, a 403 for another origin, a 429 past the cap, or a 502 when
Resend fails (logged).

## Sources and segments

A body may name its `source` (`{"email": "...", "source": "ade"}`); without one it is `website`.
`RESEND_SEGMENTS` maps each source to the Resend segment its contacts join
(`website=bf68b570-…,ade=<segment id>`). A source it doesn't list gets a 400 `invalid_source`, so a
caller can never pick a segment by ID. A contact joins the segment of the source it first confirmed
from; signing up again from another source changes nothing, since joining another segment would
need its own consent.

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
iii.dev reaches it through CloudFront, which proxies `iii.dev/api/subscribe*` (both routes) to the
environment's address.
