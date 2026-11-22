/// <reference types="vite/client" />
import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TableWrap } from "../TableWrap";

const files = import.meta.glob(["../**/*.tsx", "!./**"], { query: "?raw", import: "default", eager: true }) as Record<
  string,
  string
>;

describe("scrolling tables", () => {
  // axe `scrollable-region-focusable`: a keyboard user must be able to focus a
  // table that scrolls sideways on a phone, or its hidden columns are unreachable.
  it("take keyboard focus", () => {
    const { container } = render(
      <TableWrap>
        <table />
      </TableWrap>,
    );
    expect((container.firstChild as HTMLElement).tabIndex).toBe(0);
  });

  it("all go through TableWrap", () => {
    expect(Object.keys(files).length).toBeGreaterThan(5);
    const raw = Object.entries(files)
      .filter(([file]) => !file.endsWith("/TableWrap.tsx"))
      .flatMap(([file, text]) =>
        text
          .split("\n")
          .map((line, i) => ({ line, at: `${file}:${i + 1}` }))
          .filter(({ line }) => line.includes('className="table-wrap"'))
          .map(({ at }) => at),
      );
    expect(raw).toEqual([]);
  });
});
