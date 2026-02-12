# SENTRY-SRE-9 Doing

- Keep cloud-run endpoints measurable (start with `/healthz` and `/api/online`).
- Make sure smoke checks stay fast and always runnable from a dev machine (`just https-smoke prd`).
- Turn local experiments into production-safe ops improvements (alerts, limits, and runbooks).
