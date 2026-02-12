#!/usr/bin/env python3
import json
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path


def _wait_http(url: str, timeout_s: float = 8.0) -> None:
    import urllib.request

    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            req = urllib.request.Request(url, method="HEAD")
            with urllib.request.urlopen(req, timeout=1.0) as resp:
                if 200 <= resp.status < 500:
                    return
        except Exception as e:
            last = e
            time.sleep(0.1)
    raise RuntimeError(f"timeout waiting for http: {url} ({last})")


def _http_json(method: str, url: str, obj, timeout_s: float = 6.0):
    import urllib.request

    data = json.dumps(obj).encode("utf-8")
    req = urllib.request.Request(url, method=method, data=data)
    req.add_header("content-type", "application/json")
    with urllib.request.urlopen(req, timeout=timeout_s) as resp:
        raw = resp.read()
    return json.loads(raw.decode("utf-8"))


def _start(cmd, env, log_path: Path):
    f = open(log_path, "wb")
    p = subprocess.Popen(
        cmd,
        env=env,
        stdout=f,
        stderr=f,
        start_new_session=True,
    )
    return p, f


def _port_free(host: str, port: int) -> bool:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.bind((host, port))
        return True
    except OSError:
        return False
    finally:
        try:
            s.close()
        except Exception:
            pass


def _pick_ports():
    # Keep away from other local stacks; use a 54xx block.
    host = "127.0.0.1"
    for base in range(5450, 5490, 5):
        broker_p = base
        shard_p = base + 1
        web_p = base + 2
        oidc_p = base + 4
        if (
            _port_free(host, broker_p)
            and _port_free(host, shard_p)
            and _port_free(host, web_p)
            and _port_free(host, oidc_p)
        ):
            return (
                f"{host}:{broker_p}",
                f"{host}:{shard_p}",
                f"{host}:{web_p}",
                f"{host}:{oidc_p}",
            )
    raise RuntimeError("no free 54xx port block found (5450..5489)")


def _wait_oauth_identity(web_bind: str, resume: str, provider: str, timeout_s: float = 12.0):
    import urllib.request
    import urllib.parse

    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            qs = urllib.parse.urlencode({"resume": resume})
            with urllib.request.urlopen(
                f"http://{web_bind}/api/oauth/status?{qs}", timeout=2.0
            ) as resp:
                d = json.loads(resp.read().decode("utf-8"))
            ident = (d or {}).get("identity")
            if ident and ident.get("provider") == provider:
                return ident
        except Exception as e:
            last = e
        time.sleep(0.1)
    raise TimeoutError(f"timeout waiting for oauth identity ({last})")


def _follow_location(opener, loc: str, timeout_s: float = 10.0):
    import urllib.request

    with opener.open(urllib.request.Request(loc, method="GET"), timeout=timeout_s) as resp:
        _ = resp.read()


def _oidc_register_via_http(oauth_url: str, username: str, password: str, timeout_s: float = 10.0):
    # Complete the /register flow inside internal_oidc without using a Selenium popup.
    # After registration it should redirect to slopmud_web's callback URL, which stores identity.
    import urllib.error
    import urllib.parse
    import urllib.request

    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor())

    with opener.open(oauth_url, timeout=timeout_s) as resp:
        final_url = resp.geturl()

    u = urllib.parse.urlparse(final_url)
    if not u.path.endswith("/authorize"):
        raise RuntimeError(f"expected /authorize, got {final_url!r}")

    q = urllib.parse.parse_qs(u.query)
    get1 = lambda k: (q.get(k) or [""])[0]

    form = {
        "username": username,
        "password": password,
        "password2": password,
        "response_type": get1("response_type"),
        "client_id": get1("client_id"),
        "redirect_uri": get1("redirect_uri"),
        "state": get1("state"),
        "code_challenge": get1("code_challenge"),
        "code_challenge_method": get1("code_challenge_method"),
    }
    scope = get1("scope")
    if scope:
        form["scope"] = scope

    reg_url = f"{u.scheme}://{u.netloc}/register"
    data = urllib.parse.urlencode(form).encode("utf-8")
    req = urllib.request.Request(reg_url, data=data, method="POST")
    req.add_header("content-type", "application/x-www-form-urlencoded")

    try:
        with opener.open(req, timeout=timeout_s) as resp:
            _ = resp.read()
            return
    except urllib.error.HTTPError as e:
        # internal_oidc uses temporary redirects; force the callback to be a GET.
        if e.code not in (301, 302, 303, 307, 308):
            raise
        loc = e.headers.get("Location")
        if not loc:
            raise
        _follow_location(opener, loc, timeout_s=timeout_s)


def main():
    broker_bind, shard_bind, web_bind, oidc_bind = _pick_ports()

    env = os.environ.copy()
    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_BIND"] = shard_bind
    env["SHARD_ADDR"] = shard_bind
    env["WORLD_TICK_MS"] = "200"
    env["SESSION_TCP_ADDR"] = broker_bind

    run_id = str(time.time_ns())
    env["SLOPMUD_ACCOUNTS_PATH"] = f"/tmp/slopmud_accounts_e2e_web_slopsso_reg_{run_id}.json"

    # OIDC SSO provider (internal_oidc).
    oidc_issuer = f"http://{oidc_bind}"
    oidc_client_id = "slopmud-local"
    oidc_client_secret = "slopmud-local-secret"
    oidc_redirect_uri = f"http://{web_bind}/auth/oidc/callback"

    users_path = f"/tmp/slopmud_internal_oidc_users_reg_{run_id}.json"
    Path(users_path).write_text(json.dumps({"users": []}, indent=2) + "\n", encoding="utf-8")

    env["OIDC_BIND"] = oidc_bind
    env["OIDC_ISSUER"] = oidc_issuer
    env["OIDC_CLIENT_ID"] = oidc_client_id
    env["OIDC_CLIENT_SECRET"] = oidc_client_secret
    env["OIDC_USERS_PATH"] = users_path
    env["OIDC_ALLOWED_REDIRECT_URIS"] = oidc_redirect_uri
    env["OIDC_ALLOW_PLAINTEXT_PASSWORDS"] = "1"
    env["OIDC_ALLOW_REGISTRATION"] = "1"

    env["BIND"] = web_bind
    env["STATIC_DIR"] = "web_homepage"
    env["SLOPMUD_OIDC_SSO_AUTH_URL"] = f"{oidc_issuer}/authorize"
    env["SLOPMUD_OIDC_SSO_TOKEN_URL"] = f"{oidc_issuer}/token"
    env["SLOPMUD_OIDC_SSO_USERINFO_URL"] = f"{oidc_issuer}/userinfo"
    env["SLOPMUD_OIDC_SSO_CLIENT_ID"] = oidc_client_id
    env["SLOPMUD_OIDC_SSO_CLIENT_SECRET"] = oidc_client_secret
    env["SLOPMUD_OIDC_SSO_REDIRECT_URI"] = oidc_redirect_uri

    base = Path("/tmp")
    shard_log = base / f"slopmud_e2e_web_slopsso_reg_shard_{run_id}.log"
    broker_log = base / f"slopmud_e2e_web_slopsso_reg_broker_{run_id}.log"
    web_log = base / f"slopmud_e2e_web_slopsso_reg_web_{run_id}.log"
    oidc_log = base / f"slopmud_e2e_web_slopsso_reg_oidc_{run_id}.log"

    subprocess.check_call(["cargo", "build", "-q", "-p", "shard_01"], env=env)
    subprocess.check_call(["cargo", "build", "-q", "-p", "slopmud"], env=env)
    subprocess.check_call(["cargo", "build", "-q", "-p", "slopmud_web"], env=env)
    subprocess.check_call(["cargo", "build", "-q", "-p", "internal_oidc"], env=env)

    shard, shard_f = _start(["target/debug/shard_01"], env=env, log_path=shard_log)
    broker = None
    web = None
    oidc = None
    broker_f = None
    web_f = None
    oidc_f = None

    try:
        time.sleep(0.7)
        broker, broker_f = _start(["target/debug/slopmud"], env=env, log_path=broker_log)
        time.sleep(0.7)
        oidc, oidc_f = _start(["target/debug/internal_oidc"], env=env, log_path=oidc_log)
        time.sleep(0.7)
        web_cmd = [
            "target/debug/slopmud_web",
            "--bind",
            web_bind,
            "--dir",
            "web_homepage",
            "--session-tcp-addr",
            broker_bind,
        ]
        web, web_f = _start(web_cmd, env=env, log_path=web_log)

        _wait_http(f"http://{web_bind}/play.html", timeout_s=10.0)
        _wait_http(f"{oidc_issuer}/.well-known/openid-configuration", timeout_s=10.0)

        # Resume tokens are opaque; just keep it URL safe.
        resume = "r" + run_id[-31:]

        start = _http_json(
            "POST",
            f"http://{web_bind}/api/oauth/start",
            {"provider": "oidc", "resume": resume, "return_to": "/play.html"},
        )
        if not (isinstance(start, dict) and start.get("type") == "ok" and start.get("url")):
            raise RuntimeError(f"oauth start failed: {start!r}")
        oauth_url = start["url"]
        if oauth_url.startswith("/"):
            oauth_url = f"http://{web_bind}{oauth_url}"

        _oidc_register_via_http(oauth_url, "bob", "pw-bob-1234", timeout_s=12.0)
        _wait_oauth_identity(web_bind, resume, "oidc", timeout_s=16.0)

        print(f"e2e web slopsso registration ok ({web_bind}, oidc={oidc_bind})")
        return 0
    except Exception:
        print(
            f"e2e web slopsso registration failed; logs: {web_log} {broker_log} {shard_log} {oidc_log}",
            file=sys.stderr,
        )
        for p in [web_log, broker_log, shard_log, oidc_log]:
            try:
                lines = Path(p).read_text(encoding="utf-8", errors="replace").splitlines()
                tail = lines[-160:]
                print(f"--- tail {p} ---", file=sys.stderr)
                print("\n".join(tail), file=sys.stderr)
            except Exception as e:
                print(f"--- tail {p} failed: {e} ---", file=sys.stderr)
        raise
    finally:
        for proc in [web, oidc, broker, shard]:
            if proc is None:
                continue
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except Exception:
                pass
        for f in [web_f, oidc_f, broker_f, shard_f]:
            try:
                if f:
                    f.close()
            except Exception:
                pass


if __name__ == "__main__":
    raise SystemExit(main())

