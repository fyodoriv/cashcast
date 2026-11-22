function run(argv) {
  const data = JSON.parse($.NSString.stringWithContentsOfFileEncodingError(argv[0], $.NSUTF8StringEncoding, null).js);
  const app = Application('Calendar');
  const found = app.calendars.whose({name: data.calendar})();
  let cal;
  if (found.length) { cal = found[0]; } else { cal = app.Calendar({name: data.calendar}); app.calendars.push(cal); }
  const existing = {};
  cal.events().forEach(function (e) {
    const m = /\[cashcast:([^\]]+)\]/.exec(e.description() || '');
    if (m) existing[m[1]] = e;
  });
  const wanted = {};
  let created = 0, updated = 0, removed = 0;
  data.events.forEach(function (t) {
    wanted[t.id] = true;
    const start = new Date(t.date + 'T00:00:00');
    const end = new Date(start.getTime() + 86400000);
    const body = (t.notes ? t.notes + '\n' : '') + '[cashcast:' + t.id + ']';
    let e = existing[t.id];
    if (!e) {
      e = app.Event({summary: t.title, startDate: start, endDate: end, alldayEvent: true, description: body});
      cal.events.push(e);
      created++;
    } else {
      const same = e.summary() === t.title && (e.description() || '') === body && e.startDate().toDateString() === start.toDateString();
      if (!same) {  // unchanged: no write, so a quiet hour costs Calendar nothing
        // each date write saves the event, and Calendar refuses a start on or after the end: moving later, write the end first
        e.summary = t.title;
        if (start >= e.endDate()) { e.endDate = end; e.startDate = start; } else { e.startDate = start; e.endDate = end; }
        e.alldayEvent = true; e.description = body;
        updated++;
      }
    }
    // Calendar refuses to delete an alert and ignores a changed trigger, so add ours only once.
    const has = e.displayAlarms().some(function (a) { return a.triggerInterval() === -t.alert_minutes; });
    if (t.alert_minutes && !has) e.displayAlarms.push(app.DisplayAlarm({triggerInterval: -t.alert_minutes}));
  });
  const gone = Object.keys(existing).filter(function (id) { return !wanted[id] && id.indexOf(data.owner + ':') === 0; });
  let held = 0;
  if (gone.length > (data.max_remove || 10)) { held = gone.length; }  // too many at once: the task list may be incomplete
  else { gone.forEach(function (id) { app.delete(existing[id]); removed++; }); }
  return JSON.stringify({calendar: data.calendar, created: created, updated: updated, removed: removed, held: held});
}
