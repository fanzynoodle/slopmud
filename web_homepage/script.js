(() => {
  const seed = Math.floor(Math.random() * 1_000_000);
  const mottos = [
    "ship it, regret it later",
    "cheap box, loud errors",
    "good enough is a feature",
    "the output is the product",
    "reduce idea-to-running-code distance",
  ];

  const seedEl = document.getElementById("slop-seed");
  const mottoEl = document.getElementById("slop-motto");

  if (seedEl) seedEl.textContent = String(seed);
  if (mottoEl) mottoEl.textContent = mottos[seed % mottos.length];

  const humanBtn = document.getElementById("connect-human-btn");
  const robotBtn = document.getElementById("connect-robot-btn");
  const humanPane = document.getElementById("connect-human");
  const robotPane = document.getElementById("connect-robot");

  function setConnectMode(mode) {
    const isHuman = mode === "human";
    if (humanBtn) humanBtn.classList.toggle("seg__btn--active", isHuman);
    if (robotBtn) robotBtn.classList.toggle("seg__btn--active", !isHuman);
    if (humanPane) humanPane.classList.toggle("is-hidden", !isHuman);
    if (robotPane) robotPane.classList.toggle("is-hidden", isHuman);
  }

  if (humanBtn && robotBtn && humanPane && robotPane) {
    humanBtn.addEventListener("click", () => setConnectMode("human"));
    robotBtn.addEventListener("click", () => setConnectMode("robot"));
    setConnectMode("human");
  }

  const onlineMetaEl = document.getElementById("online-meta");
  const onlineHumansEl = document.getElementById("online-humans");
  const onlineBotsEl = document.getElementById("online-bots");
  const onlineHumansCountEl = document.getElementById("online-humans-count");
  const onlineBotsCountEl = document.getElementById("online-bots-count");

  function renderNames(el, names) {
    if (!el) return;
    el.textContent = "";
    if (!Array.isArray(names) || names.length === 0) return;

    const frag = document.createDocumentFragment();
    for (const n of names) {
      const li = document.createElement("li");
      li.textContent = String(n);
      frag.appendChild(li);
    }
    el.appendChild(frag);
  }

  async function refreshOnline() {
    const ctl = new AbortController();
    const t = setTimeout(() => ctl.abort(), 1500);
    try {
      const res = await fetch("/api/online", { cache: "no-store", signal: ctl.signal });
      const data = await res.json();
      if (!data || data.type !== "ok_sessions") {
        const msg = data && data.message ? data.message : "bad response";
        throw new Error(msg);
      }

      const humans = Array.isArray(data.humans) ? data.humans : [];
      const bots = Array.isArray(data.bots) ? data.bots : [];
      renderNames(onlineHumansEl, humans);
      renderNames(onlineBotsEl, bots);

      if (onlineHumansCountEl) onlineHumansCountEl.textContent = `(${humans.length})`;
      if (onlineBotsCountEl) onlineBotsCountEl.textContent = `(${bots.length})`;

      if (onlineMetaEl) {
        const total = humans.length + bots.length;
        const ts = new Date().toLocaleTimeString();
        onlineMetaEl.textContent = `${total} online (updated ${ts})`;
      }
    } catch (e) {
      renderNames(onlineHumansEl, []);
      renderNames(onlineBotsEl, []);
      if (onlineHumansCountEl) onlineHumansCountEl.textContent = "";
      if (onlineBotsCountEl) onlineBotsCountEl.textContent = "";
      if (onlineMetaEl) onlineMetaEl.textContent = "offline";
    } finally {
      clearTimeout(t);
    }
  }

  if (onlineHumansEl && onlineBotsEl) {
    refreshOnline();
    setInterval(refreshOnline, 4000);
  }
})();
