// The grocery-cart browser helpers (skills/grocery-cart/search.js): the Skip
// adapter reads a synthetic store page once and returns `skip:<store>` rows,
// and the Skip+ fee comes from the page text. Moved from sidequest with the skill.
import { expect, test } from "vitest";

const raw = (files: Record<string, unknown>) => Object.values(files)[0] as string;
const page = raw(import.meta.glob("./skip-store.html", { query: "?raw", import: "default", eager: true }));
const script = raw(
  import.meta.glob("../../../skills/grocery-cart/search.js", { query: "?raw", import: "default", eager: true }),
);

function load() {
  let fetches = 0;
  // In a browser, `window` is the global object.
  const ctx: Record<string, unknown> = {
    location: { host: "www.skipthedishes.com", origin: "https://www.skipthedishes.com", pathname: "/test-mart" },
    fetch: async () => {
      fetches++;
      return { text: async () => page };
    },
    performance,
  };
  ctx.window = ctx;
  // In a browser `window` is the global object: `with` makes ctx the global scope here.
  new Function("ctx", `with (ctx) {\n${script}\n}`)(ctx);
  return { window: ctx as Record<string, (...a: unknown[]) => unknown>, fetches: () => fetches };
}

test("Skip adapter reads the menu once, as skip:<store> rows", async () => {
  const { window, fetches } = load();
  const rows = await window.sqRows(["kettle chips", "coconut water"]);
  expect(rows).toEqual([
    expect.objectContaining({ item: "kettle chips", store: "skip:Test Mart", price: 4.99, unit: "200g" }),
    expect.objectContaining({ item: "coconut water", store: "skip:Test Mart", price: 6.99, unit: "1L" }),
  ]);
  expect(fetches()).toBe(1);
});

test("Skip+ fee comes from the store page text", () => {
  const { window } = load();
  expect(window.sqSkipFee("Service fees apply\n$2.99\n$0.00 Delivery Fee Over $15.00")).toEqual({
    delivery: 2.99,
    free_over: 15,
    note: expect.stringMatching(/saved address differs/),
  });
  expect(window.sqSkipFee("no fee here")).toBeNull();
});
