# ADR: OAuth 2.0 (Google) for User Identity

**Status:** Proposed
**Date:** 2026-02-06
**Authors:** Kinetic Ball Team

## Context

Kinetic Ball currently has no concept of user identity. The HMAC-based API
authentication (introduced alongside this ADR) validates that requests come from
a legitimate client build, but it does not identify *who* is making the request.

As the game grows we need user accounts to support:

- **Bans & moderation** — block abusive players by identity, not IP.
- **Audit trail** — track who created/joined rooms for debugging and analytics.
- **Persistent profiles** — player names, stats, preferences tied to an account.
- **Friend lists / invites** — future social features require stable identities.

## Decision

We propose adopting **Google OAuth 2.0** as the primary identity provider, using
the Authorization Code flow adapted for native desktop clients.

### OAuth2 Flow for Desktop Clients

```
1. Client opens system browser → Google consent screen
2. User grants permission
3. Google redirects to http://localhost:<port>/callback?code=...
4. Client exchanges code for id_token + access_token
5. Client sends id_token to our server in Authorization header
6. Server validates id_token with Google's public keys (JWKS)
7. Server extracts user info (sub, email, name, picture)
```

The localhost redirect is the standard approach for native/desktop OAuth clients
(RFC 8252 — "OAuth 2.0 for Native Apps").

### Server Changes

1. **New endpoint `POST /api/auth/google`** — receives the Google `id_token`,
   validates it against Google's JWKS endpoint, and returns a session JWT.
2. **User storage** — a `users` table/map keyed by Google `sub` (subject ID),
   storing email, display name, avatar URL, created_at, banned flag.
3. **Session middleware** — validates the session JWT on protected endpoints.
   The existing HMAC middleware remains as the first layer (client authenticity);
   the session JWT is the second layer (user identity).
4. **Ban enforcement** — check the `banned` flag during session validation.

### Client Changes

1. **Login screen** — "Sign in with Google" button before room selection.
2. **Token management** — store refresh token securely on disk (OS keychain via
   `keyring` crate, falling back to an encrypted file).
3. **Authorization header** — send the session JWT on all API requests.
4. **Logout** — clear stored tokens, return to login screen.

## Alternatives Considered

| Provider | Pros | Cons |
|----------|------|------|
| **Google** | Ubiquitous, well-documented, free | Requires Google account |
| **Discord** | Gamers already have accounts | Smaller reach, API rate limits |
| **Steam** | Native to gaming | Requires Steamworks SDK integration, only covers Steam users |
| **Email/password** | No third-party dependency | Security burden (password storage, reset flow, 2FA) |
| **Anonymous + device ID** | Zero friction | No cross-device identity, easy to evade bans |

Google offers the best balance of reach, simplicity, and security for our use
case. We can add Discord or Steam as supplementary providers later.

## Incremental Adoption Path

1. **Phase 0 (current):** HMAC-only — validates client authenticity, no user
   identity.
2. **Phase 1:** Add Google OAuth as opt-in. Unauthenticated users can still
   browse rooms but cannot create or join. This lets us roll out gradually.
3. **Phase 2:** Require authentication for all gameplay actions. Anonymous
   browsing remains available.
4. **Phase 3:** Add secondary providers (Discord, Steam) based on player demand.

## Pros

- Strong, verified identities backed by Google's infrastructure.
- No password storage or reset flow to maintain.
- Standard protocol (OAuth 2.0 / OpenID Connect) with mature libraries.
- Enables moderation, bans, and future social features.

## Cons

- Adds complexity to the client (browser redirect, token storage).
- Requires internet access for login (already required for online play).
- Players must have a Google account (mitigated by adding more providers later).
- Server must periodically refresh Google's JWKS keys.

## References

- [RFC 8252 — OAuth 2.0 for Native Apps](https://datatracker.ietf.org/doc/html/rfc8252)
- [Google Identity — OAuth 2.0 for Desktop Apps](https://developers.google.com/identity/protocols/oauth2/native-app)
- [OpenID Connect Core](https://openid.net/specs/openid-connect-core-1_0.html)
