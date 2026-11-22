// Runs a cashcast JavaScript-for-Automation sync script against a fake
// Reminders store (EventKit) or Calendar app, so tests check the real script without macOS.
// usage: node jxa_reminders_calendar.js SCRIPT PAYLOAD STATE  → prints {"result": ..., "state": ...}
// STATE: {"lists": {name: [{name, body, completed, completed_at, due}]}} or {"calendars": {name: [{summary, description, start, alarms}]}}
// (alarms: trigger minutes). Like Calendar on macOS 27: a new event gets the default all-day alert (540),
// deleting an alert fails, and setting an alert's trigger is ignored.
'use strict';
const fs = require('fs');
const vm = require('vm');
const [scriptPath, payloadPath, statePath] = process.argv.slice(2);
const state = JSON.parse(fs.readFileSync(statePath, 'utf8'));

// A JXA-like object: `obj.prop()` reads, `obj.prop = v` writes.
function element(fields) {
  const o = {};
  for (const key of Object.keys(fields)) {
    let value = fields[key];
    Object.defineProperty(o, key, {
      get: () => () => value,
      set: (v) => { value = v; },
      enumerable: true,
      configurable: true,
    });
  }
  o._fields = () => Object.fromEntries(Object.keys(fields).map((k) => [k, o[k]()]));
  return o;
}

// Like Calendar: each date write saves the event, and a start on or after the end fails the save.
function guardDates(ev) {
  for (const key of ['startDate', 'endDate']) {
    const d = Object.getOwnPropertyDescriptor(ev, key);
    Object.defineProperty(ev, key, {
      get: d.get,
      set: (v) => {
        const start = key === 'startDate' ? v : ev.startDate();
        const end = key === 'endDate' ? v : ev.endDate();
        if (start >= end) throw new Error('Failed to save event [' + ev.summary() + '], with error [The start date must be before the end date.]');
        d.set(v);
      },
      enumerable: true,
    });
  }
  return ev;
}

const DEFAULT_ALERT = 540;
function alarm(trigger) {
  const a = {_alarm: true};
  Object.defineProperty(a, 'triggerInterval', {get: () => () => trigger, set: () => {}, enumerable: true});
  return a;
}

function collection(items) {
  const f = () => items.slice();
  f.push = (x) => items.push(x);
  f.whose = (q) => () => items.filter((x) => Object.keys(q).every((k) => x[k]() === q[k]));
  f._items = items;
  return f;
}

const containers = {};
function container(kind, name, rows) {
  const make = kind === 'list'
    ? (r) => element({name: r.name, body: r.body || '', completed: !!r.completed, completionDate: r.completed_at ? new Date(r.completed_at) : null, dueDate: r.due ? new Date(r.due) : null})
    : (e) => guardDates(element({summary: e.summary, description: e.description || '', startDate: new Date(e.start), endDate: new Date(new Date(e.start).getTime() + 86400000), alldayEvent: true}));
  const c = element({name});
  const items = rows.map(make);
  if (kind === 'list') c.reminders = collection(items);
  else c.events = collection(items);
  items.forEach((it, i) => { it.displayAlarms = collection((rows[i].alarms || []).map(alarm)); });
  containers[name] = c;
  return c;
}

function app(which) {
  const isReminders = which === 'Reminders';
  const top = [];
  for (const [name, rows] of Object.entries((isReminders ? state.lists : state.calendars) || {})) {
    top.push(container(isReminders ? 'list' : 'calendar', name, rows));
  }
  const a = {
    delete(x) {
      if (x && x._alarm) throw new Error('AppleEvent handler failed.');
      for (const c of top) {
        const items = (isReminders ? c.reminders : c.events)._items;
        const i = items.indexOf(x);
        if (i >= 0) { items.splice(i, 1); return; }
      }
    },
    Reminder: (p) => element({name: p.name, body: p.body || '', completed: false, completionDate: null, dueDate: null}),
    Event: (p) => { const e = element(p); e.displayAlarms = collection([alarm(DEFAULT_ALERT)]); return e; },
    DisplayAlarm: (p) => alarm(p.triggerInterval),
    List: (p) => { const c = element({name: p.name}); c.reminders = collection([]); return c; },
    Calendar: (p) => { const c = element({name: p.name}); c.events = collection([]); return c; },
  };
  if (isReminders) a.lists = collection(top); else a.calendars = collection(top);
  a._top = top;
  return a;
}

// EventKit, as the Reminders sync uses it: lists are calendars of reminders, a fetch answers
// through a callback on a later run-loop turn, and nothing is stored until save + commit.
const NIL = {isNil: () => true};
function nsdate(d) { return d ? {timeIntervalSince1970: d.getTime() / 1000, isNil: () => false} : NIL; }
function nsarray(items) { return {count: items.length, objectAtIndex: (i) => items[i]}; }
function dueComps(due) {
  if (!due) return NIL;
  const [y, m, d] = due.slice(0, 10).split('-').map(Number);
  return {year: y, month: m, day: d, hour: 9, minute: 0, isNil: () => false};
}
let ek = null, pending = null;
function eventKit() {
  const cals = Object.entries(state.lists || {}).map(([name, rows]) => {
    const cal = {title: name, items: []};
    cal.items = rows.map((r) => ({
      title: r.name, notes: r.body || '', completed: !!r.completed, calendar: cal,
      completionDate: nsdate(r.completed_at ? new Date(r.completed_at) : null), dueDateComponents: dueComps(r.due),
    }));
    return cal;
  });
  const staged = new Set(), removed = new Set();
  const store = {
    calendarsForEntityType: () => nsarray(cals),
    defaultCalendarForNewReminders: {source: 'iCloud'},
    saveCalendarCommitError: (c) => { c.items = []; cals.push(c); return true; },
    predicateForRemindersInCalendars: (a) => a,
    fetchRemindersMatchingPredicateCompletion: (pred, cb) => {
      const out = [];
      for (let i = 0; i < pred.count; i++) out.push(...pred.objectAtIndex(i).items);
      pending = () => cb(nsarray(out));
    },
    saveReminderCommitError: (r) => { staged.add(r); return true; },
    removeReminderCommitError: (r) => { removed.add(r); return true; },
    commit: () => {
      for (const r of staged) if (!r.calendar.items.includes(r)) r.calendar.items.push(r);
      for (const r of removed) r.calendar.items.splice(r.calendar.items.indexOf(r), 1);
      store.commits++;
      return true;
    },
    commits: 0,
    _cals: cals,
  };
  return store;
}

let theApp;
const sandbox = {
  Application: (name) => (theApp = app(name)),
  ObjC: {import: () => {}, unwrap: (x) => x},
  $: {
    NSString: {stringWithContentsOfFileEncodingError: (p) => ({js: fs.readFileSync(p, 'utf8')})},
    NSUTF8StringEncoding: 4,
    EKEntityTypeReminder: 1,
    EKEventStore: {authorizationStatusForEntityType: () => 3, alloc: {get init() { return (ek = eventKit()); }}},
    EKCalendar: {calendarForEntityTypeEventStore: () => ({title: '', source: null})},
    EKReminder: {reminderWithEventStore: () => ({title: '', notes: '', completed: false, completionDate: NIL, dueDateComponents: NIL})},
    NSArray: {arrayWithObject: (x) => nsarray([x])},
    NSDateComponents: {alloc: {get init() { return {isNil: () => false}; }}},
    NSDate: {dateWithTimeIntervalSinceNow: () => null},
    NSRunLoop: {currentRunLoop: {runUntilDate: () => { if (pending) { const p = pending; pending = null; p(); } }}},
  },
  JSON, Date, Object, Number, Error,
};
vm.createContext(sandbox);
vm.runInContext(fs.readFileSync(scriptPath, 'utf8'), sandbox);
const result = JSON.parse(sandbox.run([payloadPath]));
if (ek) {
  const lists = {};
  for (const c of ek._cals) {
    lists[c.title] = c.items.map((r) => {
      const at = r.completionDate.isNil() ? null : new Date(r.completionDate.timeIntervalSince1970 * 1000);
      const due = r.dueDateComponents.isNil() ? null
        : `${r.dueDateComponents.year}-${String(r.dueDateComponents.month).padStart(2, '0')}-${String(r.dueDateComponents.day).padStart(2, '0')}`;
      return {name: r.title, body: r.notes, completed: r.completed, completed_at: at ? at.toISOString() : null, due};
    });
  }
  process.stdout.write(JSON.stringify({result, state: {lists}, commits: ek.commits}));
  process.exit(0);
}
const out = {};
const isReminders = !!theApp.lists;
for (const c of theApp._top) {
  const items = (isReminders ? c.reminders : c.events)._items;
  out[c.name()] = items.map((x) => {
    const f = x._fields();
    if (isReminders) {
      return {name: f.name, body: f.body, completed: f.completed, completed_at: f.completionDate ? f.completionDate.toISOString() : null,
              due: f.dueDate ? f.dueDate.toISOString() : null};
    }
    return {summary: f.summary, description: f.description, start: f.startDate.toISOString(), alarms: x.displayAlarms._items.map((a) => a.triggerInterval())};
  });
}
process.stdout.write(JSON.stringify({result, state: isReminders ? {lists: out} : {calendars: out}}));
