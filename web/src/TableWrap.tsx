import { type ReactNode, useLayoutEffect, useRef } from "react";

/** A table that scrolls sideways on a phone. It takes keyboard focus, so a
 * keyboard user can scroll to the hidden columns (axe `scrollable-region-focusable`).
 * By default (`stack`), a phone shows each row as a card instead: every cell gets its
 * column header as a label (`data-label`), so no column is cut off. */
export function TableWrap({ children, stack = true }: { children: ReactNode; stack?: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  // Runs after every render: the rows can change, and labeling is cheap.
  useLayoutEffect(() => {
    if (!stack || !box.current) return;
    for (const table of box.current.querySelectorAll("table")) {
      const heads = [...table.querySelectorAll("thead th")].map((th) => th.textContent?.trim() ?? "");
      table.setAttribute("role", "table");
      for (const th of table.querySelectorAll("thead th")) th.setAttribute("role", "columnheader");
      for (const tr of table.querySelectorAll("tr")) tr.setAttribute("role", "row");
      for (const row of table.querySelectorAll("tbody tr")) {
        [...row.children].forEach((cell, j) => {
          if (heads[j]) cell.setAttribute("data-label", heads[j]);
          // display: block on phone drops table semantics in WebKit: name the roles.
          cell.setAttribute("role", "cell");
        });
      }
    }
  });
  return (
    // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrolling region must take focus (WCAG 2.1.1)
    <div className={stack ? "table-wrap stack" : "table-wrap"} tabIndex={0} ref={box}>
      {children}
    </div>
  );
}
