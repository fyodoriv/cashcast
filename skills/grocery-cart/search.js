// Paste into Chrome's JS tool on a store's home page, then run:
//   await sqSearch(["coconut water", "kombucha"])
// One call searches every item at this store in parallel (same-origin fetch,
// no page loads). Returns rows: {q, store, name, price, was, unit, organic, link}.
window.sqSearch = async (items) => {
  const host = location.host.replace(/^www\./, "");
  const num = (s) =>
    s == null || s === "" ? null : Number(String(s).replace(/[^0-9.]/g, "")) || null;
  const walk = (o, hit, out = []) => {
    if (!o || typeof o !== "object") return out;
    if (hit(o)) out.push(o);
    else for (const v of Object.values(o)) walk(v, hit, out);
    return out;
  };
  const json = (h, re) => JSON.parse(h.match(re)[1]);
  const NEXT = /<script id="__NEXT_DATA__"[^>]*>(.*?)<\/script>/s;
  const stores = {
    // Loblaw platform: Superstore, No Frills, Loblaws.
    pcx: {
      url: (q) => `/en/search?search-bar=${encodeURIComponent(q)}`,
      rows: (h) =>
        walk(json(h, NEXT), (o) => o.productId && o.pricing)
          .filter((o) => !o.isSponsored)
          .map((o) => ({
            name: `${o.brand || ""} ${o.title}`.trim(),
            price: num(o.pricing.price),
            was: num(o.pricing.wasPrice),
            unit: o.packageSizing,
            link: location.origin + o.link,
          })),
    },
    "voila.ca": {
      url: (q) => `/search?q=${encodeURIComponent(q)}`,
      rows: (h) =>
        walk(
          json(h, /__INITIAL_STATE__\s*=\s*(\{.*?\})\s*;?\s*<\/script>/s),
          (o) => o.productId && o.name && o.price,
        ).map((o) => ({
          name: o.name,
          price: num(o.price.current?.amount),
          was:
            o.price.original && o.price.original.amount !== o.price.current?.amount
              ? num(o.price.original.amount)
              : null,
          unit: o.price.unit?.current
            ? `$${o.price.unit.current.amount} ${o.price.unit.label.replace("fop.price.per.", "/")}`
            : o.size?.value,
          link: `${location.origin}/products/${o.retailerProductId}/details`,
        })),
    },
    "walmart.ca": {
      url: (q) => `/en/search?q=${encodeURIComponent(q)}`,
      rows: (h) =>
        walk(json(h, NEXT), (o) => o.name && o.priceInfo && o.__typename === "Product").map(
          (o) => ({
            name: o.name,
            price: num(o.priceInfo.linePrice),
            was: num(o.priceInfo.wasPrice),
            unit: o.priceInfo.unitPrice,
            link: location.origin + (o.canonicalUrl || ""),
          }),
        ),
    },
    "metro.ca": {
      url: (q) => `/en/online-grocery/search?filter=${encodeURIComponent(q)}`,
      rows: (h) =>
        [
          ...new DOMParser()
            .parseFromString(h, "text/html")
            .querySelectorAll(".default-product-tile"),
        ].map((e) => {
          const t = (s) => e.querySelector(s)?.textContent.replace(/\s+/g, " ").trim();
          const a = e.querySelector("a[href*='/p/']");
          return {
            name: [t(".head__brand"), t(".head__title"), t(".head__unit-details")]
              .filter(Boolean)
              .join(" "),
            price: num(t(".price-update")),
            was: num(t(".pricing__before-price")),
            unit: t(".pricing__secondary-price"),
            link: a ? location.origin + a.getAttribute("href") : null,
          };
        }),
    },
    // Amazon.ca: signed in, Prime ships free. Bulk packs are common.
    "amazon.ca": {
      url: (q) => `/s?k=${encodeURIComponent(q)}&i=grocery`,
      rows: (h) =>
        [
          ...new DOMParser()
            .parseFromString(h, "text/html")
            .querySelectorAll("[data-component-type=s-search-result]"),
        ]
          .filter((e) => !e.querySelector(".puis-sponsored-label-text"))
          .map((e) => ({
            name: e.querySelector("h2")?.textContent.trim(),
            price: num(e.querySelector(".a-price .a-offscreen")?.textContent),
            was: num(e.querySelector(".a-price.a-text-price .a-offscreen")?.textContent),
            unit: e.textContent.match(/\$[\d.]+\s*\/\s*(100\s*(ml|g|gram)|l|kg|count)/i)?.[0],
            link: `${location.origin}/dp/${e.getAttribute("data-asin")}`,
          })),
    },
    // Skip (skipthedishes.com): run on a store's own page (/<store-slug>).
    // The page's __NEXT_DATA__ holds the whole menu (prices in cents), so one
    // fetch of the same page serves every item; sqSum and sqRows keep the
    // rows that match. Store name: `skip:<store>`. The Grocery tab is
    // Instacart inside Skip and asks to accept its terms: not covered here.
    "skipthedishes.com": {
      url: () => location.pathname,
      rows: (h, q) => {
        const p = json(h, NEXT).props.pageProps;
        // A whole menu: keep names with every query word first, so the
        // 40-row cut below never drops a match.
        const words = q
          .toLowerCase()
          .split(/\s+/)
          .filter((w) => w && !w.startsWith("-"));
        const store = `skip:${p.partner?.name ?? location.pathname.slice(1)}`;
        return (p.menuV2?.categories ?? []).flatMap((c) =>
          (c.menuItems ?? [])
            .filter(
              (i) =>
                i.available !== false &&
                words.every((w) => i.name.toLowerCase().includes(w.replace(/(es|s)$/, ""))),
            )
            .map((i) => ({
              store,
              name: i.name,
              price: i.subtotal / 100,
              was: null,
              unit: i.name.match(/\(([^)]*\d[^)]*)\)\s*$/)?.[1] ?? null,
              link: `${location.origin}${location.pathname}`,
            })),
        );
      },
    },
  };
  // Costco.ca renders results in the page, so it reads the open search page
  // (navigate to /s?keyword=<item> first, then call sqTiles("<item>")).
  window.sqTiles = (q) =>
    [...document.querySelectorAll('[data-testid^="ProductTile"]')]
      .map((e) => {
        const t = e.innerText.replace(/\s+/g, " ");
        const a = e.querySelector("a[href]");
        return {
          q,
          store: host,
          name:
            t.match(/^(.*?)(?: CostcoGrocery| Online Only| Warehouse Only| \$)/)?.[1] ??
            t.slice(0, 80),
          price: num(t.match(/\$[\d,]+\.\d\d/)?.[0]),
          link: a?.href,
          organic: /organic|biologique/i.test(t),
          deliverable: !/Warehouse Only|Out of Stock/.test(t),
        };
      })
      .filter((r) => r.price && r.deliverable);
  const s = stores[host] || (/superstore|nofrills|loblaws/.test(host) ? stores.pcx : null);
  if (!s) return `no adapter for ${host}`;
  const pages = {};
  const res = await Promise.all(
    items.map(async (q) => {
      try {
        const u = s.url(q.replace(/\s+-\S+/g, ""));
        // One fetch per page: Skip uses the same store page for every item.
        pages[u] ??= fetch(u).then((r) => r.text());
        const h = await pages[u];
        return s
          .rows(h, q)
          .filter((r) => r.price)
          .slice(0, 40)
          .map((r) => ({ q, store: host, ...r, organic: /organic|biologique/i.test(r.name) }));
      } catch (e) {
        return [{ q, store: host, error: String(e).slice(0, 80) }];
      }
    }),
  );
  return res.flat();
};

// Skip+ fee on a Skip store page: pass the page text (document.body.innerText).
// "$2.99 $0.00 Delivery Fee Over $15.00" is Skip+: $0 at $15 or more, else the
// struck-out $2.99. It returns {delivery: 2.99, free_over: 15, note}: save it with
// `bin/cashcast grocery fee skip:<store> --delivery 2.99 --free-over 15 --note "<note>"`.
// Skip keeps its own saved address, which differs from the other apps' address:
// the note says so. Never change it.
window.sqSkipFee = (text = document.body.innerText) => {
  const m = String(text).match(/(?:\$([\d.]+)\s+)?\$([\d.]+) Delivery Fee(?: Over \$([\d.]+))?/i);
  if (!m) return null;
  const over = m[3] ? Number(m[3]) : null;
  return {
    delivery: over != null && m[1] ? Number(m[1]) : Number(m[2]),
    free_over: over != null && Number(m[2]) === 0 ? over : null,
    note: "Skip+ fee; Skip's saved address differs from the other apps' address (never change it)",
  };
};

// Relevant rows only (every query word in the name; "sun dried" matches
// "sundried"), cheapest first, organic flagged. Short output for the agent.
window.sqSum = async (items) => {
  const t0 = performance.now();
  const rows = await sqSearch(items);
  if (typeof rows === "string") return rows;
  const norm = (s) => s.toLowerCase().replace(/[^a-z]/g, "");
  const fits = (q, name) =>
    q
      .toLowerCase()
      .split(/\s+/)
      .every((w) =>
        w.startsWith("-")
          ? !norm(name).includes(norm(w))
          : norm(name).includes(norm(w).replace(/(es|s)$/, "")),
      );
  const f = (r) =>
    `${r.price}${r.was ? ` (was ${r.was})` : ""} ${r.organic ? "ORGANIC " : ""}${r.name} [${r.unit || ""}]`;
  const out = { store: location.host, ms: Math.round(performance.now() - t0) };
  for (const q of items) {
    // Cheapest per 100 g/ml first (small packs look cheap by item price).
    const per = (r) => {
      const m = String(r.unit || "").match(/\$?\s*([\d.]+)\s*(¢?)\s*\/\s*100/);
      return m ? Number(m[1]) / (m[2] ? 100 : 1) : r.price;
    };
    const ok = rows
      .filter((r) => r.q === q && r.price && fits(q, r.name))
      .sort((a, b) => per(a) - per(b));
    out[q] = [...ok.slice(0, 3), ...ok.filter((r) => r.organic).slice(0, 2)]
      .filter((r, i, a) => a.indexOf(r) === i)
      .map(f);
  }
  return out;
};

// Rows to save in the grocery db (`bin/cashcast grocery save --file`): the 6 best
// relevant packs per item by price per 100 g/ml, pack sizes read from the name
// ("12 × 330 mL") so bulk packs compare fairly. Pass Costco's sqTiles rows as `rows`.
window.sqRows = async (items, rows) => {
  const all = rows ?? (await sqSearch(items));
  if (typeof all === "string") return all;
  const norm = (s) => s.toLowerCase().replace(/[^a-z]/g, "");
  const fits = (q, name) =>
    q
      .toLowerCase()
      .split(/\s+/)
      .every((w) =>
        w.startsWith("-")
          ? !norm(name).includes(norm(w))
          : norm(name).includes(norm(w).replace(/(es|s)$/, "")),
      );
  const per = (r) => {
    const u = String(r.unit || "");
    const m = u.match(/\$?\s*([\d.]+)\s*(¢?)\s*\/\s*100/);
    if (m) return Number(m[1]) / (m[2] ? 100 : 1);
    const s = `${u} ${r.name}`.match(/(?:(\d+)\s*[x×]\s*)?([\d.]+)\s*(ml|l|litre|g|kg)\b/i);
    if (!s) return 1e9;
    const n = Number(s[1] ?? 1) * Number(s[2]) * (/^(l|litre|kg)$/i.test(s[3]) ? 1000 : 1);
    return (r.price / n) * 100;
  };
  return items.flatMap((q) =>
    all
      .filter((r) => r.q === q && r.price && fits(q, r.name))
      .sort((a, b) => per(a) - per(b))
      .slice(0, 6)
      .map((r) => ({
        item: q,
        store: r.store,
        name: r.name,
        price: r.price,
        was: r.was ?? null,
        unit: r.unit ?? null,
      })),
  );
};
