import { type ReactNode, useEffect, useState } from "react";
import { fetchDoc, fetchDocFile, fetchDocs } from "../api";
import { type DocLinks, Markdown } from "../markdown";
import type { Doc, DocEntry } from "../types";

export interface DocsApi {
  list: (user: string, q?: string) => Promise<DocEntry[]>;
  read: (user: string, path: string) => Promise<Doc>;
  file: (user: string, path: string) => Promise<string>;
}

const realApi: DocsApi = { list: fetchDocs, read: fetchDoc, file: fetchDocFile };

/** An image from the owner's documents, fetched with the token and shown from a blob: URL. */
function DocImage({ user, path, alt, api }: { user: string; path: string; alt: string; api: DocsApi }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let url: string | null = null;
    let live = true;
    api
      .file(user, path)
      .then((u) => {
        url = u;
        if (live) setSrc(u);
      })
      .catch(() => setSrc(null));
    return () => {
      live = false;
      if (url) URL.revokeObjectURL?.(url);
    };
  }, [user, path, api]);
  return src ? <img className="doc-img" src={src} alt={alt} /> : <span className="muted">[{alt || "image"}]</span>;
}

/** Your notes and records, moved out of Notion: a searchable list and a reader. Read-only, this computer only. */
export function Documents({ user, api = realApi }: { user: string; api?: DocsApi }) {
  const [list, setList] = useState<DocEntry[] | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<DocEntry[] | null>(null);
  const [doc, setDoc] = useState<Doc | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    api
      .list(user)
      .then(setList)
      .catch(() => setList([]));
  }, [user, api]);

  useEffect(() => {
    if (!query.trim()) {
      setHits(null);
      return;
    }
    const t = setTimeout(() => {
      api
        .list(user, query)
        .then(setHits)
        .catch(() => setHits([]));
    }, 200);
    return () => clearTimeout(t);
  }, [user, query, api]);

  const open = (path: string) => {
    setError("");
    api
      .read(user, path)
      .then((d) => {
        setDoc(d);
        document.querySelector(".doc-reader")?.scrollIntoView?.({ block: "start" });
      })
      .catch(() => setError("That document is not in your documents folder."));
  };

  const links: DocLinks = {
    open,
    image: (path: string, alt: string, key: string): ReactNode => (
      <DocImage key={key} user={user} path={path} alt={alt} api={api} />
    ),
  };

  const shown = hits ?? list ?? [];
  const groups = new Map<string, DocEntry[]>();
  for (const d of shown) groups.set(d.folder, [...(groups.get(d.folder) ?? []), d]);

  return (
    <section>
      <div className="page-head">
        <h2>Documents</h2>
        <p className="lead">
          Your finance notes and records, for example moved from Notion with{" "}
          <code>bin/cashcast notion import EXPORT.zip</code>. They stay in your private folder; this page loads nothing
          from the internet.
        </p>
      </div>
      {list !== null && list.length === 0 ? (
        <div className="panel">
          <h3>No documents yet</h3>
          <ol className="steps">
            <li>
              In Notion, open your Finances page, then <b>••• → Export</b>: format <b>Markdown &amp; CSV</b>, include
              subpages, create folders for subpages.
            </li>
            <li>
              In Terminal: <code>bin/cashcast notion import ~/Downloads/Export.zip --delete-source</code>
            </li>
            <li>Come back here. Your own .md files in your docs folder show up too.</li>
          </ol>
        </div>
      ) : (
        <div className="docs">
          <div className="panel doc-list">
            <input
              type="search"
              className="doc-search"
              placeholder="Search your documents"
              aria-label="search documents"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            {list === null && <p className="muted">Loading…</p>}
            {hits !== null && hits.length === 0 && <p className="muted">No match.</p>}
            {[...groups.entries()].map(([folder, docs]) => (
              <div key={folder || "/"} className="doc-group">
                {folder && (
                  <span className="nav-title">
                    {folder
                      .replace(/^notion(?=\/|$)/, "Notion")
                      .split("/")
                      .join(" / ")}
                  </span>
                )}
                <ul>
                  {docs.map((d) => (
                    <li key={d.path}>
                      <button
                        type="button"
                        className="doc-link"
                        aria-current={doc?.path === d.path ? "page" : undefined}
                        onClick={() => open(d.path)}
                      >
                        {d.title}
                        {d.snippet && <span className="doc-snippet">{d.snippet}</span>}
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
          <article className="panel doc-reader">
            {error && <p className="banner">{error}</p>}
            {doc ? (
              <>
                <p className="muted doc-path">{doc.path}</p>
                <Markdown text={doc.markdown} links={links} />
              </>
            ) : (
              <p className="muted">Pick a document on the left.</p>
            )}
          </article>
        </div>
      )}
    </section>
  );
}
