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


def _term_text(driver):
    return driver.execute_script("return document.getElementById('term')?.textContent || ''")


def _wait_term(driver, needle: str, timeout_s: float = 14.0) -> str:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        t = _term_text(driver)
        if needle in t:
            return t
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for {needle!r}; got:\n{_term_text(driver)[-2000:]}")


def _wait_el(driver, by: str, value: str, timeout_s: float = 10.0):
    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            return driver.find_element(by, value)
        except Exception as e:
            last = e
            time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for element {by}={value!r} ({last})")


def _click_id(driver, el_id: str, timeout_s: float = 10.0):
    el = _wait_el(driver, "id", el_id, timeout_s=timeout_s)
    el.click()
    return el


def _wait_enabled_id(driver, el_id: str, timeout_s: float = 10.0):
    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            el = driver.find_element("id", el_id)
            disabled = el.get_attribute("disabled")
            if disabled is None:
                return el
        except Exception as e:
            last = e
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for enabled element id={el_id!r} ({last})")


def _wait_dialog_open(driver, dlg_id: str, timeout_s: float = 10.0):
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        try:
            is_open = driver.execute_script(
                "return !!document.getElementById(arguments[0])?.open;", dlg_id
            )
            if is_open:
                return
        except Exception:
            pass
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for dialog open: {dlg_id!r}")


def _send_line(driver, s: str):
    el = driver.find_element("id", "line")
    el.send_keys(s + "\n")


def new_chrome(user_data_dir: Path):
    from selenium import webdriver
    from selenium.webdriver.chrome.options import Options
    from selenium.webdriver.chrome.service import Service

    o = Options()
    # Selenium Manager isn't available in this environment; point at system Chromium + ChromeDriver.
    o.binary_location = "/usr/bin/chromium"
    o.add_argument("--headless=new")
    o.add_argument("--no-sandbox")
    o.add_argument("--disable-dev-shm-usage")
    o.add_argument(f"--user-data-dir={user_data_dir}")
    o.add_argument("--window-size=1200,900")

    return webdriver.Chrome(service=Service("/usr/bin/chromedriver"), options=o)


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
    # Use a port block away from the default 49xx local dev stack and the existing selenium e2e.
    # This avoids flaky collisions if multiple e2e scripts run concurrently.
    host = "127.0.0.1"
    for base in range(5050, 5090, 5):
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
    raise RuntimeError("no free 50xx port block found (5050..5089)")


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
                raw = resp.read().decode("utf-8")
            d = json.loads(raw)
            ident = (d or {}).get("identity")
            if ident and ident.get("provider") == provider:
                return ident
        except Exception as e:
            last = e
        time.sleep(0.1)
    raise TimeoutError(f"timeout waiting for oauth identity ({last})")


def _oidc_login_via_http(oauth_url: str, username: str, password: str, timeout_s: float = 10.0):
    # Complete the internal_oidc login flow without using a second Selenium browser.
    # This keeps the test deterministic and avoids popup/window quirks in headless mode.
    import urllib.parse
    import urllib.request

    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor())

    # Follow the slopmud_web redirect to the IdP /authorize URL and scrape required query params.
    with opener.open(oauth_url, timeout=timeout_s) as resp:
        final_url = resp.geturl()

    u = urllib.parse.urlparse(final_url)
    if not u.path.endswith("/authorize"):
        raise RuntimeError(f"expected /authorize, got {final_url!r}")

    q = urllib.parse.parse_qs(u.query)
    get1 = lambda k: (q.get(k) or [""])[0]
    response_type = get1("response_type")
    client_id = get1("client_id")
    redirect_uri = get1("redirect_uri")
    code_challenge = get1("code_challenge")
    code_challenge_method = get1("code_challenge_method")
    state = get1("state")
    scope = get1("scope") or None

    if not (response_type and client_id and redirect_uri and code_challenge and code_challenge_method and state):
        raise RuntimeError(f"missing authorize params in {final_url!r}")

    form = {
        "username": username,
        "password": password,
        "response_type": response_type,
        "client_id": client_id,
        "redirect_uri": redirect_uri,
        "state": state,
        "code_challenge": code_challenge,
        "code_challenge_method": code_challenge_method,
    }
    if scope is not None:
        form["scope"] = scope

    post_url = f"{u.scheme}://{u.netloc}{u.path}"
    data = urllib.parse.urlencode(form).encode("utf-8")
    req = urllib.request.Request(post_url, data=data, method="POST")
    req.add_header("content-type", "application/x-www-form-urlencoded")
    # Follow redirects through the callback; side effect is that slopmud_web stores identity.
    import urllib.error

    try:
        with opener.open(req, timeout=timeout_s) as resp:
            _ = resp.read()
            return
    except urllib.error.HTTPError as e:
        # internal_oidc uses axum Redirect::temporary which is HTTP 307 (preserves method).
        # For our test we just need the callback GET to happen, so follow Location manually.
        if e.code not in (301, 302, 303, 307, 308):
            raise
        loc = e.headers.get("Location")
        if not loc:
            raise
        with opener.open(loc, timeout=timeout_s) as resp:
            _ = resp.read()
        return


def run_sso_create_flow(driver):
    # WEB_AUTH fast path starts at step 2.
    _wait_term(driver, "type: human | bot", timeout_s=20.0)
    _send_line(driver, "human")

    _wait_term(driver, "type: agree", timeout_s=20.0)
    _send_line(driver, "agree")

    _wait_term(driver, "code of conduct:", timeout_s=20.0)
    _wait_term(driver, "type: agree", timeout_s=20.0)
    _send_line(driver, "agree")

    _wait_term(driver, "choose race", timeout_s=20.0)
    _send_line(driver, "race human")
    _wait_term(driver, "choose class", timeout_s=20.0)
    _send_line(driver, "class fighter")
    _wait_term(driver, "sex:", timeout_s=20.0)
    _send_line(driver, "none")

    # Attached to shard: should render the start room.
    _wait_term(driver, "Orientation Wing", timeout_s=20.0)


def main():
    broker_bind, shard_bind, web_bind, oidc_bind = _pick_ports()

    env = os.environ.copy()
    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_BIND"] = shard_bind
    env["SHARD_ADDR"] = shard_bind
    env["WORLD_TICK_MS"] = "200"

    run_id = str(time.time_ns())
    accounts_path = f"/tmp/slopmud_accounts_e2e_web_sso_{run_id}.json"
    env["SLOPMUD_ACCOUNTS_PATH"] = accounts_path

    # Ensure the web -> broker bridge uses the same port and never defaults to :23.
    env["SESSION_TCP_ADDR"] = broker_bind

    # OIDC SSO provider (internal_oidc).
    oidc_issuer = f"http://{oidc_bind}"
    oidc_client_id = "slopmud-local"
    oidc_client_secret = "slopmud-local-secret"
    oidc_redirect_uri = f"http://{web_bind}/auth/oidc/callback"
    users_path = f"/tmp/slopmud_internal_oidc_users_{run_id}.json"
    Path(users_path).write_text(
        json.dumps(
            {
                "users": [
                    {
                        "username": "alice",
                        "password": "pw-alice-1234",
                        "sub": "alice-sub",
                        "email": "alice@example.com",
                        "caps": ["admin.all"],
                    }
                ]
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )

    env["OIDC_BIND"] = oidc_bind
    env["OIDC_ISSUER"] = oidc_issuer
    env["OIDC_CLIENT_ID"] = oidc_client_id
    env["OIDC_CLIENT_SECRET"] = oidc_client_secret
    env["OIDC_USERS_PATH"] = users_path
    env["OIDC_ALLOWED_REDIRECT_URIS"] = oidc_redirect_uri
    env["OIDC_ALLOW_PLAINTEXT_PASSWORDS"] = "1"

    env["BIND"] = web_bind
    env["STATIC_DIR"] = "web_homepage"
    env["SLOPMUD_OIDC_SSO_AUTH_URL"] = f"{oidc_issuer}/authorize"
    env["SLOPMUD_OIDC_SSO_TOKEN_URL"] = f"{oidc_issuer}/token"
    env["SLOPMUD_OIDC_SSO_USERINFO_URL"] = f"{oidc_issuer}/userinfo"
    env["SLOPMUD_OIDC_SSO_CLIENT_ID"] = oidc_client_id
    env["SLOPMUD_OIDC_SSO_CLIENT_SECRET"] = oidc_client_secret
    env["SLOPMUD_OIDC_SSO_REDIRECT_URI"] = oidc_redirect_uri

    base = Path("/tmp")
    shard_log = base / f"slopmud_e2e_web_sso_shard_{run_id}.log"
    broker_log = base / f"slopmud_e2e_web_sso_broker_{run_id}.log"
    web_log = base / f"slopmud_e2e_web_sso_web_{run_id}.log"
    oidc_log = base / f"slopmud_e2e_web_sso_oidc_{run_id}.log"

    # Build once up front so failures are obvious.
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
    ok = False

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

        # Main browser (game UI).
        prof1 = Path(f"/tmp/slopmud_selenium_profile_{run_id}_main")
        d1 = new_chrome(prof1)
        try:
            d1.get(f"http://{web_bind}/play.html")

            # Connect to establish the resumable session (auth gate prevents auto-connect).
            # We pick password here to avoid being redirected away by the SlopSSO button.
            _click_id(d1, "btn-gate-password", timeout_s=12.0)
            _wait_dialog_open(d1, "dlg-connect", timeout_s=12.0)
            _click_id(d1, "btn-connect", timeout_s=12.0)
            _wait_term(d1, "# connected:", timeout_s=20.0)

            # Grab resume token generated by play.js connect().
            resume = d1.execute_script("return localStorage.getItem('slopmud_resume_token') || ''")
            if not isinstance(resume, str) or len(resume.strip()) != 32:
                raise RuntimeError(f"bad resume token: {resume!r}")

            # Start OAuth (OIDC) via API, then complete the IdP login in a separate browser.
            start = _http_json(
                "POST",
                f"http://{web_bind}/api/oauth/start",
                {"provider": "oidc", "resume": resume.strip(), "return_to": "/play.html"},
            )
            if not (isinstance(start, dict) and start.get("type") == "ok" and start.get("url")):
                raise RuntimeError(f"oauth start failed: {start!r}")
            oauth_url = start["url"]
            if oauth_url.startswith("/"):
                oauth_url = f"http://{web_bind}{oauth_url}"

            _oidc_login_via_http(oauth_url, "alice", "pw-alice-1234", timeout_s=12.0)

            _wait_oauth_identity(web_bind, resume.strip(), "oidc", timeout_s=16.0)

            # Create with SSO via the same API the web UI uses. This sends a WEB_AUTH line to the
            # resumable broker session identified by `resume`.
            name = "SsoAdmin"
            wa = _http_json(
                "POST",
                f"http://{web_bind}/api/webauth",
                {"resume": resume.strip(), "action": "create", "method": "oidc", "name": name},
            )
            if not (isinstance(wa, dict) and wa.get("type") == "ok"):
                raise RuntimeError(f"webauth failed: {wa!r}")

            run_sso_create_flow(d1)

            # RBAC assertion: caps should include admin.all from OIDC userinfo -> broker -> shard.
            _send_line(d1, "caps")
            _wait_term(d1, "caps:", timeout_s=10.0)
            _wait_term(d1, "admin.all", timeout_s=10.0)

        finally:
            d1.quit()

        ok = True
        print(f"e2e web sso/rbac ok ({web_bind} -> {broker_bind} -> {shard_bind}, oidc={oidc_bind})")
        return 0
    except Exception:
        print(
            f"e2e web sso/rbac failed; logs: {web_log} {broker_log} {shard_log} {oidc_log}",
            file=sys.stderr,
        )
        for p in [web_log, broker_log, shard_log, oidc_log]:
            try:
                lines = Path(p).read_text(encoding="utf-8", errors="replace").splitlines()
                tail = lines[-180:]
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
