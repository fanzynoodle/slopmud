#!/usr/bin/env python3
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


def _wait_term(driver, needle: str, timeout_s: float = 12.0) -> str:
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


def _set_input(driver, el_id: str, value: str, timeout_s: float = 10.0):
    el = _wait_el(driver, "id", el_id, timeout_s=timeout_s)
    try:
        el.clear()
    except Exception:
        pass
    el.send_keys(value)
    return el


def _send_line(driver, s: str):
    el = driver.find_element("id", "line")
    el.send_keys(s + "\n")


def run_create_flow(driver, name: str, password: str):
    # Pick auth method before the terminal connects, then connect and use the in-terminal flow.
    # (static_web doesn't serve /api/webauth; only slopmud_web does.)
    _click_id(driver, "btn-gate-password", timeout_s=12.0)
    _click_id(driver, "btn-connect", timeout_s=12.0)

    _wait_term(driver, "name:", timeout_s=20.0)
    _send_line(driver, name)
    _wait_term(driver, "auth method", timeout_s=20.0)
    _send_line(driver, "password")
    _wait_term(driver, "password", timeout_s=20.0)  # "set password" or "password"
    _send_line(driver, password)

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


def run_password_login_flow(driver, name: str, password: str):
    _click_id(driver, "btn-gate-password", timeout_s=12.0)
    _click_id(driver, "btn-connect", timeout_s=12.0)

    _wait_term(driver, "name:", timeout_s=20.0)
    _send_line(driver, name)
    _wait_term(driver, "auth method", timeout_s=20.0)
    _send_line(driver, "password")
    _wait_term(driver, "password", timeout_s=20.0)  # should be login prompt now
    _send_line(driver, password)
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
    # Keep ports in the 49xx series but avoid colliding with any developer-run stack.
    host = "127.0.0.1"
    for base in range(4950, 4990, 2):
        broker_p = base
        shard_p = base + 1
        web_p = base + 3
        if (
            _port_free(host, broker_p)
            and _port_free(host, shard_p)
            and _port_free(host, web_p)
        ):
            return (
                f"{host}:{broker_p}",
                f"{host}:{shard_p}",
                f"{host}:{web_p}",
            )
    raise RuntimeError("no free 49xx port block found (4950..4989)")


def main():
    broker_bind, shard_bind, web_bind = _pick_ports()

    env = os.environ.copy()
    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_BIND"] = shard_bind
    env["SHARD_ADDR"] = shard_bind
    env["WORLD_TICK_MS"] = "200"

    run_id = str(time.time_ns())
    accounts_path = f"/tmp/slopmud_accounts_e2e_web_{run_id}.json"
    env["SLOPMUD_ACCOUNTS_PATH"] = accounts_path

    # Ensure the web -> broker bridge uses the same port and never defaults to :23.
    env["SESSION_TCP_ADDR"] = broker_bind

    base = Path("/tmp")
    shard_log = base / f"slopmud_e2e_web_shard_{run_id}.log"
    broker_log = base / f"slopmud_e2e_web_broker_{run_id}.log"
    web_log = base / f"slopmud_e2e_web_web_{run_id}.log"

    # Build once up front so failures are obvious.
    subprocess.check_call(["cargo", "build", "-q", "-p", "shard_01"], env=env)
    subprocess.check_call(["cargo", "build", "-q", "-p", "slopmud"], env=env)
    subprocess.check_call(["cargo", "build", "-q", "-p", "static_web"], env=env)

    shard, shard_f = _start(["target/debug/shard_01"], env=env, log_path=shard_log)
    broker = None
    web = None
    broker_f = None
    web_f = None
    ok = False

    try:
        time.sleep(0.7)
        broker, broker_f = _start(["target/debug/slopmud"], env=env, log_path=broker_log)
        time.sleep(0.7)
        web_cmd = [
            "target/debug/static_web",
            "--bind",
            web_bind,
            "--dir",
            "web_homepage",
            "--session-tcp-addr",
            broker_bind,
        ]
        web, web_f = _start(web_cmd, env=env, log_path=web_log)

        _wait_http(f"http://{web_bind}/play.html", timeout_s=10.0)

        name = "Selenium"
        pw = "pw-Selenium-1234"

        # First run: create account + character.
        prof1 = Path(f"/tmp/slopmud_selenium_profile_{run_id}_1")
        d1 = new_chrome(prof1)
        try:
            d1.get(f"http://{web_bind}/play.html")
            run_create_flow(d1, name, pw)
        finally:
            d1.quit()

        # Second run (fresh browser profile): same name should hit password login.
        prof2 = Path(f"/tmp/slopmud_selenium_profile_{run_id}_2")
        d2 = new_chrome(prof2)
        try:
            d2.get(f"http://{web_bind}/play.html")
            run_password_login_flow(d2, name, pw)
        finally:
            d2.quit()

        ok = True
        print(f"e2e web ok ({web_bind} -> {broker_bind} -> {shard_bind})")
        return 0
    except Exception:
        print(
            f"e2e web failed; logs: {web_log} {broker_log} {shard_log}",
            file=sys.stderr,
        )
        for p in [web_log, broker_log, shard_log]:
            try:
                lines = Path(p).read_text(encoding="utf-8", errors="replace").splitlines()
                tail = lines[-160:]
                print(f"--- tail {p} ---", file=sys.stderr)
                print("\n".join(tail), file=sys.stderr)
            except Exception as e:
                print(f"--- tail {p} failed: {e} ---", file=sys.stderr)
        raise
    finally:
        for proc in [web, broker, shard]:
            if proc is None:
                continue
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except Exception:
                pass
        for f in [web_f, broker_f, shard_f]:
            try:
                if f:
                    f.close()
            except Exception:
                pass


if __name__ == "__main__":
    raise SystemExit(main())
