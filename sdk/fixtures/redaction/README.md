# Payload redaction vectors

Shared vectors for the credentials that payload redaction finds inside URL string values of a
recorded invocation payload (`iii.payload.json`). **Every implementation runs the full set**: the
engine (`engine/src/workers/observability/redaction.rs`, on the payload's JSON text) and the Node,
Python, Rust and Go SDKs (`redact`, on the value before it is serialized). Changing the rule means
changing this file first.

`url-credentials.json` holds `{ "note", "value", "expect" }` cases. A runner redacts
`{"headers":{"location":value},"links":[value]}` and expects the same shape with `expect` in both
places, then redacts that result and expects it unchanged.

The rule: a string value that starts with `scheme://` (`scheme` is a letter followed by letters,
digits, `+`, `-` or `.`), `//` or `/` is read as a URL. The first two have an authority; a single
leading `/` starts a path, which has none.

- In `scheme://user:password@host` (or `//user:password@host`), the password becomes `[REDACTED]` and the user is kept. A
  userinfo without a `:` (`scheme://user@host`) becomes `[REDACTED]` as a whole, since a token is
  often sent as the user name (GitHub PATs, CI clone URLs). The authority ends at the first `/`,
  `?` or `#`, and the userinfo at its last `@`.
- The query runs from the first `?` to the next `#`. Each `&`-separated `name=value` whose name is
  sensitive has its value replaced by `[REDACTED]`. A name is sensitive under the engine's key
  rule, which every implementation applies to parameter names: stripped to ASCII letters and
  digits and lowercased, it ends in `token` or contains `password`, `passwd`, `secret`,
  `credential`, `authorization`, `apikey`, `privatekey`, `bearer`, `cookie`, `signature`,
  `accesstoken`, `refreshtoken` or `authtoken` (so `idToken` is sensitive and `max_tokens` is
  not). It is also sensitive when it is one of (any case) `token`, `sig`, `signature`, `policy`,
  `key-pair-id`, `awsaccesskeyid`, `x-amz-signature`, `x-amz-credential`, `x-amz-security-token`,
  `x-goog-signature` or `x-goog-credential`.
- Empty values and values that are already `[REDACTED]` are kept, so the rule is idempotent.
  Everything else (scheme, host, path, other parameters, their order, the fragment) is kept byte for
  byte.

The placeholder is not percent-encoded: it is the same `[REDACTED]` as everywhere else in a
recorded payload. The SDKs keep their older key rule for object keys; for query parameter names
they all use the engine's.

What only the engine sees, the JSON text of a payload, is tested in the engine: strings with
escapes (`\u003f` for `?`, `\/` for `/`), and payloads truncated inside a URL. A string cut inside
the authority of a URL is replaced whole, since the `@` of a userinfo may be what was cut off.
