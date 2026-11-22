// Sun Life transaction history -> sunlife-transactions-<today>.csv (a download).
//
// Run it with the browser tool's JavaScript on the Details page
// (/GRS_MBR/wca/transHistoryDetails; reach it through Manage plan > my plan >
// Check on account > View transaction history > Details: a direct link shows a
// maintenance page). It walks every "Next" page with fetch, builds the CSV
// inside the page and saves it as a download, so no value reaches the agent.
// It returns only counts and the first and last dates.
//
// Columns: Date (ISO), Plan, Transaction type, Fund name, Units, Unit price,
// Amount. The playbook (connectors/playbooks/sunlife.md) explains each one.
(async () => {
  const MONTHS = { Jan: 1, Feb: 2, Mar: 3, Apr: 4, May: 5, Jun: 6, Jul: 7, Aug: 8, Sep: 9, Oct: 10, Nov: 11, Dec: 12 };
  const iso = (s) => {
    const m = s.trim().match(/^(\d{1,2}) ([A-Z][a-z]{2}) (\d{4})$/);
    return m ? `${m[3]}-${String(MONTHS[m[2]]).padStart(2, "0")}-${m[1].padStart(2, "0")}` : "";
  };
  const num = (s) => s.replace(/[$,\s]/g, "");
  const csv = (v) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
  const UNITS = /^(-?[\d,.]+) units at \$([\d,.]+)$/;

  const rows = [];
  const types = {};
  const read = (doc) => {
    const table = doc.querySelector("table.transHistActivityDetail");
    if (!table) return 0;
    let date = "", plan = "", type = "", n = 0;
    for (const r of [...table.rows].slice(1)) {
      const [d, p, desc, amount] = r.cells;
      if (!desc || !amount) continue;
      if (d.innerText.trim()) {
        date = iso(d.innerText);
        plan = p.innerText.trim();
        type = (desc.querySelector("strong, b")?.innerText || "").trim();
        types[type] = (types[type] || 0) + 1;
      }
      // Divs: [the type], fund name, "N units at $P". The first line of an
      // interfund transfer reads "from X" with its units in one div.
      const lines = [...desc.querySelectorAll("div")]
        .filter((x) => !x.querySelector("strong, b") && !x.querySelector("div"))
        .map((x) => x.innerText.trim())
        .filter(Boolean);
      let fund = "", units = "", price = "";
      for (const l of lines) {
        const u = l.match(UNITS);
        const inline = l.match(/^(.*?) (-?[\d,.]+) units at \$([\d,.]+)$/);
        if (u) [units, price] = [num(u[1]), num(u[2])];
        else if (inline) [fund, units, price] = [inline[1], num(inline[2]), num(inline[3])];
        else fund = l;
      }
      if (!date || !fund) continue;
      rows.push([date, plan, type, fund, units, price, num(amount.innerText)]);
      n++;
    }
    return n;
  };

  let doc = document;
  let pages = 0;
  for (;;) {
    pages++;
    read(doc);
    const form = doc.querySelector('form[action$="transHistoryDetails"]');
    const next = form && [...form.querySelectorAll("button, input[type=submit]")].some((b) => /next/i.test(b.textContent || b.value));
    if (!next || pages > 40) break;
    const body = new URLSearchParams(new FormData(form));
    body.set("direction", "next");
    const res = await fetch(form.action, { method: "POST", body, credentials: "include" });
    doc = new DOMParser().parseFromString(await res.text(), "text/html");
  }

  const head = ["Date", "Plan", "Transaction type", "Fund name", "Units", "Unit price", "Amount"];
  const text = [head, ...rows].map((r) => r.map(csv).join(",")).join("\n") + "\n";
  const today = new Date().toISOString().slice(0, 10);
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([text], { type: "text/csv" }));
  a.download = `sunlife-transactions-${today}.csv`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  const dates = rows.map((r) => r[0]).sort();
  return { pages, rows: rows.length, first: dates[0], last: dates.at(-1), types };
})();
