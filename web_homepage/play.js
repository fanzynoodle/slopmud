(() => {
  const term = document.getElementById("term");
  const wsUrlEl = document.getElementById("ws-url");
  const lineEl = document.getElementById("line");

  const menuBtn = document.getElementById("btn-menu");
  const menuDlg = document.getElementById("menu");

  const btnConnect = document.getElementById("btn-connect");
  const btnDisconnect = document.getElementById("btn-disconnect");
  const btnNewSession = document.getElementById("btn-new-session");
  const btnClear = document.getElementById("btn-clear");

  const optScroll = document.getElementById("opt-scroll");

  const acctEmailEl = document.getElementById("acct-email");
  const btnEmailShow = document.getElementById("btn-email-show");
  const btnEmailSet = document.getElementById("btn-email-set");
  const btnEmailClear = document.getElementById("btn-email-clear");

  const ssoUrlEl = document.getElementById("sso-url");
  const btnSsoOpen = document.getElementById("btn-sso-open");
  const btnSsoGoogle = document.getElementById("btn-sso-google");
  const btnSsoCheck = document.getElementById("btn-sso-check");

  const statusPill = document.getElementById("status-pill");
  const statusDetail = document.getElementById("status-detail");

  const decoder = new TextDecoder("utf-8", { fatal: false });
  let sock = null;
  let shouldReconnect = true;
  let reconnectTimer = null;
  let reconnectAttempts = 0;

  let ssoScanBuf = "";
  let lastSsoUrl = "";

  const LS_WS_URL = "slopmud_ws_url";
  const LS_AUTOSCROLL = "slopmud_autoscroll";
  const LS_RESUME_TOKEN = "slopmud_resume_token";

  function updateSsoUrl(url) {
    const u = String(url || "").trim();
    lastSsoUrl = u;
    if (!ssoUrlEl) return;
    if (!u) {
      ssoUrlEl.textContent = "none";
      ssoUrlEl.setAttribute("href", "#");
      btnSsoOpen && (btnSsoOpen.disabled = true);
      return;
    }
    ssoUrlEl.textContent = u;
    ssoUrlEl.setAttribute("href", u);
    btnSsoOpen && (btnSsoOpen.disabled = false);
  }

  function scanForSsoUrl(chunk) {
    const s = String(chunk || "");
    if (!s) return;

    // Keep a rolling buffer to handle URLs split across frames.
    ssoScanBuf = (ssoScanBuf + s).slice(-4096);

    // Broker prints: "open this url in a browser to sign in:\r\n  https://.../auth/google?code=...\r\n"
    const m = ssoScanBuf.match(/https?:\/\/[^\s]+\/auth\/google\?code=[A-Za-z0-9]+/);
    if (m && m[0] && m[0] !== lastSsoUrl) updateSsoUrl(m[0]);
  }

  function isValidResumeToken(t) {
    if (!t) return false;
    const s = String(t).trim();
    if (s.length < 16 || s.length > 128) return false;
    return /^[A-Za-z0-9_-]+$/.test(s);
  }

  function hex(u8) {
    const HEX = "0123456789abcdef";
    let out = "";
    for (let i = 0; i < u8.length; i++) {
      const b = u8[i];
      out += HEX[(b >> 4) & 0xf];
      out += HEX[b & 0xf];
    }
    return out;
  }

  function newResumeToken() {
    try {
      const u8 = new Uint8Array(16);
      crypto.getRandomValues(u8);
      return hex(u8);
    } catch {
      // Extremely old browsers / restricted contexts.
      return `r${Math.random().toString(16).slice(2)}${Date.now().toString(16)}`;
    }
  }

  function getOrCreateResumeToken() {
    const existing = localStorage.getItem(LS_RESUME_TOKEN);
    if (isValidResumeToken(existing)) return existing.trim();
    const t = newResumeToken();
    localStorage.setItem(LS_RESUME_TOKEN, t);
    return t;
  }

  function withResumeToken(url, token) {
    try {
      const u = new URL(url, defaultWsUrl());
      if (u.protocol === "http:") u.protocol = "ws:";
      if (u.protocol === "https:") u.protocol = "wss:";
      u.searchParams.set("resume", token);
      return u.toString();
    } catch {
      return url;
    }
  }

  function redactResumeToken(url) {
    try {
      const u = new URL(url, defaultWsUrl());
      if (u.protocol === "http:") u.protocol = "ws:";
      if (u.protocol === "https:") u.protocol = "wss:";
      if (u.searchParams.has("resume")) {
        u.searchParams.set("resume", "<resume>");
      }
      return u.toString();
    } catch {
      return url;
    }
  }

  function defaultWsUrl() {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${location.host}/ws`;
  }

  function loadSettings() {
    const u = localStorage.getItem(LS_WS_URL);
    if (wsUrlEl) wsUrlEl.value = (u && u.trim()) || defaultWsUrl();

    const s = localStorage.getItem(LS_AUTOSCROLL);
    if (optScroll) optScroll.checked = s === null ? true : s === "1";
  }

  function saveSettings() {
    if (wsUrlEl) localStorage.setItem(LS_WS_URL, wsUrlEl.value.trim());
    if (optScroll) localStorage.setItem(LS_AUTOSCROLL, optScroll.checked ? "1" : "0");
  }

  function setStatus(kind, detail = "") {
    if (statusPill) statusPill.textContent = kind;
    if (statusDetail) statusDetail.textContent = detail;
    if (!statusPill) return;

    statusPill.classList.remove("pill--ok", "pill--warn", "pill--bad");
    if (kind === "connected") statusPill.classList.add("pill--ok");
    else if (kind === "connecting") statusPill.classList.add("pill--warn");
    else if (kind === "error") statusPill.classList.add("pill--bad");
  }

  function append(text) {
    if (!term) return;
    term.textContent += text;
    if (optScroll && optScroll.checked) term.scrollTop = term.scrollHeight;
  }

  function appendLine(text) {
    append(text.endsWith("\n") ? text : `${text}\n`);
  }

  function clear() {
    if (!term) return;
    term.textContent = "";
  }

  function sendBytes(u8) {
    if (!sock || sock.readyState !== WebSocket.OPEN) return false;
    sock.send(u8);
    return true;
  }

  function sendText(s) {
    if (!sock || sock.readyState !== WebSocket.OPEN) return false;
    sock.send(s);
    return true;
  }

  function sendCmd(line) {
    const payload = line.endsWith("\n") ? line : `${line}\n`;
    if (!sendText(payload)) appendLine("# not connected");
    lineEl && lineEl.focus();
  }

  function connect() {
    const baseUrl = (wsUrlEl && wsUrlEl.value.trim()) || defaultWsUrl();
    if (wsUrlEl) wsUrlEl.value = baseUrl;
    saveSettings();

    if (sock && (sock.readyState === WebSocket.OPEN || sock.readyState === WebSocket.CONNECTING)) {
      return;
    }
    const resumeToken = getOrCreateResumeToken();
    const url = withResumeToken(baseUrl, resumeToken);
    const displayUrl = redactResumeToken(url);
    setStatus("connecting", displayUrl);

    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    sock = ws;

    btnConnect && (btnConnect.disabled = true);
    btnDisconnect && (btnDisconnect.disabled = false);

    ws.addEventListener("open", () => {
      setStatus("connected", displayUrl);
      appendLine(`# connected: ${displayUrl}`);
      lineEl && lineEl.focus();
      reconnectAttempts = 0;
    });

    ws.addEventListener("close", (ev) => {
      const why = ev.reason ? ` (${ev.reason})` : "";
      setStatus("disconnected", `${ev.code}${why}`);
      appendLine(`# disconnected: ${ev.code}${why}`);
      btnConnect && (btnConnect.disabled = false);
      btnDisconnect && (btnDisconnect.disabled = true);
      if (shouldReconnect) scheduleReconnect();
    });

    ws.addEventListener("error", () => {
      setStatus("error", "websocket error");
      appendLine("# websocket error");
    });

    ws.addEventListener("message", (ev) => {
      if (typeof ev.data === "string") {
        scanForSsoUrl(ev.data);
        append(ev.data);
        return;
      }
      if (ev.data instanceof ArrayBuffer) {
        const s = decoder.decode(new Uint8Array(ev.data));
        scanForSsoUrl(s);
        append(s);
        return;
      }
      // Blob fallback
      if (ev.data && typeof ev.data.arrayBuffer === "function") {
        ev.data.arrayBuffer().then((ab) => {
          const s = decoder.decode(new Uint8Array(ab));
          scanForSsoUrl(s);
          append(s);
        });
      }
    });
  }

  function disconnect() {
    shouldReconnect = false;
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (!sock) return;
    try {
      sock.close(1000, "bye");
    } catch {
      // ignore
    }
  }

  function scheduleReconnect() {
    if (reconnectTimer) return;
    const delay = Math.min(8000, 500 * Math.pow(2, reconnectAttempts));
    reconnectAttempts += 1;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, delay);
    setStatus("connecting", `reconnect in ${Math.round(delay)}ms`);
  }

  function submitLine() {
    if (!lineEl) return;
    const raw = lineEl.value;
    lineEl.value = "";

    const payload = raw + "\n";

    // Send as text; servers that care about bytes can accept Binary too.
    if (!sendText(payload)) appendLine("# not connected");
  }

  loadSettings();
  setStatus("disconnected", "");
  lineEl && lineEl.focus();

  menuBtn &&
    menuBtn.addEventListener("click", () => {
      if (!menuDlg || typeof menuDlg.showModal !== "function") return;
      menuDlg.showModal();
      wsUrlEl && wsUrlEl.focus();
      wsUrlEl && wsUrlEl.select && wsUrlEl.select();
    });

  menuDlg &&
    menuDlg.addEventListener("close", () => {
      saveSettings();
      lineEl && lineEl.focus();
    });

  btnConnect && btnConnect.addEventListener("click", () => {
    shouldReconnect = true;
    connect();
  });
  btnDisconnect && btnDisconnect.addEventListener("click", disconnect);
  btnNewSession &&
    btnNewSession.addEventListener("click", () => {
      const ok = window.confirm(
        "Start a new session? This clears the saved resume token so you can choose a different account name."
      );
      if (!ok) return;
      try {
        localStorage.removeItem(LS_RESUME_TOKEN);
      } catch {
        // ignore
      }
      location.reload();
    });
  btnClear && btnClear.addEventListener("click", () => {
    clear();
    lineEl && lineEl.focus();
  });

  btnEmailShow && btnEmailShow.addEventListener("click", () => sendCmd("account email"));
  btnEmailSet &&
    btnEmailSet.addEventListener("click", () => {
      const raw = acctEmailEl ? acctEmailEl.value.trim() : "";
      if (!raw) {
        appendLine("# missing email");
        return;
      }
      sendCmd(`account email set ${raw}`);
    });
  btnEmailClear && btnEmailClear.addEventListener("click", () => sendCmd("account email clear"));

  btnSsoOpen &&
    btnSsoOpen.addEventListener("click", () => {
      if (!lastSsoUrl) {
        appendLine("# no sso url seen yet");
        return;
      }
      try {
        window.open(lastSsoUrl, "_blank", "noopener,noreferrer");
      } catch {
        appendLine("# failed to open sso url");
      }
    });

  btnSsoGoogle && btnSsoGoogle.addEventListener("click", () => sendCmd("google"));
  btnSsoCheck && btnSsoCheck.addEventListener("click", () => sendCmd("check"));

  lineEl &&
    lineEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        submitLine();
        return;
      }

      if (e.ctrlKey && (e.key === "l" || e.key === "L")) {
        e.preventDefault();
        clear();
        return;
      }

      if (e.ctrlKey && (e.key === "c" || e.key === "C")) {
        // Send ^C
        e.preventDefault();
        const u8 = new Uint8Array([3]);
        if (!sendBytes(u8)) appendLine("# not connected");
        return;
      }
    });

  wsUrlEl &&
    wsUrlEl.addEventListener("change", () => {
      saveSettings();
    });

  optScroll &&
    optScroll.addEventListener("change", () => {
      saveSettings();
    });

  connect();
})();
