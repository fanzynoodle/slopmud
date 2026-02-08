(() => {
  const term = document.getElementById("term");
  const wsUrlEl = document.getElementById("ws-url");
  const lineEl = document.getElementById("line");

  const btnConnect = document.getElementById("btn-connect");
  const btnDisconnect = document.getElementById("btn-disconnect");
  const btnNewSession = document.getElementById("btn-new-session");
  const btnClear = document.getElementById("btn-clear");

  const optEcho = document.getElementById("opt-echo");
  const optCrlf = document.getElementById("opt-crlf");
  const optScroll = document.getElementById("opt-scroll");

  const statusPill = document.getElementById("status-pill");
  const statusDetail = document.getElementById("status-detail");

  const decoder = new TextDecoder("utf-8", { fatal: false });
  let sock = null;
  let shouldReconnect = true;
  let reconnectTimer = null;
  let reconnectAttempts = 0;

  const LS_RESUME_TOKEN = "slopmud_resume_token";

  function defaultWsUrl() {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    return `${proto}//${location.host}/ws`;
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

  function connect() {
    const url = (wsUrlEl && wsUrlEl.value.trim()) || defaultWsUrl();
    if (wsUrlEl) wsUrlEl.value = url;

    if (sock && (sock.readyState === WebSocket.OPEN || sock.readyState === WebSocket.CONNECTING)) {
      return;
    }
    setStatus("connecting", url);

    const ws = new WebSocket(url);
    ws.binaryType = "arraybuffer";
    sock = ws;

    btnConnect && (btnConnect.disabled = true);
    btnDisconnect && (btnDisconnect.disabled = false);

    ws.addEventListener("open", () => {
      setStatus("connected", url);
      appendLine(`# connected: ${url}`);
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
        append(ev.data);
        return;
      }
      if (ev.data instanceof ArrayBuffer) {
        append(decoder.decode(new Uint8Array(ev.data)));
        return;
      }
      // Blob fallback
      if (ev.data && typeof ev.data.arrayBuffer === "function") {
        ev.data.arrayBuffer().then((ab) => {
          append(decoder.decode(new Uint8Array(ab)));
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

    const lineEnd = optCrlf && optCrlf.checked ? "\r\n" : "\n";
    const payload = raw + lineEnd;

    if (optEcho && optEcho.checked) append(payload);

    // Send as text; servers that care about bytes can accept Binary too.
    if (!sendText(payload)) appendLine("# not connected");
  }

  if (wsUrlEl) wsUrlEl.value = defaultWsUrl();
  setStatus("disconnected", "");
  lineEl && lineEl.focus();

  btnConnect && btnConnect.addEventListener("click", () => {
    shouldReconnect = true;
    connect();
  });
  btnDisconnect && btnDisconnect.addEventListener("click", disconnect);
  btnNewSession &&
    btnNewSession.addEventListener("click", () => {
      const ok = window.confirm(
        "Start a new session? This clears any saved resume token so you can choose a different account name."
      );
      if (!ok) return;
      try {
        localStorage.removeItem(LS_RESUME_TOKEN);
      } catch {
        // ignore
      }
      disconnect();
      location.reload();
    });
  btnClear && btnClear.addEventListener("click", () => {
    clear();
    lineEl && lineEl.focus();
  });

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

  connect();
})();
