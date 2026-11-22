//! The part of Python's `argparse` (3.13) the CLI uses, ported so `bin/cashcast`
//! parses, errs and prints help as the Python CLI did (decision 0010).
//!
//! It covers what `cli.parser()` declares: options that store a value, a
//! switch or a list (`append`), `-h/--help`, positionals with one or an
//! optional value, `type=int`, choices, and one level of subcommands. The
//! parse follows `_parse_known_args` step by step: the A/O pattern, prefix
//! matching, `--`, `--name=value`, positionals consumed between options.
//! Help and usage follow `HelpFormatter`, wrapped to the terminal width less 2.

use regex::Regex;

/// A parsed value, as argparse stores it in the namespace.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    None,
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Store,
    StoreTrue,
    Append,
    Help,
}

/// One `add_argument`.
#[derive(Clone, Debug)]
pub struct Arg {
    pub dest: &'static str,
    /// Option strings; empty for a positional.
    pub flags: Vec<&'static str>,
    pub kind: Kind,
    /// `nargs="?"` (positionals only here).
    pub optional: bool,
    pub choices: Option<Vec<&'static str>>,
    pub default: Val,
    pub help: Option<&'static str>,
    pub metavar: Option<&'static str>,
    pub int: bool,
}

impl Arg {
    pub fn positional(dest: &'static str) -> Self {
        Arg {
            dest,
            flags: Vec::new(),
            kind: Kind::Store,
            optional: false,
            choices: None,
            default: Val::None,
            help: None,
            metavar: None,
            int: false,
        }
    }
    pub fn option(flag: &'static str) -> Self {
        // argparse's dest: `--dry-run` → `dry_run` (built once per run).
        let dest = flag.trim_start_matches('-').replace('-', "_");
        let mut a = Arg::positional(Box::leak(dest.into_boxed_str()));
        a.flags = vec![flag];
        a
    }
    pub fn switch(flag: &'static str) -> Self {
        let mut a = Arg::option(flag);
        a.kind = Kind::StoreTrue;
        a.default = Val::Bool(false);
        a
    }
    pub fn maybe(mut self) -> Self {
        self.optional = true;
        self
    }
    pub fn choices(mut self, c: &[&'static str]) -> Self {
        self.choices = Some(c.to_vec());
        self
    }
    pub fn default(mut self, v: Val) -> Self {
        self.default = v;
        self
    }
    pub fn help(mut self, h: &'static str) -> Self {
        self.help = Some(h);
        self
    }
    pub fn metavar(mut self, m: &'static str) -> Self {
        self.metavar = Some(m);
        self
    }
    pub fn int(mut self) -> Self {
        self.int = true;
        self
    }
    pub fn append(mut self) -> Self {
        self.kind = Kind::Append;
        self
    }
    fn takes_value(&self) -> bool {
        matches!(self.kind, Kind::Store | Kind::Append)
    }
    fn is_positional(&self) -> bool {
        self.flags.is_empty()
    }
    /// `_get_action_name`.
    fn name(&self) -> String {
        if !self.flags.is_empty() {
            self.flags.join("/")
        } else if let Some(m) = self.metavar {
            m.to_string()
        } else {
            self.dest.to_string()
        }
    }
}

/// `add_subparsers(dest=…)`: the command word and one parser per command.
pub struct Sub {
    pub dest: &'static str,
    pub parsers: Vec<(&'static str, &'static str, Parser)>,
}

/// One `ArgumentParser`.
pub struct Parser {
    pub prog: String,
    pub args: Vec<Arg>,
    pub sub: Option<Sub>,
}

impl Parser {
    pub fn new(prog: &str, add_help: bool) -> Self {
        let mut args = Vec::new();
        if add_help {
            let mut h = Arg::positional("help");
            h.flags = vec!["-h", "--help"];
            h.kind = Kind::Help;
            h.help = Some("show this help message and exit");
            args.push(h);
        }
        Parser {
            prog: prog.to_string(),
            args,
            sub: None,
        }
    }
    pub fn add(&mut self, a: Arg) -> &mut Self {
        self.args.push(a);
        self
    }
}

/// The parsed values, in the order argparse sets them.
#[derive(Default, Debug)]
pub struct Namespace(pub Vec<(String, Val)>);

impl Namespace {
    pub fn get(&self, dest: &str) -> &Val {
        self.0
            .iter()
            .find(|(k, _)| k == dest)
            .map_or(&Val::None, |(_, v)| v)
    }
    fn set(&mut self, dest: &str, v: Val) {
        match self.0.iter_mut().find(|(k, _)| k == dest) {
            Some(slot) => slot.1 = v,
            None => self.0.push((dest.to_string(), v)),
        }
    }
    fn has(&self, dest: &str) -> bool {
        self.0.iter().any(|(k, _)| k == dest)
    }
    pub fn str(&self, dest: &str) -> Option<String> {
        match self.get(dest) {
            Val::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
    pub fn flag(&self, dest: &str) -> bool {
        matches!(self.get(dest), Val::Bool(true))
    }
    pub fn list(&self, dest: &str) -> Vec<String> {
        match self.get(dest) {
            Val::List(l) => l.clone(),
            _ => Vec::new(),
        }
    }
}

/// argparse ended the run: help on stdout (code 0) or a usage error on stderr (code 2).
#[derive(Debug, PartialEq)]
pub struct Exit {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// An `ArgumentError`: the action's name (if any) and the message.
struct ArgError(Option<String>, String);

impl ArgError {
    fn text(&self) -> String {
        match &self.0 {
            Some(n) => format!("argument {n}: {}", self.1),
            None => self.1.clone(),
        }
    }
}

enum Fail {
    Arg(ArgError),
    Exit(Exit),
}

impl From<Exit> for Fail {
    fn from(e: Exit) -> Self {
        Fail::Exit(e)
    }
}

/// Python `repr()` of a str.
pub fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `int(text)` for a str: spaces around, a sign, `_` between digits.
pub fn py_int(text: &str) -> Option<i64> {
    let t = text.trim();
    let (neg, digits) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(t)),
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
    {
        return None;
    }
    // Python ints have no limit: past i64 saturate, so the command's own
    // range check fails later (exit 1) as Python's does.
    let mut n: i128 = 0;
    for c in digits.chars().filter(|c| *c != '_') {
        let d = c.to_digit(10).or_else(|| {
            // Python accepts any Unicode decimal digit.
            c.is_numeric().then(|| unicode_digit(c)).flatten()
        })?;
        n = (n * 10 + d as i128).min(i64::MAX as i128 + 1);
    }
    let n = if neg { -n } else { n };
    Some(n.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

fn unicode_digit(c: char) -> Option<u32> {
    // Decimal digit blocks start at a code point ending in 0.
    let cp = c as u32;
    for zero in [
        0x660, 0x6F0, 0x7C0, 0x966, 0x9E6, 0xA66, 0xAE6, 0xB66, 0xBE6, 0xC66, 0xCE6, 0xD66, 0xE50,
        0xED0, 0xF20, 0x1040, 0xFF10,
    ] {
        if (zero..zero + 10).contains(&cp) {
            return Some(cp - zero);
        }
    }
    None
}

/// The width argparse wraps to: `shutil.get_terminal_size().columns - 2`.
pub fn width() -> usize {
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| py_int(&c))
        .filter(|c| *c > 0)
        .map(|c| c as usize)
        .or_else(terminal_columns)
        .unwrap_or(80);
    cols.saturating_sub(2)
}

#[cfg(unix)]
fn terminal_columns() -> Option<usize> {
    #[repr(C)]
    struct Winsize {
        rows: u16,
        cols: u16,
        x: u16,
        y: u16,
    }
    unsafe extern "C" {
        fn ioctl(fd: i32, request: std::ffi::c_ulong, ...) -> i32;
    }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    const TIOCGWINSZ: std::ffi::c_ulong = 0x4008_7468;
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "freebsd")))]
    const TIOCGWINSZ: std::ffi::c_ulong = 0x5413;
    let mut ws = Winsize {
        rows: 0,
        cols: 0,
        x: 0,
        y: 0,
    };
    // SAFETY: TIOCGWINSZ writes one winsize struct into `ws`.
    let r = unsafe { ioctl(1, TIOCGWINSZ, &mut ws as *mut Winsize) };
    (r == 0 && ws.cols > 0).then_some(ws.cols as usize)
}

#[cfg(not(unix))]
fn terminal_columns() -> Option<usize> {
    None
}

/// One `_parse_optional` reading: the action (None: unknown), the option
/// string, the separator and the explicit argument.
#[derive(Clone)]
struct OptTuple {
    action: Option<usize>,
    option: String,
    sep: Option<String>,
    explicit: Option<String>,
}

impl Parser {
    /// Option strings in the order they were added (Python dict order).
    fn option_map(&self) -> Vec<(&'static str, usize)> {
        let mut m = Vec::new();
        for (i, a) in self.args.iter().enumerate() {
            for f in &a.flags {
                m.push((*f, i));
            }
        }
        m
    }

    fn lookup(&self, s: &str) -> Option<usize> {
        self.option_map()
            .into_iter()
            .find(|(f, _)| *f == s)
            .map(|(_, i)| i)
    }

    fn parse_optional(&self, arg: &str) -> Option<Vec<OptTuple>> {
        if arg.is_empty() || !arg.starts_with('-') {
            return None;
        }
        if let Some(i) = self.lookup(arg) {
            return Some(vec![OptTuple {
                action: Some(i),
                option: arg.into(),
                sep: None,
                explicit: None,
            }]);
        }
        if arg.chars().count() == 1 {
            return None;
        }
        if let Some((name, value)) = arg.split_once('=')
            && let Some(i) = self.lookup(name)
        {
            return Some(vec![OptTuple {
                action: Some(i),
                option: name.into(),
                sep: Some("=".into()),
                explicit: Some(value.into()),
            }]);
        }
        let tuples = self.option_tuples(arg);
        if !tuples.is_empty() {
            return Some(tuples);
        }
        let negative = Regex::new(r"^-\d+$|^-\d*\.\d+$").expect("regex");
        if negative.is_match(arg) {
            return None;
        }
        if arg.contains(' ') {
            return None;
        }
        Some(vec![OptTuple {
            action: None,
            option: arg.into(),
            sep: None,
            explicit: None,
        }])
    }

    fn option_tuples(&self, arg: &str) -> Vec<OptTuple> {
        let mut out = Vec::new();
        let (prefix, sep, explicit) = match arg.split_once('=') {
            Some((p, e)) => (p, Some("=".to_string()), Some(e.to_string())),
            None => (arg, None, None),
        };
        let second = arg.chars().nth(1);
        if second == Some('-') {
            for (f, i) in self.option_map() {
                if f.starts_with(prefix) {
                    out.push(OptTuple {
                        action: Some(i),
                        option: f.into(),
                        sep: sep.clone(),
                        explicit: explicit.clone(),
                    });
                }
            }
        } else {
            let cut = arg.char_indices().nth(2).map_or(arg.len(), |(i, _)| i);
            let (short, short_explicit) = arg.split_at(cut);
            for (f, i) in self.option_map() {
                if f == short {
                    out.push(OptTuple {
                        action: Some(i),
                        option: f.into(),
                        sep: Some(String::new()),
                        explicit: Some(short_explicit.into()),
                    });
                } else if f.starts_with(prefix) {
                    out.push(OptTuple {
                        action: Some(i),
                        option: f.into(),
                        sep: sep.clone(),
                        explicit: explicit.clone(),
                    });
                }
            }
        }
        out
    }

    /// `_get_nargs_pattern`.
    fn nargs_pattern(&self, i: Option<usize>) -> &'static str {
        match i.map(|i| &self.args[i]) {
            None => "(-*A[-AO]*)", // the subcommand (nargs=PARSER)
            Some(a) if !a.is_positional() => {
                if a.takes_value() {
                    "([A])"
                } else {
                    "([AO]{0})"
                }
            }
            Some(a) if a.optional => "(-*A?-*)",
            Some(_) => "(-*A-*)",
        }
    }

    fn match_argument(&self, i: usize, pattern: &str) -> Result<usize, ArgError> {
        let re = Regex::new(&format!("^{}", self.nargs_pattern(Some(i)))).expect("regex");
        match re.captures(pattern) {
            Some(c) => Ok(c.get(1).map_or(0, |m| m.len())),
            None => Err(ArgError(
                Some(self.args[i].name()),
                "expected one argument".into(),
            )),
        }
    }

    /// `_match_arguments_partial`; `None` stands for the subcommand.
    fn match_partial(&self, actions: &[Option<usize>], pattern: &str) -> Vec<usize> {
        for n in (1..=actions.len()).rev() {
            let joined: String = actions[..n]
                .iter()
                .map(|a| self.nargs_pattern(*a))
                .collect();
            let re = Regex::new(&format!("^{joined}")).expect("regex");
            if let Some(c) = re.captures(pattern) {
                let mut result: Vec<usize> = (1..c.len())
                    .map(|g| c.get(g).map_or(0, |m| m.len()))
                    .collect();
                let end = c.get(0).map_or(0, |m| m.end());
                if end < pattern.len() && pattern.as_bytes()[end] == b'O' {
                    while result.last() == Some(&0) {
                        result.pop();
                    }
                }
                return result;
            }
        }
        Vec::new()
    }

    fn convert(&self, i: usize, s: &str) -> Result<Val, ArgError> {
        let a = &self.args[i];
        let v = if a.int {
            match py_int(s) {
                Some(n) => Val::Int(n),
                None => {
                    return Err(ArgError(
                        Some(a.name()),
                        format!("invalid int value: {}", py_repr(s)),
                    ));
                }
            }
        } else {
            Val::Str(s.to_string())
        };
        if let Some(choices) = &a.choices {
            let text = match &v {
                Val::Int(n) => n.to_string(),
                Val::Str(s) => s.clone(),
                _ => String::new(),
            };
            if !choices.contains(&text.as_str()) {
                let list: Vec<String> = choices.iter().map(|c| py_repr(c)).collect();
                return Err(ArgError(
                    Some(a.name()),
                    format!(
                        "invalid choice: {} (choose from {})",
                        py_repr(&text),
                        list.join(", ")
                    ),
                ));
            }
        }
        Ok(v)
    }

    /// `parse_known_args`: the namespace and the arguments it did not know.
    pub fn parse_known(&self, args: &[String]) -> Result<(Namespace, Vec<String>), Exit> {
        let mut ns = Namespace::default();
        match self.parse_into(args, &mut ns) {
            Ok(extras) => Ok((ns, extras)),
            Err(Fail::Exit(e)) => Err(e),
            Err(Fail::Arg(e)) => Err(self.error(&e.text())),
        }
    }

    /// `ArgumentParser.error`: usage and the message on stderr, exit 2.
    pub fn error(&self, message: &str) -> Exit {
        Exit {
            code: 2,
            stdout: String::new(),
            stderr: format!("{}{}: error: {message}\n", self.format_usage(), self.prog),
        }
    }

    fn parse_into(&self, args: &[String], ns: &mut Namespace) -> Result<Vec<String>, Fail> {
        for a in &self.args {
            if a.kind != Kind::Help && !ns.has(a.dest) {
                ns.set(a.dest, a.default.clone());
            }
        }
        if let Some(sub) = &self.sub
            && !ns.has(sub.dest)
        {
            ns.set(sub.dest, Val::None);
        }
        // The A/O/- pattern and the options found.
        let mut pattern = String::new();
        let mut options: Vec<(usize, Vec<OptTuple>)> = Vec::new();
        let mut after_dashes = false;
        for (i, s) in args.iter().enumerate() {
            if after_dashes {
                pattern.push('A');
            } else if s == "--" {
                pattern.push('-');
                after_dashes = true;
            } else {
                match self.parse_optional(s) {
                    None => pattern.push('A'),
                    Some(t) => {
                        options.push((i, t));
                        pattern.push('O');
                    }
                }
            }
        }
        let option_at = |i: usize| options.iter().find(|(j, _)| *j == i).map(|(_, t)| t);
        // Positionals left to fill; None is the subcommand.
        let mut positionals: Vec<Option<usize>> = self
            .args
            .iter()
            .enumerate()
            .filter(|(_, a)| a.is_positional() && a.kind != Kind::Help)
            .map(|(i, _)| Some(i))
            .collect();
        if self.sub.is_some() {
            positionals.push(None);
        }
        let mut seen: Vec<Option<usize>> = Vec::new();
        let mut extras: Vec<String> = Vec::new();
        let mut sub_extras: Vec<String> = Vec::new();
        let max_option = options.iter().map(|(i, _)| *i).max();
        let mut start = 0usize;

        macro_rules! consume_positionals {
            ($from:expr) => {{
                let from: usize = $from;
                let counts = self.match_partial(&positionals, &pattern[from..]);
                let mut at = from;
                for (k, count) in counts.iter().enumerate() {
                    let action = positionals[k];
                    let mut taken: Vec<String> = args[at..at + count].to_vec();
                    if action.is_none() {
                        if pattern.as_bytes().get(at) == Some(&b'-') {
                            taken.remove(0);
                        }
                    } else if pattern[at..at + count].contains('-')
                        && let Some(p) = taken.iter().position(|t| t == "--")
                    {
                        taken.remove(p);
                    }
                    at += count;
                    seen.push(action);
                    match action {
                        None => {
                            sub_extras.extend(self.take_subcommand(&taken, ns)?);
                        }
                        Some(i) => self.take_positional(i, &taken, ns)?,
                    }
                }
                positionals.drain(..counts.len());
                at
            }};
        }

        if let Some(max_option) = max_option {
            while start <= max_option {
                let mut next = start;
                while next <= max_option && option_at(next).is_none() {
                    next += 1;
                }
                if start != next {
                    let end = consume_positionals!(start);
                    if end > start {
                        start = end;
                        continue;
                    }
                    start = end;
                }
                if option_at(start).is_none() {
                    extras.extend(args[start..next].iter().cloned());
                    start = next;
                }
                start = self.consume_optional(
                    start,
                    args,
                    &pattern,
                    option_at(start).expect("option").clone(),
                    ns,
                    &mut extras,
                    &mut seen,
                )?;
            }
        }
        let stop = consume_positionals!(start);
        extras.extend(args[stop..].iter().cloned());

        let mut required = Vec::new();
        for (i, a) in self.args.iter().enumerate() {
            if !seen.contains(&Some(i)) && a.is_positional() && !a.optional && a.kind != Kind::Help
            {
                required.push(a.name());
            }
        }
        if !required.is_empty() {
            return Err(Fail::Arg(ArgError(
                None,
                format!(
                    "the following arguments are required: {}",
                    required.join(", ")
                ),
            )));
        }
        extras.extend(sub_extras);
        Ok(extras)
    }

    fn take_positional(&self, i: usize, taken: &[String], ns: &mut Namespace) -> Result<(), Fail> {
        let a = &self.args[i];
        let value = match taken {
            [] => match &a.default {
                Val::Str(s) => self.convert(i, s).map_err(Fail::Arg)?,
                other => other.clone(),
            },
            [one] => self.convert(i, one).map_err(Fail::Arg)?,
            _ => unreachable!("positionals take at most one value here"),
        };
        ns.set(a.dest, value);
        Ok(())
    }

    fn take_subcommand(&self, taken: &[String], ns: &mut Namespace) -> Result<Vec<String>, Fail> {
        let sub = self.sub.as_ref().expect("subparsers");
        let name = &taken[0];
        let Some((_, _, parser)) = sub.parsers.iter().find(|(n, _, _)| n == name) else {
            let list: Vec<String> = sub.parsers.iter().map(|(n, _, _)| py_repr(n)).collect();
            return Err(Fail::Arg(ArgError(
                Some(sub.dest.to_string()),
                format!(
                    "invalid choice: {} (choose from {})",
                    py_repr(name),
                    list.join(", ")
                ),
            )));
        };
        ns.set(sub.dest, Val::Str(name.clone()));
        let mut inner = Namespace::default();
        let extras = match parser.parse_into(&taken[1..], &mut inner) {
            Ok(x) => x,
            Err(Fail::Exit(e)) => return Err(Fail::Exit(e)),
            Err(Fail::Arg(e)) => return Err(Fail::Exit(parser.error(&e.text()))),
        };
        for (k, v) in inner.0 {
            ns.set(&k, v);
        }
        Ok(extras)
    }

    #[allow(clippy::too_many_arguments)]
    fn consume_optional(
        &self,
        start: usize,
        args: &[String],
        pattern: &str,
        tuples: Vec<OptTuple>,
        ns: &mut Namespace,
        extras: &mut Vec<String>,
        seen: &mut Vec<Option<usize>>,
    ) -> Result<usize, Fail> {
        if tuples.len() > 1 {
            let names: Vec<String> = tuples.iter().map(|t| t.option.clone()).collect();
            return Err(Fail::Arg(ArgError(
                None,
                format!(
                    "ambiguous option: {} could match {}",
                    args[start],
                    names.join(", ")
                ),
            )));
        }
        let OptTuple {
            mut action,
            mut option,
            mut sep,
            mut explicit,
        } = tuples.into_iter().next().expect("one tuple");
        let mut todo: Vec<(usize, Vec<String>, String)> = Vec::new();
        let stop;
        loop {
            let Some(i) = action else {
                extras.push(args[start].clone());
                return Ok(start + 1);
            };
            if let Some(exp) = explicit.clone() {
                let count = self.match_argument(i, "A").map_err(Fail::Arg)?;
                let second = option.chars().nth(1);
                if count == 0 && second != Some('-') && !exp.is_empty() {
                    if sep.as_deref().is_some_and(|s| !s.is_empty()) || exp.starts_with('-') {
                        return Err(Fail::Arg(ArgError(
                            Some(self.args[i].name()),
                            format!("ignored explicit argument {}", py_repr(&exp)),
                        )));
                    }
                    todo.push((i, Vec::new(), option.clone()));
                    let mut chars = exp.chars();
                    let first = chars.next().expect("non-empty");
                    let rest: String = chars.collect();
                    option = format!("-{first}");
                    if let Some(j) = self.lookup(&option) {
                        action = Some(j);
                        if rest.is_empty() {
                            sep = None;
                            explicit = None;
                        } else if let Some(r) = rest.strip_prefix('=') {
                            sep = Some("=".into());
                            explicit = Some(r.into());
                        } else {
                            sep = Some(String::new());
                            explicit = Some(rest);
                        }
                    } else {
                        extras.push(format!("-{exp}"));
                        stop = start + 1;
                        break;
                    }
                } else if count == 1 {
                    todo.push((i, vec![exp], option.clone()));
                    stop = start + 1;
                    break;
                } else {
                    return Err(Fail::Arg(ArgError(
                        Some(self.args[i].name()),
                        format!("ignored explicit argument {}", py_repr(&exp)),
                    )));
                }
            } else {
                let from = start + 1;
                let count = self
                    .match_argument(i, &pattern[from..])
                    .map_err(Fail::Arg)?;
                todo.push((i, args[from..from + count].to_vec(), option.clone()));
                stop = from + count;
                break;
            }
        }
        for (i, values, _) in todo {
            seen.push(Some(i));
            let a = &self.args[i];
            match a.kind {
                Kind::Help => {
                    return Err(Fail::Exit(Exit {
                        code: 0,
                        stdout: self.format_help(),
                        stderr: String::new(),
                    }));
                }
                Kind::StoreTrue => ns.set(a.dest, Val::Bool(true)),
                Kind::Store => {
                    let v = self.convert(i, &values[0]).map_err(Fail::Arg)?;
                    ns.set(a.dest, v);
                }
                Kind::Append => {
                    let v = self.convert(i, &values[0]).map_err(Fail::Arg)?;
                    let mut list = ns.list(a.dest);
                    if let Val::Str(s) = v {
                        list.push(s);
                    }
                    ns.set(a.dest, Val::List(list));
                }
            }
        }
        Ok(stop)
    }

    // ---- help and usage (HelpFormatter) ----

    fn metavar(a: &Arg, optional_default: bool) -> String {
        if let Some(m) = a.metavar {
            m.to_string()
        } else if let Some(c) = &a.choices {
            format!("{{{}}}", c.join(","))
        } else if optional_default {
            a.dest.to_uppercase()
        } else {
            a.dest.to_string()
        }
    }

    fn sub_metavar(&self) -> String {
        let sub = self.sub.as_ref().expect("subparsers");
        let names: Vec<&str> = sub.parsers.iter().map(|(n, _, _)| *n).collect();
        format!("{{{}}}", names.join(","))
    }

    /// One usage part per action, optionals first.
    fn usage_parts(&self) -> (Vec<String>, Vec<String>) {
        let mut opts = Vec::new();
        let mut pos = Vec::new();
        for a in &self.args {
            if a.is_positional() {
                let m = Self::metavar(a, false);
                pos.push(if a.optional { format!("[{m}]") } else { m });
            } else if a.takes_value() {
                opts.push(format!("[{} {}]", a.flags[0], Self::metavar(a, true)));
            } else {
                opts.push(format!("[{}]", a.flags[0]));
            }
        }
        if self.sub.is_some() {
            pos.push(format!("{} ...", self.sub_metavar()));
        }
        (opts, pos)
    }

    /// `format_usage`: "usage: …\n".
    pub fn format_usage(&self) -> String {
        let mut s = self.usage_text(width());
        s.pop(); // `format_help` strips the blank line after the usage
        s
    }

    fn usage_text(&self, width: usize) -> String {
        let prefix = "usage: ";
        let prog = self.prog.clone();
        let (opt_parts, pos_parts) = self.usage_parts();
        let all: Vec<String> = opt_parts.iter().chain(&pos_parts).cloned().collect();
        let mut usage = std::iter::once(prog.clone())
            .chain(all.iter().cloned())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let text_width = width;
        if prefix.len() + usage.chars().count() > text_width {
            let get_lines = |parts: &[String], indent: &str, prefix: Option<&str>| {
                let mut lines: Vec<String> = Vec::new();
                let mut line: Vec<String> = Vec::new();
                let indent_len = indent.len();
                let mut line_len: isize = match prefix {
                    Some(p) => p.len() as isize - 1,
                    None => indent_len as isize - 1,
                };
                for part in parts {
                    let plen = part.chars().count() as isize;
                    if line_len + 1 + plen > text_width as isize && !line.is_empty() {
                        lines.push(format!("{indent}{}", line.join(" ")));
                        line.clear();
                        line_len = indent_len as isize - 1;
                    }
                    line.push(part.clone());
                    line_len += plen + 1;
                }
                if !line.is_empty() {
                    lines.push(format!("{indent}{}", line.join(" ")));
                }
                if prefix.is_some() && !lines.is_empty() {
                    lines[0] = lines[0][indent_len..].to_string();
                }
                lines
            };
            let lines: Vec<String> =
                if (prefix.len() + prog.chars().count()) as f64 <= 0.75 * text_width as f64 {
                    let indent = " ".repeat(prefix.len() + prog.chars().count() + 1);
                    let mut head = vec![prog.clone()];
                    if !opt_parts.is_empty() {
                        head.extend(opt_parts.iter().cloned());
                        let mut l = get_lines(&head, &indent, Some(prefix));
                        l.extend(get_lines(&pos_parts, &indent, None));
                        l
                    } else if !pos_parts.is_empty() {
                        head.extend(pos_parts.iter().cloned());
                        get_lines(&head, &indent, Some(prefix))
                    } else {
                        vec![prog.clone()]
                    }
                } else {
                    let indent = " ".repeat(prefix.len());
                    let parts: Vec<String> = all.clone();
                    let mut l = get_lines(&parts, &indent, None);
                    if l.len() > 1 {
                        l = get_lines(&opt_parts, &indent, None);
                        l.extend(get_lines(&pos_parts, &indent, None));
                    }
                    let mut v = vec![prog.clone()];
                    v.extend(l);
                    v
                };
            usage = lines.join("\n");
        }
        format!("{prefix}{usage}\n\n")
    }

    fn invocation(a: &Arg) -> String {
        if a.is_positional() {
            Self::metavar(a, false)
        } else if a.takes_value() {
            format!("{} {}", a.flags.join(", "), Self::metavar(a, true))
        } else {
            a.flags.join(", ")
        }
    }

    /// `format_help` for a parser without subcommands.
    pub fn format_help(&self) -> String {
        let width = width();
        let max_help = 24usize.min(width.saturating_sub(20).max(4));
        let indent = 2usize;
        let positionals: Vec<&Arg> = self.args.iter().filter(|a| a.is_positional()).collect();
        let optionals: Vec<&Arg> = self.args.iter().filter(|a| !a.is_positional()).collect();
        let max_len = self
            .args
            .iter()
            .map(|a| Self::invocation(a).chars().count() + indent)
            .max()
            .unwrap_or(0);
        let help_position = (max_len + 2).min(max_help);
        let help_width = width.saturating_sub(help_position).max(11);
        let action_width = help_position.saturating_sub(indent + 2);
        let format_action = |a: &Arg| {
            let header = Self::invocation(a);
            let mut out = String::new();
            let help = a.help.filter(|h| !h.is_empty());
            let indent_first;
            match help {
                None => {
                    return format!("{}{header}\n", " ".repeat(indent));
                }
                Some(_) if header.chars().count() <= action_width => {
                    out.push_str(&format!("{}{header:<action_width$}  ", " ".repeat(indent)));
                    indent_first = 0;
                }
                Some(_) => {
                    out.push_str(&format!("{}{header}\n", " ".repeat(indent)));
                    indent_first = help_position;
                }
            }
            let text = help.unwrap_or_default();
            if !text.trim().is_empty() {
                let lines = wrap(&collapse(text), help_width);
                for (k, line) in lines.iter().enumerate() {
                    let pad = if k == 0 { indent_first } else { help_position };
                    out.push_str(&format!("{}{line}\n", " ".repeat(pad)));
                }
            } else if !out.ends_with('\n') {
                out.push('\n');
            }
            out
        };
        let mut help = self.usage_text(width);
        for (title, group) in [
            ("positional arguments", positionals),
            ("options", optionals),
        ] {
            if group.is_empty() {
                continue;
            }
            help.push_str(&format!("\n{title}:\n"));
            for a in group {
                help.push_str(&format_action(a));
            }
            help.push('\n');
        }
        let re = Regex::new(r"\n\n\n+").expect("regex");
        let help = re.replace_all(&help, "\n\n").to_string();
        format!("{}\n", help.trim_matches('\n'))
    }
}

/// Whitespace runs to one space, trimmed (`_whitespace_matcher`).
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_letter(c: char) -> bool {
    is_word(c) && !c.is_numeric()
}

fn is_word_punct(c: char) -> bool {
    is_word(c) || "!\"'&.,?".contains(c)
}

/// `TextWrapper._split`: words, spaces, and hyphenated words split after the hyphen.
fn chunks(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().collect();
    let n = c.len();
    let at = |i: isize| -> Option<char> {
        if i < 0 {
            None
        } else {
            c.get(i as usize).copied()
        }
    };
    // `-{2,}\w` starting at i.
    let dashes_then_word = |i: usize| -> bool {
        let mut j = i;
        while j < n && c[j] == '-' {
            j += 1;
        }
        j - i >= 2 && j < n && is_word(c[j])
    };
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < n {
        if c[pos].is_whitespace() {
            let mut j = pos;
            while j < n && c[j].is_whitespace() {
                j += 1;
            }
            out.push(c[pos..j].iter().collect());
            pos = j;
            continue;
        }
        if pos > 0 && is_word_punct(c[pos - 1]) && dashes_then_word(pos) {
            let mut j = pos;
            while j < n && c[j] == '-' {
                j += 1;
            }
            out.push(c[pos..j].iter().collect());
            pos = j;
            continue;
        }
        let mut k = pos + 1;
        let end = loop {
            if k > n || c[k - 1].is_whitespace() {
                break n; // cannot happen: the run stops at whitespace
            }
            // hyphenated word: `-` at k, then lookbehind and lookahead
            if k < n && c[k] == '-' {
                let k = k as isize;
                let behind = (at(k - 2).is_some_and(is_letter) && at(k - 1).is_some_and(is_letter))
                    || (at(k - 3).is_some_and(is_letter)
                        && at(k - 2) == Some('-')
                        && at(k - 1).is_some_and(is_letter));
                let ahead = at(k + 1).is_some_and(is_letter)
                    && (at(k + 2).is_some_and(is_letter)
                        || (at(k + 2) == Some('-') && at(k + 3).is_some_and(is_letter)));
                if behind && ahead {
                    break k as usize + 1;
                }
            }
            if k == n || c[k].is_whitespace() {
                break k;
            }
            if is_word_punct(c[k - 1]) && dashes_then_word(k) {
                break k;
            }
            k += 1;
        };
        out.push(c[pos..end].iter().collect());
        pos = end;
    }
    out
}

/// `textwrap.wrap(text, width)` with the default options.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut chunks: Vec<String> = chunks(text);
    chunks.reverse();
    let mut lines: Vec<String> = Vec::new();
    let len = |s: &String| s.chars().count();
    while !chunks.is_empty() {
        let mut cur: Vec<String> = Vec::new();
        let mut cur_len = 0usize;
        if chunks.last().is_some_and(|c| c.trim().is_empty()) && !lines.is_empty() {
            chunks.pop();
        }
        while let Some(last) = chunks.last() {
            let l = len(last);
            if cur_len + l <= width {
                cur_len += l;
                cur.push(chunks.pop().expect("chunk"));
            } else {
                break;
            }
        }
        if chunks.last().is_some_and(|c| len(c) > width) {
            // `_handle_long_word`
            let space_left = if width < 1 { 1 } else { width - cur_len };
            let chunk: Vec<char> = chunks.last().expect("chunk").chars().collect();
            let mut end = space_left;
            if chunk.len() > space_left
                && let Some(h) = chunk[..space_left].iter().rposition(|c| *c == '-')
                && h > 0
                && chunk[..h].iter().any(|c| *c != '-')
            {
                end = h + 1;
            }
            cur.push(chunk[..end].iter().collect());
            *chunks.last_mut().expect("chunk") = chunk[end..].iter().collect();
        }
        if cur.last().is_some_and(|c| c.trim().is_empty()) {
            cur.pop();
        }
        if !cur.is_empty() {
            lines.push(cur.concat());
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_ints_saturate_so_execute_reports_the_range() {
        assert_eq!(py_int("99999999999999999999"), Some(i64::MAX));
        assert_eq!(py_int("-99999999999999999999"), Some(i64::MIN));
        assert_eq!(py_int("12"), Some(12));
    }

    #[test]
    fn wraps_like_textwrap() {
        assert_eq!(
            wrap(
                "send: print the exact body with the key hidden; send nothing",
                20
            ),
            vec![
                "send: print the",
                "exact body with the",
                "key hidden; send",
                "nothing"
            ]
        );
        assert_eq!(
            wrap("a single-user folder", 12),
            vec!["a single-", "user folder"]
        );
    }

    #[test]
    fn reads_ints_like_python() {
        assert_eq!(py_int(" 1_0 "), Some(10));
        assert_eq!(py_int("+5"), Some(5));
        assert_eq!(py_int("-5"), Some(-5));
        assert_eq!(py_int("1__0"), None);
        assert_eq!(py_int("x"), None);
    }

    #[test]
    fn repr_quotes_like_python() {
        assert_eq!(py_repr("x"), "'x'");
        assert_eq!(py_repr("it's"), "\"it's\"");
    }
}
