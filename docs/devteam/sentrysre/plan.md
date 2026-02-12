# SENTRY-SRE-9 Plan

## Next

- Stand up external uptime checks for:
  - `https://slopmud.com/healthz`
  - `https://www.slopmud.com/healthz`
- Add a second-tier check for partial failure detection:
  - `https://slopmud.com/api/online`
- Define alert routing (who gets paged) and a tiny runbook oriented around cloud ops: verify DNS, verify TLS expiry, verify service status, verify ports, verify rate limits.

## Later

- Track certificate expiry and alert before it matters.
- Add basic latency + error-rate tracking for the homepage endpoints.
- Establish "what is healthy" for the game path vs the static site path.
- Add a "local dev -> cloud ops" loop:
  - anything built locally must end as a cloud-safe check, automation, or runbook step.
