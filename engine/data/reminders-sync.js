ObjC.import('EventKit');
function run(argv) {
  const data = JSON.parse($.NSString.stringWithContentsOfFileEncodingError(argv[0], $.NSUTF8StringEncoding, null).js);
  const REM = $.EKEntityTypeReminder;
  if (Number($.EKEventStore.authorizationStatusForEntityType(REM)) !== 3) {
    throw new Error('no full Reminders access: System Settings > Privacy & Security > Reminders > allow Terminal');
  }
  const store = $.EKEventStore.alloc.init;
  function nil(x) { return x === undefined || x === null || (typeof x.isNil === 'function' && x.isNil()); }
  function str(x) { return nil(x) ? '' : ObjC.unwrap(x); }
  function when(d) { return nil(d) ? null : new Date(Number(d.timeIntervalSince1970) * 1000); }
  function day(c) {
    if (nil(c)) return null;
    const two = function (n) { return ('0' + Number(n)).slice(-2); };
    return Number(c.year) + '-' + two(c.month) + '-' + two(c.day);
  }
  function wait(done) {  // EventKit answers through a callback: run the loop until it does
    const end = Date.now() + 60000;
    while (!done() && Date.now() < end) $.NSRunLoop.currentRunLoop.runUntilDate($.NSDate.dateWithTimeIntervalSinceNow(0.05));
    if (!done()) throw new Error('Reminders did not answer within 60 s');
  }
  function save(r) { if (!store.saveReminderCommitError(r, false, null)) throw new Error('could not save a reminder'); }
  const lists = store.calendarsForEntityType(REM);
  let list = null;
  for (let i = 0; i < Number(lists.count); i++) {
    const c = lists.objectAtIndex(i);
    if (str(c.title) === data.list) { list = c; break; }
  }
  if (!list) {
    list = $.EKCalendar.calendarForEntityTypeEventStore(REM, store);
    list.title = data.list;
    list.source = store.defaultCalendarForNewReminders.source;
    if (!store.saveCalendarCommitError(list, true, null)) throw new Error('could not create the list');
  }
  let rows = null;
  store.fetchRemindersMatchingPredicateCompletion(
    store.predicateForRemindersInCalendars($.NSArray.arrayWithObject(list)), function (rs) { rows = rs; });
  wait(function () { return rows !== null; });
  const existing = {};
  for (let i = 0; i < Number(rows.count); i++) {
    const r = rows.objectAtIndex(i);
    const m = /\[cashcast:([^\]]+)\]/.exec(str(r.notes));
    if (m) existing[m[1]] = r;
  }
  const wanted = {}, done = {}, open = {};
  (data.done || []).forEach(function (id) { done[id] = true; });
  (data.open || []).forEach(function (id) { open[id] = true; });
  const ours = {};  // reminders this sync completed itself last time: never read back as your tick
  (data.closed_by_sync || []).forEach(function (id) { ours[id] = true; });
  const since = data.since ? new Date(data.since) : null;
  function fresh(r) { const at = when(r.completionDate); return !since || !at || at > since; }
  const completed = [];
  let created = 0, updated = 0, kept = 0, reopened = 0, closed = 0, removed = 0, held = 0, saved = 0;
  const closedIds = [];
  data.tasks.forEach(function (t) {
    wanted[t.id] = true;
    const body = (t.notes ? t.notes + '\n' : '') + '[cashcast:' + t.id + ']';
    let r = existing[t.id], dirty = false;
    if (r && r.completed) {
      if (fresh(r) && !ours[t.id]) { completed.push(t.id); kept++; return; }  // ticked since the last sync: cashcast records it
      r.completed = false; reopened++; dirty = true;  // an old tick on a task that is open again (a new year, or undone)
    }
    if (!r) { r = $.EKReminder.reminderWithEventStore(store); r.calendar = list; created++; dirty = true; }
    else if (str(r.title) !== t.title || str(r.notes) !== body || (t.due && day(r.dueDateComponents) !== t.due)) { updated++; dirty = true; }
    if (!dirty) return;  // unchanged: no write, so a quiet hour costs Reminders nothing
    r.title = t.title; r.notes = body;
    if (t.due) {
      const c = $.NSDateComponents.alloc.init, p = t.due.split('-');
      c.year = Number(p[0]); c.month = Number(p[1]); c.day = Number(p[2]); c.hour = 9; c.minute = 0;
      r.dueDateComponents = c;
    }
    save(r); saved++;
  });
  Object.keys(open).forEach(function (id) {  // open but not listed (a week overdue): a tick still counts
    const r = existing[id];
    if (wanted[id] || !r || !r.completed) return;
    if (fresh(r) && !ours[id]) completed.push(id);
    else { r.completed = false; reopened++; save(r); saved++; }  // an old tick on a task that is open again
  });
  const gone = Object.keys(existing).filter(function (id) {
    // only this person's own marked, open reminders whose task is no longer open in cashcast
    return !wanted[id] && !open[id] && id.indexOf(data.owner + ':') === 0 && !existing[id].completed;
  });
  const deletions = gone.filter(function (id) { return !done[id]; });
  gone.forEach(function (id) {
    const r = existing[id];
    if (done[id]) { r.completed = true; save(r); saved++; closed++; closedIds.push(id); }
    else if (deletions.length > data.max_remove) { held++; }  // too many at once: the plan may be incomplete
    else { store.removeReminderCommitError(r, false, null); saved++; removed++; }
  });
  if (saved && !store.commit(null)) throw new Error('could not save the changes to Reminders');
  return JSON.stringify({list: data.list, created: created, updated: updated, completed_kept: kept, reopened: reopened,
                         closed: closed, closed_ids: closedIds, removed: removed, held: held, completed: completed});
}
