import { dayLabel, friendlyDates } from "./format";
import type { PlanTask } from "./types";

const riskLabel: Record<PlanTask["risk"], string> = { money: "you do this", settings: "approve first", read: "" };

/** A checklist: date pill (amber and "overdue" when late), title, and who acts. */
export function TaskList({ tasks, today, empty }: { tasks: PlanTask[]; today: string; empty?: string }) {
  if (tasks.length === 0) return empty ? <p className="muted">{empty}</p> : null;
  return (
    <ul className="tasks">
      {tasks.map((t) => (
        <li key={t.id} className={`risk-${t.risk}${t.due && t.due < today ? " overdue" : ""}`}>
          {t.due ? (
            <time dateTime={t.due}>
              {dayLabel(t.due)}
              {t.due < today && " · overdue"}
            </time>
          ) : (
            <time>later</time>
          )}
          <span className="task-title">{friendlyDates(t.title)}</span>
          {riskLabel[t.risk] && <span className={`chip risk-${t.risk}`}>{riskLabel[t.risk]}</span>}
        </li>
      ))}
    </ul>
  );
}
