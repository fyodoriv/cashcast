import { Markdown } from "../markdown";
import type { Plan } from "../types";

export function SectionText({ plan, id, lift = 0 }: { plan: Plan; id: string; lift?: number }) {
  const s = plan.sections.find((x) => x.id === id);
  if (!s) return <p className="muted">This section has no data yet.</p>;
  return <Markdown text={s.markdown} skipTitle lift={lift} />;
}

/** A view made of one or more strategy sections, each with its own heading. */
export function SectionView({ plan, title, ids }: { plan: Plan; title: string; ids: string[] }) {
  return (
    <section>
      <div className="page-head">
        <h2>{title}</h2>
      </div>
      {ids.map((id) => {
        const s = plan.sections.find((x) => x.id === id);
        return (
          <article key={id} className="panel">
            {ids.length > 1 && <h3>{s?.title ?? id}</h3>}
            <SectionText plan={plan} id={id} lift={ids.length > 1 ? 0 : 1} />
          </article>
        );
      })}
    </section>
  );
}
