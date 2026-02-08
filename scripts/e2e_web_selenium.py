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


def send_line(line_el, s: str) -> None:
    line_el.send_keys(s)
    line_el.send_keys(Keys.ENTER)


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

        opts.binary_location = os.environ.get("CHROME_BIN", "/usr/bin/chromium")
        service = ChromeService(executable_path=os.environ.get("CHROMEDRIVER", "/usr/bin/chromedriver"))

        driver = webdriver.Chrome(service=service, options=opts)
        driver.set_page_load_timeout(20)

        url = f"http://{web_bind}/play.html"
        driver.get(url)

        term = driver.find_element(By.ID, "term")
        line = driver.find_element(By.ID, "line")

        # Full creation flow via web UI.
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

        send_line(line, "who")
        out = wait_for_term_contains(driver, term, "online (players):", timeout_s=20.0)
        if name not in out:
            raise RuntimeError("did not see our name in who output")

        # Verify session survives a page reload (resume token).
        driver.refresh()
        term = driver.find_element(By.ID, "term")
        line = driver.find_element(By.ID, "line")

        wait_for_term_contains(driver, term, "# connected:", timeout_s=20.0)
        send_line(line, "who")
        out2 = wait_for_term_contains(driver, term, "online (players):", timeout_s=20.0)
        if name not in out2:
            raise RuntimeError("after reload, did not see our name in who output (session not resumed?)")

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
