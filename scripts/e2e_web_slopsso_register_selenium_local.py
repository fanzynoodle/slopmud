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


def _wait_url_contains(driver, needle: str, timeout_s: float = 12.0):
    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            u = driver.current_url
            if needle in u:
                return u
        except Exception as e:
            last = e
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for url contains {needle!r} ({last})")


def _wait_url_path_endswith(driver, suffix: str, timeout_s: float = 12.0):
    import urllib.parse

    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            u = driver.current_url
            p = urllib.parse.urlparse(u).path
            if p.endswith(suffix):
                return u
        except Exception as e:
            last = e
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for url path endswith {suffix!r} ({last})")


def _set_input_name(driver, name: str, value: str, timeout_s: float = 10.0):
    el = _wait_el(driver, "name", name, timeout_s=timeout_s)
    try:
        el.clear()
    except Exception:
        pass
    el.send_keys(value)
    return el


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
    # Use a port block away from default 49xx local dev and 50xx e2e scripts.
    host = "127.0.0.1"
    for base in range(5650, 5750, 5):
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
    raise RuntimeError("no free 56xx/57xx port block found (5650..5749)")


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

    _wait_term(driver, "Orientation Wing", timeout_s=20.0)


def main():
    broker_bind, shard_bind, web_bind, oidc_bind = _pick_ports()
    oidc_issuer = f"http://{oidc_bind}"

    env = os.environ.copy()
    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_BIND"] = shard_bind
    env["SHARD_ADDR"] = shard_bind
    env["WORLD_TICK_MS"] = "200"

    run_id = str(time.time_ns())
    accounts_path = f"/tmp/slopmud_accounts_e2e_web_slopsso_reg_selenium_{run_id}.json"
    env["SLOPMUD_ACCOUNTS_PATH"] = accounts_path
    env["SESSION_TCP_ADDR"] = broker_bind

    # OIDC SSO provider (internal_oidc) with self-serve registration/reset.
    oidc_client_id = "slopmud-local"
    oidc_client_secret = "slopmud-local-secret"
    oidc_redirect_uri = f"http://{web_bind}/auth/oidc/callback"
    users_path = f"/tmp/slopmud_internal_oidc_users_empty_{run_id}.json"
    Path(users_path).write_text(json.dumps({"users": []}, indent=2) + "\n", encoding="utf-8")

    env["OIDC_BIND"] = oidc_bind
    env["OIDC_ISSUER"] = oidc_issuer
    env["OIDC_CLIENT_ID"] = oidc_client_id
    env["OIDC_CLIENT_SECRET"] = oidc_client_secret
    env["OIDC_USERS_PATH"] = users_path
    env["OIDC_ALLOWED_REDIRECT_URIS"] = oidc_redirect_uri
    env["OIDC_ALLOW_PLAINTEXT_PASSWORDS"] = "1"
    env["OIDC_ALLOW_REGISTRATION"] = "1"
    env["OIDC_ALLOW_PASSWORD_RESET"] = "1"

    env["BIND"] = web_bind
    env["STATIC_DIR"] = "web_homepage"
    env["SLOPMUD_OIDC_SSO_AUTH_URL"] = f"{oidc_issuer}/authorize"
    env["SLOPMUD_OIDC_SSO_TOKEN_URL"] = f"{oidc_issuer}/token"
    env["SLOPMUD_OIDC_SSO_USERINFO_URL"] = f"{oidc_issuer}/userinfo"
    env["SLOPMUD_OIDC_SSO_CLIENT_ID"] = oidc_client_id
    env["SLOPMUD_OIDC_SSO_CLIENT_SECRET"] = oidc_client_secret
    env["SLOPMUD_OIDC_SSO_REDIRECT_URI"] = oidc_redirect_uri

    base = Path("/tmp")
    shard_log = base / f"slopmud_e2e_slopsso_reg_selenium_shard_{run_id}.log"
    broker_log = base / f"slopmud_e2e_slopsso_reg_selenium_broker_{run_id}.log"
    web_log = base / f"slopmud_e2e_slopsso_reg_selenium_web_{run_id}.log"
    oidc_log = base / f"slopmud_e2e_slopsso_reg_selenium_oidc_{run_id}.log"

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

        prof = Path(f"/tmp/slopmud_selenium_profile_{run_id}_slopsso_reg")
        d = new_chrome(prof)
        try:
            d.get(f"http://{web_bind}/play.html")

            # Auth gate must appear before any terminal connection.
            _wait_dialog_open(d, "dlg-auth-gate", timeout_s=12.0)

            # SlopSSO button should be enabled (OIDC configured).
            _wait_enabled_id(d, "btn-gate-slopsso", timeout_s=12.0)
            _click_id(d, "btn-gate-slopsso", timeout_s=12.0)

            # We should now be on the IdP Sign in page (no mud UI).
            _wait_url_contains(d, oidc_bind, timeout_s=12.0)
            h1 = _wait_el(d, "tag name", "h1", timeout_s=12.0)
            if h1.text.strip() != "Sign in":
                raise RuntimeError(f"expected IdP h1=Sign in, got {h1.text!r} (url={d.current_url!r})")

            # Must offer conventional links.
            _wait_el(d, "link text", "Create account", timeout_s=10.0)
            _wait_el(d, "link text", "Forgot password?", timeout_s=10.0)

            # Registration flow (username + password twice).
            d.find_element("link text", "Create account").click()
            h1 = _wait_el(d, "tag name", "h1", timeout_s=12.0)
            if h1.text.strip() != "Create account":
                raise RuntimeError(f"expected IdP h1=Create account, got {h1.text!r} (url={d.current_url!r})")

            uname = f"u{run_id[-10:]}"
            pw = "pw-strong-1234"
            _set_input_name(d, "username", uname, timeout_s=10.0)
            _set_input_name(d, "password", pw, timeout_s=10.0)
            _set_input_name(d, "password2", pw, timeout_s=10.0)
            _wait_el(d, "css selector", "button[type='submit']", timeout_s=10.0).click()

            # Redirect back to slopmud_web /play.html.
            # (We may briefly hit /auth/oidc/callback before being redirected to /play.html.)
            _wait_url_contains(d, web_bind, timeout_s=14.0)
            _wait_url_path_endswith(d, "/play.html", timeout_s=14.0)

            # New flow: after OAuth return, client auto-connects and broker either:
            # - prompts for in-game name (first-time SSO link/create), or
            # - resumes directly at step 2+ for an existing link.
            deadline = time.time() + 18.0
            term = ""
            while time.time() < deadline:
                term = _term_text(d)
                if "character creation (step 2/4)" in term:
                    break
                if "\nname: " in term or term.rstrip().endswith("name:"):
                    break
                time.sleep(0.05)
            else:
                raise TimeoutError(f"timeout waiting for auto-connect prompt; got:\n{term[-2000:]}")

            if "name: name:" in term:
                raise RuntimeError(f"duplicate name prompt detected:\n{term[-2000:]}")

            if "character creation (step 2/4)" not in term:
                name = f"SsoReg{run_id[-6:]}"
                _send_line(d, name)

            run_sso_create_flow(d)

        finally:
            d.quit()

        print(
            f"e2e web slopsso registration (selenium) ok ({web_bind} -> {broker_bind} -> {shard_bind}, oidc={oidc_bind})"
        )
        return 0
    except Exception:
        print(
            f"e2e web slopsso registration (selenium) failed; logs: {web_log} {broker_log} {shard_log} {oidc_log}",
            file=sys.stderr,
        )
        for p in [web_log, broker_log, shard_log, oidc_log]:
            try:
                lines = Path(p).read_text(encoding="utf-8", errors="replace").splitlines()
                tail = lines[-220:]
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
