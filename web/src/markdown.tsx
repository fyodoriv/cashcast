import type { ReactNode } from "react";
import { TableWrap } from "./TableWrap";
import { friendlyDates } from "./format";

// A small Markdown renderer for the strategy sections the app writes itself and
// for the owner's documents (Notion exports): headings, paragraphs, lists, task
// lists, tables, quotes, code blocks, rules, **bold**, *italic*, `code`, links
// and images. It builds React elements only (no innerHTML), so text can never
// become markup. Images load only from the owner's own documents (`doc:` links);
// web links open in a new tab when clicked and are never fetched by the page.

/** How a document view resolves `doc:` links and images. Without it, links render as text. */
export interface DocLinks {
  open: (path: string) => void;
  image: (path: string, alt: string, key: string) => ReactNode;
}

// Notion puts a few HTML tags into its Markdown export; they are dropped, never rendered.
const HTML_TAG = /<\/?(aside|details|summary|br|span|div|p|u|mark|figure|figcaption)\b[^>]*>/gi;

function inline(source: string, key: string, links?: DocLinks): ReactNode[] {
  const out: ReactNode[] = [];
  const text = source.replace(HTML_TAG, " ");
  const re = /(!?\[[^\]]*\]\([^)\s]+\)|\*\*[^*]+\*\*|\*[^*]+\*|`[^`]+`)/g;
  let last = 0;
  let i = 0;
  for (const m of text.matchAll(re)) {
    const idx = m.index ?? 0;
    if (idx > last) out.push(friendlyDates(text.slice(last, idx)));
    const tok = m[0];
    const k = `${key}-${i++}`;
    const link = tok.match(/^(!?)\[([^\]]*)\]\(([^)\s]+)\)$/);
    if (link) out.push(renderLink(link[1] === "!", link[2], link[3], k, links));
    else if (tok.startsWith("**")) out.push(<strong key={k}>{friendlyDates(tok.slice(2, -2))}</strong>);
    else if (tok.startsWith("`")) out.push(<code key={k}>{tok.slice(1, -1)}</code>);
    else out.push(<em key={k}>{friendlyDates(tok.slice(1, -1))}</em>);
    last = idx + tok.length;
  }
  if (last < text.length) out.push(friendlyDates(text.slice(last)));
  return out;
}

function renderLink(isImage: boolean, label: string, href: string, key: string, links?: DocLinks): ReactNode {
  if (href.startsWith("doc:") && links) {
    const path = decodeURIComponent(href.slice(4));
    if (isImage || /\.(png|jpe?g|gif|webp|svg)$/i.test(path)) return links.image(path, label, key);
    if (/\.md$/i.test(path))
      return (
        <a
          key={key}
          href={`#doc=${encodeURIComponent(path)}`}
          onClick={(e) => {
            e.preventDefault();
            links.open(path);
          }}
        >
          {label || path}
        </a>
      );
    return <span key={key}>{label || path}</span>;
  }
  if (/^https?:\/\//i.test(href) && !isImage)
    return (
      <a key={key} href={href} target="_blank" rel="noreferrer noopener">
        {label || href}
      </a>
    );
  // Remote images are never loaded (the page's policy blocks them too).
  return <span key={key}>{label || (isImage ? "[image]" : href)}</span>;
}

const cells = (line: string) =>
  line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());

export function Markdown({
  text,
  skipTitle = false,
  lift = 0,
  links,
}: { text: string; skipTitle?: boolean; lift?: number; links?: DocLinks }) {
  // A folded section title (<details><summary>X</summary>) reads as a sub-heading, like its siblings.
  const lines = text.replace(/^\s*(?:<details>)?\s*<summary>(.*?)<\/summary>\s*$/gm, "### $1").split("\n");
  const blocks: ReactNode[] = [];
  let i = 0;
  let n = 0;
  while (i < lines.length) {
    const line = lines[i];
    const key = `b${n++}`;
    if (!line.trim()) {
      i++;
      continue;
    }
    const h = line.match(/^(#{1,4}) (.*)$/);
    if (h) {
      if (!(skipTitle && h[1] === "##")) {
        // `lift` raises headings one level when no section heading sits above them (no skipped level).
        const level = Math.max(2, Math.min(h[1].length + 1 - lift, 4));
        blocks.push(
          level === 2 ? (
            <h2 key={key}>{inline(h[2], key, links)}</h2>
          ) : level === 3 ? (
            <h3 key={key}>{inline(h[2], key, links)}</h3>
          ) : (
            <h4 key={key}>{inline(h[2], key, links)}</h4>
          ),
        );
      }
      i++;
      continue;
    }
    if (line.trim().startsWith("```")) {
      const code: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith("```")) code.push(lines[i++]);
      i++;
      blocks.push(
        <pre key={key}>
          <code>{code.join("\n")}</code>
        </pre>,
      );
      continue;
    }
    if (/^\s*(---|\*\*\*|___)\s*$/.test(line)) {
      blocks.push(<hr key={key} />);
      i++;
      continue;
    }
    if (line.startsWith(">")) {
      const quote: string[] = [];
      while (i < lines.length && lines[i].startsWith(">")) quote.push(lines[i++].replace(/^>\s?/, ""));
      blocks.push(<blockquote key={key}>{inline(quote.join(" "), key, links)}</blockquote>);
      continue;
    }
    if (line.trim().startsWith("|")) {
      const rows: string[] = [];
      while (i < lines.length && lines[i].trim().startsWith("|")) rows.push(lines[i++]);
      const [head, , ...body] = rows;
      // A column whose cells are all amounts or rates is right-aligned with tabular figures.
      const isNum = (c: string) => /^[-+−]?\$?[\d,.]+%?( \/ [-+]?\$?[\d,.]+%?)*$/.test(c.replace(/\*/g, "").trim());
      const numCol = cells(head).map((_, j) => body.length > 0 && body.every((r) => isNum(cells(r)[j] ?? "")));
      blocks.push(
        <TableWrap key={key}>
          <table>
            <thead>
              <tr>
                {cells(head).map((c, j) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: table cells are static text with no identity besides position
                  <th key={`${key}h${j}`} className={numCol[j] ? "num" : undefined}>
                    {inline(c, `${key}h${j}`, links)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {body.map((r, ri) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: table cells are static text with no identity besides position
                <tr key={`${key}r${ri}`}>
                  {cells(r).map((c, j) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: table cells are static text with no identity besides position
                    <td key={`${key}r${ri}c${j}`} className={numCol[j] ? "num" : undefined}>
                      {inline(c, `${key}r${ri}c${j}`, links)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </TableWrap>,
      );
      continue;
    }
    if (/^\s*([-*]|\d+\.) /.test(line)) {
      const ordered = /^\s*\d+\. /.test(line);
      const items: ReactNode[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.) /.test(lines[i])) {
        const raw = lines[i].replace(/^\s*([-*]|\d+\.) /, "");
        const k = `${key}i${items.length}`;
        const task = raw.match(/^\[( |x)\] (.*)$/);
        items.push(
          <li key={k} className={task ? "task" : undefined}>
            {task ? <input type="checkbox" checked={task[1] === "x"} readOnly aria-label="done" /> : null}
            {inline(task ? task[2] : raw, k, links)}
          </li>,
        );
        i++;
      }
      blocks.push(ordered ? <ol key={key}>{items}</ol> : <ul key={key}>{items}</ul>);
      continue;
    }
    // The first line always joins the paragraph: a line like "#1 goal" matches no block
    // above, and leaving it unread would loop forever.
    const para: string[] = [lines[i++]];
    while (i < lines.length && lines[i].trim() && !/^(#|\||>|```|\s*([-*]|\d+\.) )/.test(lines[i]))
      para.push(lines[i++]);
    blocks.push(<p key={key}>{inline(para.join(" "), key, links)}</p>);
  }
  return <div className="md">{blocks}</div>;
}
