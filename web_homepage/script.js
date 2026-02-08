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
})();
