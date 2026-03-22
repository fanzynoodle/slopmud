#!/usr/bin/env python3

import json
import os
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

from selenium import webdriver
from selenium.webdriver.chrome.options import Options
from selenium.webdriver.chrome.service import Service


def _wait_http_json(url: str, timeout_s: float = 12.0):
    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            req = urllib.request.Request(
                url,
                headers={"accept": "application/json"},
            )
            with urllib.request.urlopen(req, timeout=3.0) as resp:
                if 200 <= resp.status < 300:
                    return json.loads(resp.read().decode("utf-8"))
        except Exception as e:
            last = e
        time.sleep(0.2)
    raise RuntimeError(f"timeout waiting for json from {url}: {last}")


def _wait_el(driver, by: str, value: str, timeout_s: float = 12.0):
    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            return driver.find_element(by, value)
        except Exception as e:
            last = e
            time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for element {by}={value!r} ({last})")


def _wait_enabled_id(driver, el_id: str, timeout_s: float = 12.0):
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        el = _wait_el(driver, "id", el_id, timeout_s=1.0)
        if el.get_attribute("disabled") is None:
            return el
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for enabled element id={el_id!r}")


def _term_text(driver) -> str:
    return driver.execute_script("return document.getElementById('term')?.textContent || ''")


def _wait_term(driver, needle: str, timeout_s: float = 20.0) -> str:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        text = _term_text(driver)
        if needle in text:
            return text
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for {needle!r}; got:\n{_term_text(driver)[-2000:]}")


def _wait_url_contains(driver, needle: str, timeout_s: float = 15.0) -> str:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        url = driver.current_url
        if needle in url:
            return url
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for url containing {needle!r}; got {driver.current_url!r}")


def _wait_url_path_endswith(driver, suffix: str, timeout_s: float = 15.0) -> str:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        parsed = urllib.parse.urlparse(driver.current_url)
        if parsed.path.endswith(suffix):
            return driver.current_url
        time.sleep(0.05)
    raise TimeoutError(f"timeout waiting for url path ending with {suffix!r}; got {driver.current_url!r}")


def _click_id(driver, el_id: str, timeout_s: float = 12.0):
    el = _wait_el(driver, "id", el_id, timeout_s=timeout_s)
    el.click()
    return el


def _send_line(driver, text: str):
    el = _wait_el(driver, "id", "line", timeout_s=10.0)
    el.send_keys(text + "\n")


def _new_chrome(user_data_dir: Path):
    opts = Options()
    opts.binary_location = os.environ.get("CHROME_BIN", "/usr/bin/chromium")
    opts.add_argument("--headless=new")
    opts.add_argument("--no-sandbox")
    opts.add_argument("--disable-dev-shm-usage")
    opts.add_argument("--window-size=1400,1100")
    opts.add_argument(f"--user-data-dir={user_data_dir}")
    return webdriver.Chrome(
        service=Service(os.environ.get("CHROMEDRIVER", "/usr/bin/chromedriver")),
        options=opts,
    )


def _start_from_home(driver, home_url: str) -> str:
    driver.get(home_url)
    if urllib.parse.urlparse(driver.current_url).path.endswith("/connect.html"):
        _click_id(driver, "connect-human-btn", timeout_s=8.0)
    else:
        play_link = _wait_el(
            driver,
            "xpath",
            "//a[contains(@href,'play.html') or contains(@href,'connect.html') or normalize-space()='play']",
            timeout_s=10.0,
        )
        play_link.click()

    current_path = urllib.parse.urlparse(driver.current_url).path
    if current_path.endswith("/connect.html"):
        _click_id(driver, "connect-human-btn", timeout_s=8.0)
        launch = _wait_el(
            driver,
            "xpath",
            "//a[contains(@href,'play.html') or normalize-space()='launch web client']",
            timeout_s=10.0,
        )
        launch.click()

    return _wait_url_path_endswith(driver, "/play.html", timeout_s=12.0)


def _assert_providers(play_url: str):
    parsed = urllib.parse.urlparse(play_url)
    providers_url = f"{parsed.scheme}://{parsed.netloc}/api/oauth/providers"
    providers = _wait_http_json(providers_url, timeout_s=12.0)
    if not providers.get("google"):
        raise RuntimeError(f"expected google provider enabled, got {providers}")
    if not providers.get("oidc"):
        raise RuntimeError(f"expected oidc provider enabled, got {providers}")
    return providers


def _password_play_flow(home_url: str, run_id: str):
    profile = Path(f"/tmp/slopmud_prd_auth_{run_id}_password")
    driver = _new_chrome(profile)
    try:
        play_url = _start_from_home(driver, home_url)
        providers = _assert_providers(play_url)
        _wait_term(driver, "name:", timeout_s=20.0)

        name = f"Prd{run_id[-8:]}"
        password = f"pw-{run_id[-8:]}-Auth"

        _send_line(driver, name)
        _wait_term(driver, "type: password | google | slopsso", timeout_s=20.0)
        _send_line(driver, "password")
        _wait_term(driver, "set password", timeout_s=20.0)
        _send_line(driver, password)
        _wait_term(driver, "type: human | bot", timeout_s=20.0)
        _send_line(driver, "human")
        _wait_term(driver, "type: agree", timeout_s=20.0)
        _send_line(driver, "agree")
        _wait_term(driver, "code of conduct:", timeout_s=20.0)
        _wait_term(driver, "type: agree", timeout_s=20.0)
        _send_line(driver, "agree")
        _wait_term(driver, "choose race:", timeout_s=20.0)
        _send_line(driver, "race human")
        _wait_term(driver, "choose class:", timeout_s=20.0)
        _send_line(driver, "class fighter")
        _wait_term(driver, "sex:", timeout_s=20.0)
        _send_line(driver, "none")
        _wait_term(driver, "Orientation Wing", timeout_s=25.0)
        return {"providers": providers, "name": name, "play_url": play_url}
    finally:
        driver.quit()


def _oauth_redirect_flow(home_url: str, run_id: str, method: str, expected_needles):
    profile = Path(f"/tmp/slopmud_prd_auth_{run_id}_{method}")
    driver = _new_chrome(profile)
    try:
        play_url = _start_from_home(driver, home_url)
        _assert_providers(play_url)
        _wait_term(driver, "name:", timeout_s=20.0)
        _send_line(driver, f"Prd{run_id[-8:]}{method[:2]}")
        _wait_term(driver, "type: password | google | slopsso", timeout_s=20.0)
        _send_line(driver, method)
        deadline = time.time() + 20.0
        while time.time() < deadline:
            url = driver.current_url
            if any(needle in url for needle in expected_needles):
                return url
            time.sleep(0.05)
        raise TimeoutError(
            f"timeout waiting for redirect from {method}; expected one of {expected_needles!r}, got {driver.current_url!r}"
        )
    finally:
        driver.quit()


def main() -> int:
    run_id = str(time.time_ns())
    home_url = os.environ.get("SLOPMUD_HOME_URL", "https://slopmud.com/")
    results = {}
    try:
        results["password"] = _password_play_flow(home_url, run_id)
        results["slopsso_redirect"] = _oauth_redirect_flow(
            home_url,
            run_id,
            "slopsso",
            ["/authorize", "response_type=code"],
        )
        results["google_redirect"] = _oauth_redirect_flow(
            home_url,
            run_id,
            "google",
            ["accounts.google.com", "oauth2", "o/oauth2"],
        )
        print(json.dumps(results, indent=2, sort_keys=True))
        return 0
    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        print(json.dumps(results, indent=2, sort_keys=True), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
