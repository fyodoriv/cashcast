// Cart helpers for the owner's Chrome. Paste into the JS tool on the store's
// page, then call one helper per item. Each call adds or sets one line, so it
// stays under the JS tool's 45 s limit. A page load clears them: paste again.
// None of them presses Checkout or Place order.

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// Store search pages (Uber Eats, DoorDash): wait for the first product card,
// not a fixed 5-6 s sleep. Returns the ms it took, or -1 after `max` ms.
//   await cardsReady()            // either app
//   await cardsReady("doordash")  // only [data-testid=add-button-label]
const CARD = {
  ubereats: () =>
    [...document.querySelectorAll("button")].some((b) => /Quick Add/i.test(b.textContent)),
  doordash: () => !!document.querySelector("[data-testid=add-button-label]"),
};
window.cardsReady = async (app, max = 6000) => {
  const seen = app ? CARD[app] : () => CARD.ubereats() || CARD.doordash();
  const t0 = Date.now();
  while (Date.now() - t0 < max) {
    if (seen()) return Date.now() - t0;
    await wait(150);
  }
  return -1;
};

// Instacart store search page (/store/<store>/s?k=<item>):
//   await icAddN(/Life Smart Organic No Sugar/, 6)
window.icAddN = async (re, n) => {
  const add = [...document.querySelectorAll('button[aria-label^="Add"]')].find((b) =>
    re.test(b.getAttribute("aria-label")),
  );
  if (!add) return `not found ${re}`;
  add.scrollIntoView({ block: "center" });
  add.click();
  await wait(1800);
  for (let i = 1; i < n; i++) {
    const inc = [...document.querySelectorAll('button[aria-label^="Increment"]')].find((b) =>
      re.test(b.getAttribute("aria-label")),
    );
    if (!inc) return `stopped at ${i} ${re}`;
    inc.click();
    await wait(1300);
  }
  return `ok ${n} ${re}`;
};

// DoorDash store search (/convenience/store/<id>/search/<item>/): cards whose
// first button reads "N in cart". Click card i once to add 1. More than 1 needs
// the stepper's "add one to cart" button, which only a real (ref) click opens.
window.ddCards = () =>
  [...document.querySelectorAll('button[aria-label*="in cart"]')].map((b) => {
    let c = b;
    for (let k = 0; k < 8 && !/CA\s*\$/.test(c.innerText); k++) c = c.parentElement;
    return { b, t: c.innerText.replace(/\s+/g, " ").slice(0, 110) };
  });

// PC Express cart page (/en/cartReview, No Frills and Loblaws): set a
// preferred substitute per line. pcEdits() lists the lines; pcSet(i, [regex…])
// picks the first suggestion that matches and skips names that match
// window.sqAvoid (a regex built from the person's `grocery prefs` rules, or unset).
//   await pcSet(3, [/kettle.*original/i, /chip/i])
window.pcDlg = () =>
  [...document.querySelectorAll("section,[role=dialog]")]
    .filter((e) => /Substitution preferences/.test(e.innerText))
    .sort((a, b) => a.innerText.length - b.innerText.length)[0];
window.pcCards = () =>
  [...window.pcDlg().querySelectorAll("svg[data-testid*=Circle]")].map((s) => {
    let c = s;
    for (let k = 0; k < 6 && !/\$/.test(c.textContent); k++) c = c.parentElement;
    return { s, t: c.textContent.replace(/\s+/g, " ").slice(0, 70) };
  });
window.pcEdits = () =>
  [...document.querySelectorAll("button,a")].filter(
    (b) =>
      b.textContent.trim() === "Edit" &&
      /substitut|Replace with/i.test(b.parentElement.parentElement.textContent),
  );
window.pcSet = async (i, prefs) => {
  window.pcEdits()[i].click();
  await wait(2000);
  window.pcDlg().querySelector("input[value=CustomerChoice]").click();
  await wait(2500);
  const cards = window.pcCards();
  let hit = null;
  for (const p of prefs) {
    hit = cards.find((x) => p.test(x.t) && !(window.sqAvoid && window.sqAvoid.test(x.t)));
    if (hit) break;
  }
  const btn = (re) =>
    [...window.pcDlg().querySelectorAll("button")].find((b) => re.test(b.textContent.trim()));
  if (!hit) {
    btn(/Close/)?.click();
    await wait(1200);
    return `NONE of: ${cards.map((x) => x.t.slice(0, 45)).join(" ; ")}`;
  }
  (hit.s.closest("button,label,[role=button]") || hit.s.parentElement).click();
  await wait(1000);
  btn(/^Save$/).click();
  await wait(2200);
  return `set ${hit.t.slice(0, 50)}`;
};

// Uber Eats checkout page: ueOptions(name) opens a line's "Choose a
// replacement now" list; uePick(prefs) picks; ueFinish() saves. Run ONE line
// per JS call: a loop over all lines passes 45 s and keeps running unseen.
window.ueOptions = async (name) => {
  const li = [...document.querySelectorAll("li")]
    .filter((l) => l.innerText.includes(name))
    .sort((a, b) => a.innerText.length - b.innerText.length)[0];
  if (!li) return "no line";
  li.scrollIntoView({ block: "center" });
  const link =
    [...li.querySelectorAll("*")]
      .filter((e) => e.children.length === 0 && /^Replace with/.test(e.textContent.trim()))
      .pop() || li;
  link.click();
  await wait(2200);
  const dlg = () => document.querySelector("[role=dialog]");
  const h = [...dlg().querySelectorAll("h1,h2,h3,h4,h5,h6")].find((e) =>
    /Add note or edit replacement/.test(e.textContent),
  );
  (h?.closest("button,[role=button],a,li") || h)?.click();
  await wait(2200);
  dlg().querySelector("[role=radio],input[type=radio]")?.click();
  await wait(2800);
  return dlg().innerText.replace(/\n+/g, " | ").slice(0, 750);
};
window.uePick = async (prefs) => {
  const hs = [
    ...document.querySelector("[role=dialog]").querySelectorAll("h1,h2,h3,h4,h5,h6"),
  ].slice(1);
  for (const p of prefs) {
    const h = hs.find((e) => p.test(e.textContent) && !(window.sqAvoid && window.sqAvoid.test(e.textContent)));
    if (h) {
      (h.closest("button,[role=button],li,label") || h).click();
      await wait(1800);
      return h.textContent.trim();
    }
  }
  return null;
};
window.ueFinish = async () => {
  const btn = (re) =>
    [...document.querySelectorAll("[role=dialog] button")].find((b) =>
      re.test(b.textContent.trim()),
    );
  btn(/^Save$/)?.click();
  await wait(2500);
  btn(/^Update order/)?.click();
  await wait(3000);
  return document.querySelector("[role=dialog]") ? "dialog still open" : "closed";
};
