#!/usr/bin/env python3

import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.chrome.service import Service as ChromeService
from selenium.webdriver.support.ui import WebDriverWait


def pick_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        s.listen(1)
        return int(s.getsockname()[1])


def wait_http_ok(url: str, timeout_s: float = 12.0) -> None:
    import urllib.request

    deadline = time.time() + timeout_s
    last_err = None
    while time.time() < deadline:
        try:
            with urllib.request.urlopen(url, timeout=1.0) as resp:
                body = resp.read(128)
                if resp.status == 200 and body:
                    return
        except Exception as e:
            last_err = e
        time.sleep(0.1)
    raise RuntimeError(f"timeout waiting for {url}: {last_err}")


def wait_tcp_open(addr: str, timeout_s: float = 12.0) -> None:
    host, port_s = addr.rsplit(":", 1)
    port = int(port_s)
    deadline = time.time() + timeout_s
    last_err = None
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.5):
                return
        except OSError as e:
            last_err = e
            time.sleep(0.1)
    raise RuntimeError(f"timeout waiting for tcp {addr}: {last_err}")


def kill_proc_tree(p: subprocess.Popen, name: str) -> None:
    if p is None:
        return
    try:
        os.killpg(p.pid, signal.SIGTERM)
    except Exception:
        try:
            p.terminate()
        except Exception:
            pass


def wait_for_term_contains(driver, term_el, needle: str, timeout_s: float = 15.0) -> str:
    def _has_text(_):
        try:
            s = term_el.get_attribute("textContent") or ""
        except Exception:
            return False
        return needle in s

    WebDriverWait(driver, timeout_s).until(_has_text)
    return term_el.get_attribute("textContent") or ""


def wait_for_new_term_contains(
    driver, term_el, start_len: int, needle: str, timeout_s: float = 15.0
) -> str:
    def _has_new_text(_):
        try:
            s = term_el.get_attribute("textContent") or ""
        except Exception:
            return False
        return len(s) > start_len and needle in s[start_len:]

    WebDriverWait(driver, timeout_s).until(_has_new_text)
    return term_el.get_attribute("textContent") or ""


def send_line(line_el, s: str) -> None:
    line_el.click()
    line_el.clear()
    line_el.send_keys(s)
    line_el.send_keys(Keys.ENTER)


def send_line_and_wait(
    driver, term_el, line_el, line: str, needle: str, timeout_s: float = 15.0
) -> str:
    before = len(term_el.get_attribute("textContent") or "")
    send_line(line_el, line)
    try:
        return wait_for_new_term_contains(driver, term_el, before, needle, timeout_s=timeout_s)
    except Exception as e:
        tail = (term_el.get_attribute("textContent") or "")[-2000:]
        raise RuntimeError(
            f"timed out waiting for {needle!r} after sending {line!r}; terminal tail:\n{tail}"
        ) from e


def main() -> int:
    run_id = str(time.time_ns())

    # Dedicated ports so this is safe to run while you have a local dev session up.
    ports = set()
    while len(ports) < 3:
        ports.add(pick_free_port())
    shard_port, broker_port, web_port = sorted(list(ports))

    shard_bind = f"127.0.0.1:{shard_port}"
    broker_bind = f"127.0.0.1:{broker_port}"
    web_bind = f"127.0.0.1:{web_port}"

    env = os.environ.copy()
    env["SHARD_BIND"] = shard_bind
    env["SHARD_RAFT_LOG"] = f"/tmp/slopmud_web_e2e_shard_{run_id}.jsonl"
    env["WORLD_TICK_MS"] = "200"
    env["BARTENDER_EMOTE_MS"] = "1000"

    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_ADDR"] = shard_bind
    env["RUST_BACKTRACE"] = env.get("RUST_BACKTRACE", "1")
    env["SLOPMUD_ACCOUNTS_PATH"] = f"/tmp/slopmud_accounts_web_e2e_{run_id}.json"

    env["BIND"] = web_bind
    env["SESSION_TCP_ADDR"] = broker_bind
    env["STATIC_DIR"] = "web_homepage"

    shard_log = Path(f"/tmp/slopmud_web_e2e_shard_{run_id}.log")
    broker_log = Path(f"/tmp/slopmud_web_e2e_broker_{run_id}.log")
    web_log = Path(f"/tmp/slopmud_web_e2e_web_{run_id}.log")

    shard_f = open(shard_log, "wb")
    broker_f = open(broker_log, "wb")
    web_f = open(web_log, "wb")

    shard = None
    broker = None
    web = None
    driver = None

    try:
        subprocess.check_call(
            ["cargo", "build", "-q", "-p", "shard_01", "-p", "slopmud", "-p", "slopmud_web"],
            env=env,
        )

        shard = subprocess.Popen(
            ["target/debug/shard_01"],
            env=env,
            stdout=shard_f,
            stderr=shard_f,
            start_new_session=True,
        )
        time.sleep(0.7)

        broker = subprocess.Popen(
            ["target/debug/slopmud"],
            env=env,
            stdout=broker_f,
            stderr=broker_f,
            start_new_session=True,
        )
        time.sleep(0.7)

        web = subprocess.Popen(
            ["target/debug/slopmud_web"],
            env=env,
            stdout=web_f,
            stderr=web_f,
            start_new_session=True,
        )

        wait_http_ok(f"http://{web_bind}/healthz", timeout_s=15.0)

        # Selenium (Chromium).
        opts = webdriver.ChromeOptions()
        opts.add_argument("--headless=new")
        opts.add_argument("--no-sandbox")
        opts.add_argument("--disable-dev-shm-usage")
        opts.add_argument("--window-size=1200,900")
        opts.add_argument(f"--user-data-dir=/tmp/slopmud_web_e2e_chrome_{run_id}")

        opts.binary_location = os.environ.get("CHROME_BIN", "/usr/bin/chromium")
        service = ChromeService(executable_path=os.environ.get("CHROMEDRIVER", "/usr/bin/chromedriver"))

        driver = webdriver.Chrome(service=service, options=opts)
        driver.set_page_load_timeout(20)

        url = f"http://{web_bind}/play.html"
        local_ws_url = f"ws://{web_bind}/ws"
        driver.get(url)
        driver.execute_script(
            """
            localStorage.removeItem('slopmud_resume_token');
            localStorage.setItem('slopmud_ws_url', arguments[0]);
            """,
            local_ws_url,
        )
        driver.get(url)
        WebDriverWait(driver, 10).until(
            lambda d: d.execute_script(
                "return document.getElementById('ws-url')?.value || ''"
            )
            == local_ws_url
        )

        term = driver.find_element(By.ID, "term")
        line = driver.find_element(By.ID, "line")

        # Full creation flow via web UI.
        try:
            wait_for_term_contains(driver, term, "name:", timeout_s=2.0)
        except Exception:
            driver.find_element(By.ID, "btn-gate-password").click()
            wait_for_term_contains(driver, term, "name:", timeout_s=20.0)

        # Name must be <= 20 chars and only letters/numbers/_/-.
        name = ("Sel" + run_id[-17:])[:20]
        pw = f"pw-{name}-1234"

        send_line(line, name)
        wait_for_term_contains(driver, term, "type: password | google", timeout_s=20.0)
        send_line(line, "password")

        wait_for_term_contains(driver, term, "set password", timeout_s=20.0)
        send_line(line, pw)

        wait_for_term_contains(driver, term, "type: human | bot", timeout_s=20.0)
        send_line(line, "human")

        wait_for_term_contains(driver, term, "type: agree", timeout_s=20.0)
        send_line(line, "agree")

        wait_for_term_contains(driver, term, "code of conduct:", timeout_s=20.0)
        wait_for_term_contains(driver, term, "type: agree", timeout_s=20.0)
        send_line(line, "agree")

        wait_for_term_contains(driver, term, "choose race:", timeout_s=20.0)
        send_line(line, "race human")

        wait_for_term_contains(driver, term, "choose class:", timeout_s=20.0)
        send_line(line, "class fighter")

        wait_for_term_contains(driver, term, "sex:", timeout_s=20.0)
        send_line(line, "none")

        wait_for_term_contains(driver, term, f"hi {name}", timeout_s=30.0)

        out = send_line_and_wait(driver, term, line, "who", "who:", timeout_s=20.0)
        if name not in out:
            raise RuntimeError("did not see our name in who output")

        # Live-upgrade regression: a shard-only restart must not strand an attached web session.
        # The browser, slopmud_web, and broker stay up; only the shard process is replaced.
        send_line_and_wait(
            driver, term, line, "sigh", f"* {name} sighs", timeout_s=20.0
        )

        disconnects_before_restart = (term.get_attribute("textContent") or "").count(
            "# disconnected:"
        )
        restart_started = time.monotonic()
        kill_proc_tree(shard, "shard")
        try:
            shard.wait(timeout=5.0)
        except Exception:
            try:
                os.killpg(shard.pid, signal.SIGKILL)
            except Exception:
                pass
            shard.wait(timeout=5.0)

        shard = subprocess.Popen(
            ["target/debug/shard_01"],
            env=env,
            stdout=shard_f,
            stderr=shard_f,
            start_new_session=True,
        )
        wait_tcp_open(shard_bind, timeout_s=20.0)
        shard_listen_ms = int((time.monotonic() - restart_started) * 1000)

        # Broker reconnect is async. Keep the same web page and prove a new command reaches
        # the restarted shard without user re-auth or page reload.
        live_deadline = time.time() + 25.0
        last = ""
        live_recovery_ms = None
        while time.time() < live_deadline:
            before_retry = len(term.get_attribute("textContent") or "")
            send_line(line, "sigh")
            try:
                last = wait_for_new_term_contains(
                    driver, term, before_retry, f"* {name} sighs", timeout_s=2.0
                )
                live_recovery_ms = int((time.monotonic() - restart_started) * 1000)
                break
            except Exception:
                last = term.get_attribute("textContent") or ""
                time.sleep(0.5)
        else:
            raise RuntimeError(
                "web session did not survive shard restart; last terminal tail:\n"
                + last[-2000:]
            )
        disconnects_after_restart = (term.get_attribute("textContent") or "").count(
            "# disconnected:"
        )
        if disconnects_after_restart != disconnects_before_restart:
            raise RuntimeError(
                "browser websocket disconnected during shard restart; terminal tail:\n"
                + (term.get_attribute("textContent") or "")[-2000:]
            )

        # Verify session survives a page reload (resume token).
        driver.refresh()
        term = driver.find_element(By.ID, "term")
        line = driver.find_element(By.ID, "line")

        wait_for_term_contains(driver, term, "# connected:", timeout_s=20.0)
        out2 = send_line_and_wait(driver, term, line, "who", "who:", timeout_s=20.0)
        if name not in out2:
            raise RuntimeError("after reload, did not see our name in who output (session not resumed?)")

        print(
            "e2e web live-upgrade ok "
            f"({web_bind} -> {broker_bind} -> {shard_bind}); "
            f"shard_listen_ms={shard_listen_ms} "
            f"command_recovery_ms={live_recovery_ms}"
        )
        return 0

    except Exception as e:
        if driver is not None:
            try:
                shot = Path(f"/tmp/slopmud_web_e2e_screenshot_{run_id}.png")
                driver.save_screenshot(str(shot))
                print(f"screenshot: {shot}")
            except Exception:
                pass
        print(f"ERROR: {e}")
        print(f"logs:\n - shard: {shard_log}\n - broker: {broker_log}\n - web: {web_log}")
        return 1

    finally:
        if driver is not None:
            try:
                driver.quit()
            except Exception:
                pass

        for p, n in [(web, "web"), (broker, "broker"), (shard, "shard")]:
            if p is not None:
                kill_proc_tree(p, n)

        for p in [web, broker, shard]:
            if p is not None:
                try:
                    p.wait(timeout=3.0)
                except Exception:
                    try:
                        os.killpg(p.pid, signal.SIGKILL)
                    except Exception:
                        pass

        try:
            shard_f.close()
        except Exception:
            pass
        try:
            broker_f.close()
        except Exception:
            pass
        try:
            web_f.close()
        except Exception:
            pass


if __name__ == "__main__":
    raise SystemExit(main())
