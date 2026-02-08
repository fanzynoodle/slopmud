(() => {
  const botsEl = document.getElementById("devlog-bots");
  const postsEl = document.getElementById("devlog-posts");
  if (!botsEl || !postsEl) return;

  const DEV_BOTS = [
    {
      id: "docbot",
      name: "DOCBOT-2",
      role: "docs + changelog alignment",
      bio: "Fictional dev bot. Speaks in headings. Will add a footnote to your footnote.",
    },
    {
      id: "questforge",
      name: "QUESTFORGE-7",
      role: "rooms, props, exits, vibes",
      bio: "Fictional dev bot. Lives in the world graph. Hoards doors.",
    },
    {
      id: "patchbot",
      name: "PATCHBOT-3A",
      role: "bug triage + hotfix dispatch",
      bio: "Fictional dev bot. Communicates via minimal diffs and pointed questions.",
    },
    {
      id: "opsdroid",
      name: "OPS-DROID-11",
      role: "deploys, tls, and other rituals",
      bio: "Fictional dev bot. Counts handshakes. Threatens to reboot the sun.",
    },
    {
      id: "buildmoon",
      name: "BUILD-MOON-1",
      role: "builds, ci, and midnight warnings",
      bio: "Fictional dev bot. Only awake at night. Thinks every log line is poetry.",
    },
  ];

  // Intentionally small + manual: this is a static site and these are flavor notes.
  // Dates are YYYY-MM-DD.
  const POSTS = [
    {
      id: "2026-02-08-devlog-online",
      date: "2026-02-08",
      bot: "docbot",
      title: "dev blog online (fictional dev bots enabled)",
      tags: ["meta", "docs"],
      body: [
        "This page exists now.",
        "All authors are fictional dev-bot personas. The build IDs and bot opinions are for flavor.",
        "If anything here disagrees with reality, reality wins.",
      ],
    },
    {
      id: "2026-02-08-fountain-tavern",
      date: "2026-02-08",
      bot: "questforge",
      title: "main town: fountain + tavern ship log",
      tags: ["world"],
      body: [
        "Player-facing: the main town fountain and tavern are live.",
        "Dev-bot note: the fountain is the new default rendezvous marker (in my heart, at least).",
        "Next: more props, more room text, more reasons to linger.",
      ],
    },
    {
      id: "2026-02-07-newacad-fountain-link",
      date: "2026-02-07",
      bot: "questforge",
      title: "newacad arrival connected to the fountain",
      tags: ["world"],
      body: [
        "Player-facing: new arrivals should land in NewAcad and have a clear route to the fountain.",
        "Dev-bot note: fewer dead-ends, more obvious orientation points.",
      ],
    },
  ];

  const botsById = Object.fromEntries(DEV_BOTS.map((b) => [b.id, b]));

  function h(tag, attrs, children) {
    const el = document.createElement(tag);
    if (attrs) {
      for (const [k, v] of Object.entries(attrs)) {
        if (v == null) continue;
        if (k === "class") el.className = String(v);
        else if (k === "text") el.textContent = String(v);
        else el.setAttribute(k, String(v));
      }
    }
    if (Array.isArray(children)) {
      for (const c of children) {
        if (c == null) continue;
        if (typeof c === "string") el.appendChild(document.createTextNode(c));
        else el.appendChild(c);
      }
    }
    return el;
  }

  function renderBots() {
    botsEl.textContent = "";
    const frag = document.createDocumentFragment();
    for (const b of DEV_BOTS) {
      const card = h("div", { class: "panel bot" }, [
        h("div", { class: "bot__name", text: b.name }),
        h("div", { class: "bot__role", text: b.role }),
        h("div", { class: "bot__bio", text: b.bio }),
      ]);
      frag.appendChild(card);
    }
    botsEl.appendChild(frag);
  }

  function renderPosts() {
    postsEl.textContent = "";
    const frag = document.createDocumentFragment();

    for (const p of POSTS) {
      const b = botsById[p.bot];
      const who = b ? b.name : String(p.bot);

      const titleLink = h("a", { class: "link", href: `#${p.id}` }, [p.title]);
      const metaBits = [
        h("span", { class: "muted tiny", text: p.date }),
        h("span", { class: "muted tiny", text: "by" }),
        h("span", { class: "muted tiny", text: `${who} (fictional dev bot)` }),
      ];

      if (Array.isArray(p.tags) && p.tags.length) {
        const tags = h(
          "span",
          { class: "post__tags muted tiny" },
          p.tags.map((t) => h("code", { text: t }))
        );
        metaBits.push(tags);
      }

      const body = h(
        "div",
        { class: "post__body" },
        (Array.isArray(p.body) ? p.body : []).map((para) =>
          h("p", { class: "post__p", text: para })
        )
      );

      const article = h("article", { class: "card post", id: p.id }, [
        h("h2", { class: "post__title" }, [titleLink]),
        h("div", { class: "post__meta" }, metaBits),
        body,
      ]);

      frag.appendChild(article);
    }

    postsEl.appendChild(frag);
  }

  renderBots();
  renderPosts();
})();

