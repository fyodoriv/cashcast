import { type FormEvent, useCallback, useEffect, useState } from "react";
import { TableWrap } from "../TableWrap";
import { fetchGroceries } from "../api";
import { monthLabel, weekdayLabel } from "../format";
import type { Groceries as GroceriesData, GroceryBasket, GroceryPrice } from "../types";

export interface GroceriesApi {
  load: (user: string, items: string[]) => Promise<GroceriesData>;
}

const realApi: GroceriesApi = { load: fetchGroceries };
const SHOW = 5;

/** $12.34 with cents: grocery totals differ by a few dollars. */
const cash = (n: number | null) => (n === null ? "—" : `$${n.toFixed(2)}`);

/** Items typed as "oats, tea" become ["oats", "tea"]. */
export const parseItems = (text: string) =>
  text
    .split(",")
    .map((x) => x.trim().toLowerCase())
    .filter(Boolean);

/** How far each full basket sits below the priciest full basket, in whole percent. */
export function belowPriciest(baskets: GroceryBasket[]): Map<string, number> {
  const full = baskets.filter((b) => b.missing.length === 0 && b.total !== null);
  const top = Math.max(0, ...full.map((b) => b.total ?? 0));
  return new Map(full.map((b) => [b.store, top > 0 ? Math.round((1 - (b.total ?? 0) / top) * 100) : 0]));
}

const pack = (p: GroceryPrice) =>
  `${cash(p.price)}${p.was ? ` (was ${cash(p.was)})` : ""} ${p.organic ? "organic " : ""}${p.name}`;

function Basket({ b, below }: { b: GroceryBasket; below: number | undefined }) {
  return (
    <details>
      <summary>
        {b.store}: {cash(b.total)}
        {below ? ` · ${below} % below the priciest` : ""}
        {b.missing.length ? ` · missing ${b.missing.join(", ")}` : ""}
      </summary>
      <TableWrap>
        <table aria-label={`best packs at ${b.store}`}>
          <thead>
            <tr>
              <th scope="col">Item</th>
              <th scope="col">Best pack</th>
              <th scope="col">Substitute</th>
            </tr>
          </thead>
          <tbody>
            {b.lines.map((l) => (
              <tr key={l.item}>
                <td>{l.item}</td>
                <td>{l.best ? pack(l.best) : <span className="muted">not found</span>}</td>
                <td>{l.sub ? pack(l.sub) : <span className="muted">none</span>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </TableWrap>
      <p className="muted">
        Items {cash(b.subtotal)}
        {b.discount ? ` · promo −${cash(b.discount)}` : ""} · delivery {cash(b.delivery)} · service {cash(b.service)}
        {b.total === null ? ` · ${b.fee_note}` : ""}
      </p>
    </details>
  );
}

/** Groceries: compare store baskets from saved prices, see fee rules and spend per month. */
export function Groceries({ user, api = realApi }: { user: string; api?: GroceriesApi }) {
  const [data, setData] = useState<GroceriesData | null>(null);
  const [error, setError] = useState("");
  const [text, setText] = useState("");
  const [asked, setAsked] = useState<string[]>([]);
  const [allOrders, setAllOrders] = useState(false);

  const load = useCallback(
    (items: string[]) => {
      setError("");
      api
        .load(user, items)
        .then(setData)
        .catch((e) => setError(e instanceof Error ? e.message : String(e)));
    },
    [user, api],
  );
  useEffect(() => load(asked), [load, asked]);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setAsked(parseItems(text));
  };
  const addItem = (item: string) => {
    const now = parseItems(text);
    if (!now.includes(item)) setText([...now, item].join(", "));
  };

  if (error && !data) {
    return (
      <section>
        <h2>Groceries</h2>
        <p role="alert">{error}</p>
      </section>
    );
  }
  if (!data) {
    return (
      <section>
        <h2>Groceries</h2>
        <p className="muted">Loading…</p>
      </section>
    );
  }
  if (data.error) {
    return (
      <section>
        <h2>Groceries</h2>
        <p role="alert">The grocery prices could not be read: {data.error}</p>
      </section>
    );
  }

  const below = belowPriciest(data.compare);
  const orders = allOrders ? data.orders : data.orders.slice(0, SHOW);
  const thisMonth = data.months[0];

  return (
    <section className="groceries">
      <h2>Groceries</h2>
      <p className="muted">
        Ask Claude “order groceries”. It reads 5 or more stores in your Chrome, saves the prices here, and fills the
        cheapest carts. You pick the store and check out.
      </p>

      <div className="cards">
        <div className="card">
          <div className="card-label">Items with prices</div>
          <div className="card-value">{data.items.length}</div>
        </div>
        <div className="card">
          <div className="card-label">{thisMonth ? `Spent in ${monthLabel(thisMonth.month)}` : "Spent this month"}</div>
          <div className="card-value">{thisMonth ? cash(thisMonth.total) : "—"}</div>
          {thisMonth && (
            <div className="card-sub">{thisMonth.orders === 1 ? "1 order" : `${thisMonth.orders} orders`}</div>
          )}
        </div>
        <div className="card">
          <div className="card-label">Stores with fee rules</div>
          <div className="card-value">{data.fees.length}</div>
        </div>
      </div>

      <h3>Compare stores</h3>
      {data.items.length === 0 ? (
        <p>
          No prices yet. Ask Claude “order groceries”, or bring your old prices with{" "}
          <code>bin/cashcast grocery import --from OLD.sqlite</code>.
        </p>
      ) : (
        <>
          <form className="grocery-form" onSubmit={submit}>
            <label htmlFor="grocery-items">Items, comma-separated</label>
            <input
              id="grocery-items"
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder="oats, tea"
              autoComplete="off"
            />
            <button type="submit" className="cat-btn primary" disabled={parseItems(text).length === 0}>
              Compare
            </button>
          </form>
          <div className="chips" aria-label="items with saved prices">
            {data.items.slice(0, 12).map((i) => (
              <button key={i.item} type="button" className="cat-btn grocery-chip" onClick={() => addItem(i.item)}>
                + {i.item}
              </button>
            ))}
          </div>
          {data.asked.length > 0 &&
            (data.compare.length === 0 ? (
              <p>
                No prices newer than {data.freshDays} days for {data.asked.join(", ")}. Ask Claude to search the stores
                again.
              </p>
            ) : (
              <>
                <p className="muted">
                  Full baskets first, cheapest first. Totals include delivery, service and promos.
                </p>
                {data.compare.map((b) => (
                  <Basket key={b.store} b={b} below={below.get(b.store)} />
                ))}
              </>
            ))}
        </>
      )}

      <h3>Spend by month</h3>
      {data.months.length === 0 ? (
        <p className="muted">
          No orders logged yet. After checkout Claude runs <code>bin/cashcast grocery order STORE --total N</code>.
        </p>
      ) : (
        <TableWrap>
          <table aria-label="grocery spend by month">
            <thead>
              <tr>
                <th scope="col">Month</th>
                <th scope="col" className="num">
                  Orders
                </th>
                <th scope="col" className="num">
                  Spent
                </th>
              </tr>
            </thead>
            <tbody>
              {data.months.map((m) => (
                <tr key={m.month}>
                  <td>{monthLabel(m.month)}</td>
                  <td className="num">{m.orders}</td>
                  <td className="num">{cash(m.total)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </TableWrap>
      )}
      {data.orders.length > 0 && (
        <>
          <h3>Recent orders</h3>
          <ul>
            {orders.map((o, i) => (
              <li key={`${o.day}-${o.store}-${i}`}>
                {weekdayLabel(o.day)} · {o.store} · {cash(o.total)}
                {o.note ? ` · ${o.note}` : ""}
              </li>
            ))}
          </ul>
          {data.orders.length > SHOW && (
            <button type="button" className="cat-btn" onClick={() => setAllOrders(!allOrders)}>
              {allOrders ? "Show fewer" : `Show ${data.orders.length - SHOW} more`}
            </button>
          )}
        </>
      )}

      <h3>Store fees</h3>
      {data.fees.length === 0 ? (
        <p className="muted">
          No fee rules yet. Claude saves each store's delivery fee from its checkout page with{" "}
          <code>bin/cashcast grocery fee STORE --delivery N</code>.
        </p>
      ) : (
        <TableWrap>
          <table aria-label="store fees after memberships">
            <thead>
              <tr>
                <th scope="col">Store</th>
                <th scope="col" className="num">
                  Delivery
                </th>
                <th scope="col">Promo</th>
                <th scope="col">Checked</th>
              </tr>
            </thead>
            <tbody>
              {data.fees.map((f) => (
                <tr key={f.store}>
                  <td>{f.store}</td>
                  <td className="num">
                    {cash(f.delivery)}
                    {f.free_over !== null ? ` (free over ${cash(f.free_over)})` : ""}
                    {f.service ? ` + ${cash(f.service)} service` : ""}
                  </td>
                  <td>
                    {f.pct_off
                      ? `${f.pct_off} % off up to ${cash(f.max_off ?? 0)} on ${cash(f.min_order ?? 0)}+`
                      : f.note || <span className="muted">none</span>}
                  </td>
                  <td>{f.checked ? weekdayLabel(f.checked) : "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </TableWrap>
      )}

      {data.rules.length > 0 && (
        <>
          <h3>Your grocery rules</h3>
          <ul className="muted">
            {data.rules.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
        </>
      )}
    </section>
  );
}
