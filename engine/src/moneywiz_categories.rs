//! MoneyWiz categories as numbered migrations (decision 0012).
//!
//! `moneywiz categories` lists the live category tree with row counts.
//! `suggest` writes proposals into `inbox/moneywiz-migrations/NNNN-*.toml`,
//! a file the owner reviews. `plan` turns the next pending migration (or the
//! rollback of the last applied one) into a plan for the Core Data writer
//! (`moneywiz_edit::WriterPlan`). Each migration names the tree it was
//! written for (`expect_tree`); a migration for another tree is refused.
//! Before a migration is applied, its inverse is computed and checked, and
//! `data/moneywiz-migrations.json` keeps it for the rollback. That record
//! holds category names, fingerprints and row GIDs, never row content.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::moneywiz::{self, Store};
use crate::moneywiz_edit::{CONTRACT, CategoryOp, RowEdit, WriterPlan};

/// Migration files, in the inbox: `NNNN-name.toml`.
pub const DIR: &str = "moneywiz-migrations";
/// Applied migrations and their inverses, in `data/`.
pub const APPLIED_FILE: &str = "moneywiz-migrations.json";
/// The migration (or rollback) the writer is applying now, in `data/`.
pub const PENDING_FILE: &str = "moneywiz-migration-pending.json";

pub const EXPENSE: i64 = 1;
pub const INCOME: i64 = 2;
/// A top-level category with this many rows or fewer is rarely used.
const RARE: usize = 2;

fn kind_name(k: Option<i64>) -> &'static str {
    match k {
        Some(EXPENSE) => "expense",
        Some(INCOME) => "income",
        _ => "unknown",
    }
}

fn kind_raw(text: &str) -> Option<i64> {
    match text.trim().to_lowercase().as_str() {
        "expense" | "expenses" => Some(EXPENSE),
        "income" => Some(INCOME),
        _ => None,
    }
}

fn split_path(text: &str) -> Vec<String> {
    text.split('/')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn show(path: &[String]) -> String {
    path.join("/")
}

/// One step of a migration. Exactly one of `add`, `rename`, `move` or
/// `delete` is set.
#[derive(Deserialize, Serialize, Clone, Default, Debug)]
#[serde(deny_unknown_fields)]
pub struct Op {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub add: Option<String>,
    /// add, top level: "income" or "expense".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// add, top level: an existing top-level category to copy settings from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub like: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename: Option<String>,
    /// rename: the new name; move: the category the rows go to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    #[serde(rename = "move", skip_serializing_if = "Option::is_none")]
    pub mv: Option<String>,
    /// move: only "income" rows (amount above 0) or "expense" rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only: Option<String>,
    /// move: only these rows, by GID (rollbacks use it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete: Option<String>,
    /// delete: where its rows go.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_to: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub description: Option<String>,
    /// The tree this migration was written for (`fingerprint`).
    pub expect_tree: String,
    #[serde(default)]
    pub op: Vec<Op>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AppliedMigration {
    pub id: String,
    pub file: String,
    pub file_hash: String,
    pub before: String,
    pub after: String,
    /// The backup taken before the live write, relative to the repo.
    #[serde(default)]
    pub backup: Option<String>,
    pub inverse: Vec<Op>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Applied {
    pub migrations: Vec<AppliedMigration>,
}

#[derive(Serialize, Deserialize)]
pub struct Pending {
    /// "migrate" or "rollback".
    pub action: String,
    pub record: AppliedMigration,
}

// --- The tree ----------------------------------------------------------------

#[derive(Clone)]
struct Node {
    name: String,
    /// The name when the writer adds categories (before any rename).
    name0: String,
    parent: Option<usize>,
    kind: Option<i64>,
    alive: bool,
    /// MoneyWiz's links to this category, rows not read here included.
    links: usize,
}

#[derive(Clone)]
struct Row {
    gid: String,
    entity: String,
    cents: i64,
    date: String,
    node: usize,
    start: usize,
}

#[derive(Clone)]
struct Tree {
    nodes: Vec<Node>,
    rows: Vec<Row>,
    typed: bool,
    /// Paths that two or more MoneyWiz categories share. Cashcast cannot
    /// tell them apart, so it leaves them and everything under them alone.
    shared: Vec<Vec<String>>,
}

impl Tree {
    fn from(store: &Store) -> Tree {
        let mut cats = store.categories.clone();
        cats.sort_by_key(|p| p.len());
        let mut seen: HashSet<&Vec<String>> = HashSet::new();
        let mut shared: Vec<Vec<String>> = Vec::new();
        for p in &store.categories {
            if !seen.insert(p) && !shared.contains(p) {
                shared.push(p.clone());
            }
        }
        let mut by_path: HashMap<Vec<String>, usize> = HashMap::new();
        let mut nodes = Vec::new();
        for p in cats {
            if p.is_empty() || by_path.contains_key(&p) {
                continue;
            }
            let parent = if p.len() > 1 {
                match by_path.get(&p[..p.len() - 1]) {
                    Some(&i) => Some(i),
                    None => continue,
                }
            } else {
                None
            };
            let name = p.last().cloned().unwrap_or_default();
            nodes.push(Node {
                name0: name.clone(),
                name,
                parent,
                kind: store.category_types.get(&p).copied(),
                alive: true,
                links: store.category_links.get(&p).copied().unwrap_or(0),
            });
            by_path.insert(p, nodes.len() - 1);
        }
        let rows = store
            .txns
            .iter()
            .filter_map(|t| {
                let &node = by_path.get(&t.category)?;
                Some(Row {
                    gid: t.gid.clone(),
                    entity: t.entity.clone(),
                    cents: t.cents,
                    date: t.date.to_string(),
                    node,
                    start: node,
                })
            })
            .collect();
        let typed = nodes.iter().any(|n| n.kind.is_some());
        Tree {
            nodes,
            rows,
            typed,
            shared,
        }
    }

    /// The shared path that `path` is or lies under, if any.
    fn shared_at(&self, path: &[String]) -> Option<&Vec<String>> {
        self.shared.iter().find(|s| path.starts_with(s))
    }

    fn path_by(&self, i: usize, first: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = Some(i);
        while let Some(c) = cur {
            let n = &self.nodes[c];
            out.insert(
                0,
                if first {
                    n.name0.clone()
                } else {
                    n.name.clone()
                },
            );
            cur = n.parent;
        }
        out
    }

    fn path(&self, i: usize) -> Vec<String> {
        self.path_by(i, false)
    }

    fn find(&self, path: &[String]) -> Option<usize> {
        (0..self.nodes.len()).find(|&i| self.nodes[i].alive && self.path(i) == path)
    }

    /// The live category written as `text` ("Parent/Child"). A name may
    /// itself hold a "/", so whole paths are compared, not parts.
    fn find_text(&self, text: &str) -> Option<usize> {
        let want = show(&split_path(text));
        (0..self.nodes.len()).find(|&i| self.nodes[i].alive && show(&self.path(i)) == want)
    }

    /// A new category written as `text`: the longest existing prefix is its
    /// parent, and the rest its name.
    fn locate(&self, text: &str) -> (Option<usize>, Vec<String>) {
        let parts = split_path(text);
        for k in (1..parts.len()).rev() {
            if let Some(p) = self.find_text(&parts[..k].join("/")) {
                let mut path = self.path(p);
                path.push(parts[k..].join("/"));
                return (Some(p), path);
            }
        }
        (None, parts)
    }

    fn children(&self, i: usize) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.alive && n.parent == Some(i))
            .count()
    }

    fn rows_in(&self, i: usize) -> usize {
        self.rows.iter().filter(|r| r.node == i).count()
    }

    /// Links to the category from rows cashcast does not read. A rollback
    /// could not bring them back.
    fn other_links(&self, i: usize) -> usize {
        self.nodes[i]
            .links
            .saturating_sub(self.rows.iter().filter(|r| r.start == i).count())
    }

    fn fingerprint(&self) -> String {
        let mut lines: Vec<String> = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].alive)
            .map(|i| {
                let k = self.nodes[i].kind.map_or(String::new(), |k| k.to_string());
                format!("{}\t{k}", show(&self.path(i)))
            })
            .collect();
        lines.sort();
        lines.dedup();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in lines.join("\n").bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{h:016x}")
    }
}

/// The category tree's fingerprint: category paths and types, no rows. The
/// same tree always gives the same text.
pub fn fingerprint(store: &Store) -> String {
    Tree::from(store).fingerprint()
}

// --- Running a migration on the tree ----------------------------------------

enum Step {
    Add {
        node: usize,
        like: Option<usize>,
        path: Vec<String>,
    },
    Rename {
        path: Vec<String>,
        new: Vec<String>,
        old: String,
    },
    Move {
        from: Vec<String>,
        to: Vec<String>,
        gids: Vec<String>,
    },
    Delete {
        node: usize,
        move_to: Option<usize>,
        path: Vec<String>,
        to: Option<Vec<String>>,
        gids: Vec<String>,
        kind: Option<i64>,
        top: bool,
    },
}

/// Runs the ops in order on the tree. Errors name the op by number.
fn simulate(tree: &mut Tree, ops: &[Op]) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    let mut removed: HashSet<Vec<String>> = HashSet::new();
    let split_rule = "a name this migration removes earlier cannot come back in the same migration; split it into two migrations";
    for (n, op) in ops.iter().enumerate() {
        let n = n + 1;
        let err = |m: &str| format!("op #{n}: {m}");
        match (&op.add, &op.rename, &op.mv, &op.delete) {
            (Some(add), None, None, None) => {
                let (parent, path) = tree.locate(add);
                if path.is_empty() {
                    return Err(err("empty path"));
                }
                if tree.find_text(add).is_some() {
                    return Err(err("the category exists already"));
                }
                if removed.contains(&path) {
                    return Err(err(split_rule));
                }
                let want = match op.kind.as_deref() {
                    Some(k) => Some(
                        kind_raw(k).ok_or_else(|| err("kind must be \"income\" or \"expense\""))?,
                    ),
                    None => None,
                };
                let (parent, kind, like) = if path.len() > 1 {
                    let p = parent.ok_or_else(|| err("its parent category does not exist"))?;
                    let kind = tree.nodes[p].kind;
                    if want.is_some() && kind.is_some() && want != kind {
                        return Err(err("a subcategory has the type of its parent"));
                    }
                    (Some(p), kind, None)
                } else if let Some(like) = &op.like {
                    let l = tree
                        .find_text(like)
                        .filter(|&l| tree.nodes[l].parent.is_none())
                        .ok_or_else(|| err("`like` must name an existing top-level category"))?;
                    if want.is_some() && want != tree.nodes[l].kind {
                        return Err(err("`like` has another type than `kind`"));
                    }
                    (None, tree.nodes[l].kind, Some(l))
                } else if tree.typed {
                    let want = want.ok_or_else(|| {
                        err("a top-level category needs kind = \"income\" or \"expense\"")
                    })?;
                    // Any category the writer has when it adds: every one so far.
                    let l = (0..tree.nodes.len())
                        .find(|&i| {
                            tree.nodes[i].parent.is_none() && tree.nodes[i].kind == Some(want)
                        })
                        .ok_or_else(|| {
                            err(&format!(
                                "no top-level {} category to copy settings from",
                                kind_name(Some(want))
                            ))
                        })?;
                    (None, Some(want), Some(l))
                } else {
                    (None, None, None)
                };
                let name = path.last().cloned().unwrap_or_default();
                tree.nodes.push(Node {
                    name0: name.clone(),
                    name,
                    parent,
                    kind,
                    alive: true,
                    links: 0,
                });
                steps.push(Step::Add {
                    node: tree.nodes.len() - 1,
                    like,
                    path,
                });
            }
            (None, Some(old), None, None) => {
                let node = tree.find_text(old).ok_or_else(|| err("no such category"))?;
                let path = tree.path(node);
                let to = op.to.as_deref().unwrap_or("").trim().to_string();
                if to.is_empty() || to.contains('/') {
                    return Err(err("`to` must be a new name without /"));
                }
                let mut new = path.clone();
                new.pop();
                new.push(to.clone());
                if tree.find(&new).is_some() {
                    return Err(err("a category with the new name exists"));
                }
                if removed.contains(&new) {
                    return Err(err(split_rule));
                }
                for i in 0..tree.nodes.len() {
                    if tree.nodes[i].alive && tree.path(i).starts_with(&path) {
                        removed.insert(tree.path(i));
                    }
                }
                let old = std::mem::replace(&mut tree.nodes[node].name, to);
                steps.push(Step::Rename { path, new, old });
            }
            (None, None, Some(from), None) => {
                let a = tree
                    .find_text(from)
                    .ok_or_else(|| err("no such category to move rows from"))?;
                let b = tree
                    .find_text(op.to.as_deref().unwrap_or(""))
                    .ok_or_else(|| err("`to` must name an existing category"))?;
                let (from, to) = (tree.path(a), tree.path(b));
                if a == b {
                    return Err(err("`to` is the same category"));
                }
                let only = match op.only.as_deref() {
                    None => None,
                    Some(o) => Some(
                        kind_raw(o).ok_or_else(|| err("only must be \"income\" or \"expense\""))?,
                    ),
                };
                if let Some(gids) = &op.rows {
                    let here: HashSet<&str> = tree
                        .rows
                        .iter()
                        .filter(|r| r.node == a)
                        .map(|r| r.gid.as_str())
                        .collect();
                    if let Some(k) = gids.iter().position(|g| !here.contains(g.as_str())) {
                        return Err(err(&format!(
                            "row {} of the list is not in {} any more",
                            k + 1,
                            show(&from)
                        )));
                    }
                }
                let mut gids = Vec::new();
                for r in tree.rows.iter_mut().filter(|r| r.node == a) {
                    let pick = match only {
                        Some(INCOME) => r.cents > 0,
                        Some(_) => r.cents < 0,
                        None => true,
                    } && op.rows.as_ref().is_none_or(|l| l.contains(&r.gid));
                    if !pick {
                        continue;
                    }
                    if r.gid.is_empty() {
                        return Err(err("this MoneyWiz database has no GID for a row"));
                    }
                    r.node = b;
                    gids.push(r.gid.clone());
                }
                steps.push(Step::Move { from, to, gids });
            }
            (None, None, None, Some(del)) => {
                let node = tree.find_text(del).ok_or_else(|| err("no such category"))?;
                let path = tree.path(node);
                if tree.children(node) > 0 {
                    return Err(err("delete or move its subcategories first"));
                }
                let other = tree.other_links(node);
                if other > 0 {
                    return Err(err(&format!(
                        "{other} other MoneyWiz links use it (such as scheduled transactions), and a rollback could not bring them back; change it in MoneyWiz"
                    )));
                }
                let move_to = match &op.move_to {
                    None => None,
                    Some(m) => Some(
                        tree.find_text(m)
                            .filter(|&m| m != node)
                            .ok_or_else(|| err("move_to must be another existing category"))?,
                    ),
                };
                let in_use = tree.rows_in(node);
                if in_use > 0 && move_to.is_none() {
                    return Err(err(&format!("{in_use} rows use it; add move_to")));
                }
                let mut gids = Vec::new();
                for r in tree.rows.iter_mut().filter(|r| r.node == node) {
                    if r.gid.is_empty() {
                        return Err(err("this MoneyWiz database has no GID for a row"));
                    }
                    r.node = move_to.expect("checked above");
                    gids.push(r.gid.clone());
                }
                removed.insert(path.clone());
                tree.nodes[node].alive = false;
                steps.push(Step::Delete {
                    node,
                    move_to,
                    to: move_to.map(|m| tree.path(m)),
                    path,
                    gids,
                    kind: tree.nodes[node].kind,
                    top: tree.nodes[node].parent.is_none(),
                });
            }
            _ => return Err(err("use exactly one of add, rename, move or delete")),
        }
    }
    Ok(steps)
}

/// The ops that undo `steps`, in the order they run.
fn inverse(steps: &[Step]) -> Result<Vec<Op>, String> {
    let mut out = Vec::new();
    for s in steps.iter().rev() {
        match s {
            Step::Add { path, .. } => out.push(Op {
                delete: Some(show(path)),
                ..Default::default()
            }),
            Step::Rename { new, old, .. } => out.push(Op {
                rename: Some(show(new)),
                to: Some(old.clone()),
                ..Default::default()
            }),
            Step::Move { from, to, gids } => {
                if !gids.is_empty() {
                    out.push(Op {
                        mv: Some(show(to)),
                        to: Some(show(from)),
                        rows: Some(gids.clone()),
                        ..Default::default()
                    });
                }
            }
            Step::Delete {
                path,
                to,
                gids,
                kind,
                top,
                ..
            } => {
                let kind = if *top && kind.is_some() {
                    match kind {
                        Some(EXPENSE) | Some(INCOME) => Some(kind_name(*kind).to_string()),
                        _ => {
                            return Err(format!(
                                "cannot compute a rollback: {} has a type cashcast does not know",
                                show(path)
                            ));
                        }
                    }
                } else {
                    None
                };
                out.push(Op {
                    add: Some(show(path)),
                    kind,
                    ..Default::default()
                });
                if let Some(to) = to.as_ref().filter(|_| !gids.is_empty()) {
                    out.push(Op {
                        mv: Some(show(to)),
                        to: Some(show(path)),
                        rows: Some(gids.clone()),
                        ..Default::default()
                    });
                }
            }
        }
    }
    Ok(out)
}

pub struct Compiled {
    pub plan: WriterPlan,
    pub inverse: Vec<Op>,
    pub before: String,
    pub after: String,
    pub adds: usize,
    pub renames: usize,
    pub moves: usize,
    pub deletes: usize,
    /// Rows whose category changes.
    pub rows_moved: usize,
}

/// Checks a migration against the live tree and turns it into a writer plan
/// and its inverse. Refuses a tree that drifted, and a migration whose
/// rollback does not bring the tree back.
pub fn compile(store: &Store, m: &Migration) -> Result<Compiled, String> {
    let start = Tree::from(store);
    let before = start.fingerprint();
    if m.expect_tree.trim() != before {
        return Err(format!(
            "the MoneyWiz categories changed since this migration was written (tree {before}, the file expects {}). Write a new one: `bin/cashcast moneywiz categories suggest` or `new`",
            m.expect_tree.trim()
        ));
    }
    for (n, op) in m.op.iter().enumerate() {
        let to = op.mv.as_ref().and(op.to.as_ref());
        for text in [
            &op.add,
            &op.rename,
            &op.mv,
            &op.delete,
            &op.move_to,
            &op.like,
        ]
        .into_iter()
        .flatten()
        .chain(to)
        {
            if let Some(p) = start.shared_at(&split_path(text)) {
                return Err(format!(
                    "op #{}: two MoneyWiz categories are named \"{}\": rename one in MoneyWiz first, then write the migration again",
                    n + 1,
                    show(p)
                ));
            }
        }
    }
    let mut tree = start.clone();
    let steps = simulate(&mut tree, &m.op)?;
    let after = tree.fingerprint();

    // The writer runs adds, renames, row edits, then deletes. Adds use the
    // names before any rename, row edits and deletes the final names.
    let mut plan = WriterPlan {
        contract: CONTRACT,
        expect_tree: Some(after.clone()),
        ..Default::default()
    };
    let (mut adds, mut renames, mut moves, mut deletes) = (0, 0, 0, 0);
    for s in &steps {
        match s {
            Step::Add { node, like, .. } => {
                adds += 1;
                plan.categories.push(CategoryOp {
                    op: "add".into(),
                    path: tree.path_by(*node, true),
                    to: None,
                    move_to: None,
                    like: like.map(|l| tree.path_by(l, true)),
                });
            }
            Step::Rename { path, new, .. } => {
                renames += 1;
                plan.categories.push(CategoryOp {
                    op: "rename".into(),
                    path: path.clone(),
                    to: new.last().cloned(),
                    move_to: None,
                    like: None,
                });
            }
            Step::Move { .. } => moves += 1,
            Step::Delete { .. } => deletes += 1,
        }
    }
    let delete_steps: Vec<(usize, Option<usize>)> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Delete { node, move_to, .. } => Some((*node, *move_to)),
            _ => None,
        })
        .collect();
    let mut rows_moved = 0;
    for r in &tree.rows {
        if r.node != r.start {
            rows_moved += 1;
        }
        // Where the writer's deletes alone take the row.
        let mut at = Some(r.start);
        for &(node, move_to) in &delete_steps {
            if at == Some(node) {
                at = move_to;
            }
        }
        if at != Some(r.node) {
            plan.rows.push(RowEdit {
                entity: r.entity.clone(),
                gid: r.gid.clone(),
                expect_amount: r.cents as f64 / 100.0,
                expect_date: r.date.clone(),
                amount: None,
                category: Some(tree.path(r.node)),
                category_kind: None,
                payee: None,
                memo: None,
                date: None,
                delete: false,
                account: None,
                unlink: false,
            });
        }
    }
    for &(node, move_to) in &delete_steps {
        plan.categories.push(CategoryOp {
            op: "delete".into(),
            path: tree.path(node),
            to: None,
            move_to: move_to.map(|m| tree.path(m)),
            like: None,
        });
    }

    // The rollback must bring back the same tree, with every row where it was.
    let inverse = inverse(&steps)?;
    let mut back = tree.clone();
    simulate(&mut back, &inverse).map_err(|e| format!("cannot compute a rollback: {e}"))?;
    let rows_back = back
        .rows
        .iter()
        .all(|r| back.path(r.node) == start.path(r.start));
    if back.fingerprint() != before || !rows_back {
        return Err(
            "cannot compute a rollback that brings the tree back; split the migration".into(),
        );
    }
    Ok(Compiled {
        plan,
        inverse,
        before,
        after,
        adds,
        renames,
        moves,
        deletes,
        rows_moved,
    })
}

// --- Files -------------------------------------------------------------------

pub struct MigrationFile {
    pub id: String,
    pub file: String,
    pub path: PathBuf,
}

/// The migration files in `dir`, by number. Names not like `NNNN-name.toml`
/// are listed in the second value.
pub fn files(dir: &Path) -> Result<(Vec<MigrationFile>, Vec<String>), String> {
    let mut out = Vec::new();
    let mut odd = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok((out, odd));
    };
    for e in entries.flatten() {
        let file = e.file_name().to_string_lossy().to_string();
        if file.starts_with('.') {
            continue;
        }
        let ok = file.ends_with(".toml")
            && file.len() > 10
            && file[..4].chars().all(|c| c.is_ascii_digit())
            && file.as_bytes()[4] == b'-';
        if ok {
            out.push(MigrationFile {
                id: file[..4].to_string(),
                file: file.clone(),
                path: e.path(),
            });
        } else {
            odd.push(file);
        }
    }
    out.sort_by(|a, b| a.file.cmp(&b.file));
    if let Some(w) = out.windows(2).find(|w| w[0].id == w[1].id) {
        return Err(format!(
            "two migrations have the number {}; renumber one",
            w[0].id
        ));
    }
    odd.sort();
    Ok((out, odd))
}

pub fn load_migration(f: &MigrationFile) -> Result<(Migration, String), String> {
    let text =
        std::fs::read_to_string(&f.path).map_err(|e| format!("cannot read {}: {e}", f.file))?;
    // The parse error can quote a line of the file, so it is not shown.
    let m: Migration = toml::from_str(&text).map_err(|_| {
        format!("{} is not valid: it needs expect_tree, and each [[op]] one of add, rename (with to), move (with to) or delete", f.file)
    })?;
    Ok((m, hash(&text)))
}

fn hash(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub fn load_applied(data: &Path) -> Result<Applied, String> {
    match std::fs::read_to_string(data.join(APPLIED_FILE)) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{APPLIED_FILE} is damaged: {e}")),
        Err(_) => Ok(Applied::default()),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    crate::write_private(path, &json)
}

fn ensure_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn pending(home: &Path) -> Result<(Vec<MigrationFile>, Applied), String> {
    let (all, _) = files(&home.join("inbox").join(DIR))?;
    let applied = load_applied(&home.join("data"))?;
    let done: HashSet<&str> = applied.migrations.iter().map(|m| m.id.as_str()).collect();
    let todo = all
        .into_iter()
        .filter(|f| !done.contains(f.id.as_str()))
        .collect();
    Ok((todo, applied))
}

// --- Commands ----------------------------------------------------------------

/// `moneywiz categories`: the tree with counts, and the migrations.
pub fn render_list(store: &Store, home: &Path) -> Result<String, String> {
    let tree = Tree::from(store);
    let mut idx: Vec<usize> = (0..tree.nodes.len()).collect();
    idx.sort_by_key(|&i| tree.path(i));
    let count = |k: Option<i64>| tree.nodes.iter().filter(|n| n.kind == k).count();
    let unknown = tree.nodes.len() - count(Some(EXPENSE)) - count(Some(INCOME));
    let none = store.txns.iter().filter(|t| t.category.is_empty()).count();
    let mut out = String::from("# MoneyWiz categories\n\n");
    out.push_str(&format!(
        "{} categories ({} expense, {} income{}). {} rows have a category, {} have none.\n\n",
        tree.nodes.len(),
        count(Some(EXPENSE)),
        count(Some(INCOME)),
        if unknown > 0 {
            format!(", {unknown} of unknown type")
        } else {
            String::new()
        },
        tree.rows.len(),
        none
    ));
    out.push_str(
        "| Category | Type | Rows | Income rows | Subcategories |\n|---|---|---|---|---|\n",
    );
    for i in idx {
        let income = tree
            .rows
            .iter()
            .filter(|r| r.node == i && r.cents > 0)
            .count();
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            show(&tree.path(i)).replace('|', "/"),
            kind_name(tree.nodes[i].kind),
            tree.rows_in(i),
            income,
            tree.children(i)
        ));
    }
    let (todo, applied) = pending(home)?;
    out.push_str(&format!("\nTree: {}\n", tree.fingerprint()));
    out.push_str(&format!(
        "Migrations: {} applied{}, {} pending{}.\n",
        applied.migrations.len(),
        applied
            .migrations
            .last()
            .map_or(String::new(), |m| format!(" (last: {})", m.file)),
        todo.len(),
        if todo.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                todo.iter()
                    .map(|f| f.file.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    ));
    Ok(out)
}

/// `moneywiz categories plan [--rollback]`: the next migration (or the
/// rollback of the last one) as a writer plan, plus the pending record.
pub fn plan(store: &Store, home: &Path, rollback: bool) -> Result<String, String> {
    let data = home.join("data");
    ensure_dir(&data)?;
    let _ = std::fs::remove_file(data.join(PENDING_FILE));
    let (todo, mut applied) = pending(home)?;
    let (title, m, record, action) = if rollback {
        let Some(last) = applied.migrations.pop() else {
            return Ok("No applied migrations to roll back.\n".into());
        };
        let m = Migration {
            description: None,
            expect_tree: last.after.clone(),
            op: last.inverse.clone(),
        };
        let restore = last.backup.as_deref().map_or(String::new(), |b| {
            format!("\nLast resort: restore the backup {b} (connectors/playbooks/moneywiz.md, edit-in-place, step 8).")
        });
        let c = compile(store, &m)
            .map_err(|e| format!("cannot roll back {}: {e}{restore}", last.file))?;
        (format!("Rollback of {}", last.file), c, last, "rollback")
    } else {
        let Some(f) = todo.first() else {
            return Ok("No pending migrations.\n".into());
        };
        let (m, file_hash) = load_migration(f)?;
        let c = compile(store, &m).map_err(|e| format!("{}: {e}", f.file))?;
        let record = AppliedMigration {
            id: f.id.clone(),
            file: f.file.clone(),
            file_hash,
            before: c.before.clone(),
            after: c.after.clone(),
            backup: None,
            inverse: c.inverse.clone(),
        };
        (format!("Migration {}", f.file), c, record, "migrate")
    };
    write_json(&data.join(crate::moneywiz_edit::PLAN_FILE), &m.plan)?;
    write_json(
        &data.join(PENDING_FILE),
        &Pending {
            action: action.into(),
            record,
        },
    )?;
    let mut out = format!("# MoneyWiz categories: {title}\n\n");
    out.push_str(&format!("The tree matches (tree {}).\n", m.before));
    out.push_str(&format!(
        "{} add, {} rename, {} move, {} delete; {} rows change category ({} row edits for the writer).\n",
        m.adds, m.renames, m.moves, m.deletes, m.rows_moved, m.plan.rows.len()
    ));
    out.push_str(&format!(
        "Rollback computed and checked: {} steps.\n",
        m.inverse.len()
    ));
    out.push_str(&format!(
        "\nPlan saved for the writer: data/{}\n",
        crate::moneywiz_edit::PLAN_FILE
    ));
    if !rollback && todo.len() > 1 {
        out.push_str(&format!(
            "{} more pending after this one.\n",
            todo.len() - 1
        ));
    }
    Ok(out)
}

/// `moneywiz categories commit`: after a verified live write, records the
/// migration (or removes the rolled-back one).
pub fn commit(store: &Store, home: &Path, backup: Option<String>) -> Result<String, String> {
    let data = home.join("data");
    let path = data.join(PENDING_FILE);
    let text =
        std::fs::read_to_string(&path).map_err(|_| "no migration is waiting to be recorded")?;
    let mut p: Pending =
        serde_json::from_str(&text).map_err(|e| format!("{PENDING_FILE} is damaged: {e}"))?;
    let want = if p.action == "rollback" {
        &p.record.before
    } else {
        &p.record.after
    };
    if fingerprint(store) != *want {
        return Err(
            "the MoneyWiz categories are not the tree the write should leave; nothing recorded"
                .into(),
        );
    }
    let mut applied = load_applied(&data)?;
    let out = if p.action == "rollback" {
        applied.migrations.retain(|m| m.id != p.record.id);
        format!(
            "Rolled back {}. You can delete or edit the file now.\n",
            p.record.file
        )
    } else {
        p.record.backup = backup;
        let file = p.record.file.clone();
        applied.migrations.push(p.record);
        format!(
            "Recorded {file} as applied. `bin/cashcast moneywiz categories rollback --yes` undoes it.\n"
        )
    };
    write_json(&data.join(APPLIED_FILE), &applied)?;
    let _ = std::fs::remove_file(path);
    Ok(out)
}

fn q(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn render_op(op: &Op) -> String {
    let mut out = String::from("[[op]]\n");
    let fields: [(&str, Option<String>); 9] = [
        ("add", op.add.clone()),
        ("kind", op.kind.clone()),
        ("like", op.like.clone()),
        ("rename", op.rename.clone()),
        ("move", op.mv.clone()),
        ("to", op.to.clone()),
        ("only", op.only.clone()),
        ("delete", op.delete.clone()),
        ("move_to", op.move_to.clone()),
    ];
    for (k, v) in fields {
        if let Some(v) = v {
            out.push_str(&format!("{k} = {}\n", q(&v)));
        }
    }
    out
}

fn next_number(dir: &Path) -> Result<String, String> {
    let (all, _) = files(dir)?;
    let n = all
        .iter()
        .filter_map(|f| f.id.parse::<u32>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    Ok(format!("{n:04}"))
}

/// `moneywiz categories new [--name NAME]`: an empty migration for the
/// current tree, for the owner to fill.
pub fn new_file(store: &Store, home: &Path, name: Option<&str>) -> Result<String, String> {
    let dir = home.join("inbox").join(DIR);
    ensure_dir(&dir)?;
    let slug: String = name
        .unwrap_or("change")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let file = format!("{}-{}.toml", next_number(&dir)?, slug.trim_matches('-'));
    let text = format!(
        "# A MoneyWiz category migration. Add [[op]] blocks, then run\n\
         # `bin/cashcast moneywiz categories migrate --dry-run`.\n\
         #\n\
         # [[op]]\n# add = \"Food/Coffee\"            # top level: also kind = \"income\" or \"expense\"\n\
         # [[op]]\n# rename = \"Food/Groceries\"\n# to = \"Grocery stores\"\n\
         # [[op]]\n# move = \"Shopping\"             # moves its rows\n# to = \"Food/Coffee\"\n# only = \"income\"               # optional: only rows above 0 (or \"expense\")\n\
         # [[op]]\n# delete = \"Old\"\n# move_to = \"Food/Coffee\"         # where its rows go\n\n\
         description = \"\"\n\
         expect_tree = {}   # the category tree this file is for; do not edit\n",
        q(&fingerprint(store))
    );
    crate::write_private(&dir.join(&file), &text)?;
    Ok(format!(
        "Wrote inbox/{DIR}/{file} for the current tree. Open it in your editor and add the changes.\n"
    ))
}

// --- Suggestions -------------------------------------------------------------

/// A name with case, punctuation, "and" and plural endings removed.
fn norm(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && *w != "and")
        .map(|w| {
            if w.len() > 3 {
                w.trim_end_matches('s')
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct Suggestion {
    pub reason: String,
    pub ops: Vec<Op>,
}

/// Category paths cashcast uses: `[budget]` and `[strategy.categories]` in
/// profile.toml (written as "Parent > Child").
pub fn used_paths(profile: &Path) -> Vec<Vec<String>> {
    let Ok(text) = std::fs::read_to_string(profile) else {
        return Vec::new();
    };
    let Ok(value) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let tables = [
        value.get("budget").and_then(|v| v.as_table()),
        value
            .get("strategy")
            .and_then(|v| v.get("categories"))
            .and_then(|v| v.as_table()),
    ];
    for t in tables.into_iter().flatten() {
        for k in t.keys() {
            let p: Vec<String> = k
                .split('>')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !p.is_empty() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

pub fn suggest(store: &Store, used: &[Vec<String>]) -> Vec<Suggestion> {
    let tree = Tree::from(store);
    let alive: Vec<usize> = (0..tree.nodes.len())
        .filter(|&i| tree.nodes[i].alive && tree.shared_at(&tree.path(i)).is_none())
        .collect();
    let lower = |p: &[String]| p.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
    let used_lower: Vec<Vec<String>> = used.iter().map(|p| lower(p)).collect();
    let is_used = |i: usize| used_lower.contains(&lower(&tree.path(i)));
    let subtree_rows = |i: usize| -> usize {
        alive
            .iter()
            .filter(|&&j| tree.path(j).starts_with(&tree.path(i)))
            .map(|&j| tree.rows_in(j))
            .sum()
    };
    let same_kind = |a: usize, b: usize| {
        tree.nodes[a].kind.is_none()
            || tree.nodes[b].kind.is_none()
            || tree.nodes[a].kind == tree.nodes[b].kind
    };
    let mut out: Vec<Suggestion> = tree
        .shared
        .iter()
        .map(|p| Suggestion {
            reason: format!(
                "Two MoneyWiz categories are named \"{}\": rename one in MoneyWiz, then suggest again. Until then cashcast leaves it alone.",
                show(p)
            ),
            ops: Vec::new(),
        })
        .collect();
    let mut touched: HashSet<usize> = HashSet::new();

    // 1. Categories cashcast uses but MoneyWiz lacks.
    let mut added: HashSet<Vec<String>> = HashSet::new();
    for p in used {
        if alive.iter().any(|&i| lower(&tree.path(i)) == lower(p)) {
            continue;
        }
        let mut ops = Vec::new();
        for n in 1..=p.len() {
            let sub = p[..n].to_vec();
            if alive.iter().any(|&i| lower(&tree.path(i)) == lower(&sub))
                || !added.insert(sub.clone())
            {
                continue;
            }
            ops.push(Op {
                add: Some(show(&sub)),
                kind: (n == 1 && tree.typed).then(|| "expense".to_string()),
                ..Default::default()
            });
        }
        if !ops.is_empty() {
            out.push(Suggestion {
                reason: format!(
                    "Your budget or card strategy uses \"{}\", which MoneyWiz does not have.",
                    show(p)
                ),
                ops,
            });
        }
    }

    // 2. A category of one type that holds mostly rows of the other type.
    for &i in &alive {
        let Some(kind) = tree.nodes[i].kind.filter(|k| *k == EXPENSE || *k == INCOME) else {
            continue;
        };
        let income = tree
            .rows
            .iter()
            .filter(|r| r.node == i && r.cents > 0)
            .count();
        let expense = tree
            .rows
            .iter()
            .filter(|r| r.node == i && r.cents < 0)
            .count();
        let (wrong, right, other) = if kind == EXPENSE {
            (income, expense, INCOME)
        } else {
            (expense, income, EXPENSE)
        };
        if wrong < 3 || wrong <= right {
            continue;
        }
        let name = tree.nodes[i].name.clone();
        let target = alive.iter().copied().find(|&j| {
            j != i
                && tree.nodes[j].parent == tree.nodes[i].parent
                && tree.nodes[j].kind == Some(other)
                && norm(&tree.nodes[j].name) == norm(&name)
        });
        let mut ops = Vec::new();
        let to = match target {
            Some(j) => {
                touched.insert(j);
                tree.path(j)
            }
            None => {
                let p = vec![format!("{name} ({})", kind_name(Some(other)))];
                if tree.find(&p).is_some() {
                    continue;
                }
                ops.push(Op {
                    add: Some(show(&p)),
                    kind: Some(kind_name(Some(other)).into()),
                    ..Default::default()
                });
                p
            }
        };
        ops.push(Op {
            mv: Some(show(&tree.path(i))),
            to: Some(show(&to)),
            only: Some(kind_name(Some(other)).into()),
            ..Default::default()
        });
        touched.insert(i);
        out.push(Suggestion {
            reason: format!(
                "\"{}\" is an {} category, but {wrong} of its rows are {} (and {right} {}).",
                show(&tree.path(i)),
                kind_name(Some(kind)),
                kind_name(Some(other)),
                kind_name(Some(kind))
            ),
            ops,
        });
    }

    // 3. Near-duplicates: the same name, or a top-level category with the
    //    name of a subcategory. The smaller one goes into the bigger one.
    for &a in &alive {
        for &b in &alive {
            if a == b || touched.contains(&a) || touched.contains(&b) || !same_kind(a, b) {
                continue;
            }
            let (pa, pb) = (tree.path(a), tree.path(b));
            let leaf = |p: &[String]| norm(p.last().expect("not empty"));
            // A leaf name only counts when one subcategory has it.
            let dup = (pa.len() == pb.len() && pa.iter().zip(&pb).all(|(x, y)| norm(x) == norm(y)))
                || (pa.len() == 1
                    && pb.len() > 1
                    && norm(&pa[0]) == leaf(&pb)
                    && alive
                        .iter()
                        .filter(|&&j| tree.path(j).len() > 1 && leaf(&tree.path(j)) == leaf(&pb))
                        .count()
                        == 1);
            if !dup || norm(&tree.nodes[a].name).is_empty() {
                continue;
            }
            // Rows of the other type would land in the wrong kind of category.
            let wrong_sign = |r: &&Row| match tree.nodes[b].kind {
                Some(EXPENSE) => r.cents > 0,
                Some(INCOME) => r.cents < 0,
                _ => false,
            };
            if tree
                .rows
                .iter()
                .filter(|r| r.node == a)
                .any(|r| wrong_sign(&r))
            {
                continue;
            }
            // `a` goes away: a leaf, not used, and not the bigger one.
            let (ra, rb) = (subtree_rows(a), subtree_rows(b));
            let bigger_b = rb > ra || (rb == ra && (is_used(b) || pb.len() > pa.len() || pb < pa));
            if tree.children(a) > 0 || is_used(a) || !bigger_b {
                continue;
            }
            touched.insert(a);
            touched.insert(b);
            out.push(Suggestion {
                reason: format!(
                    "\"{}\" ({ra} {}) looks like a duplicate of \"{}\" ({rb} {}): merge it.",
                    show(&pa),
                    if ra == 1 { "row" } else { "rows" },
                    show(&pb),
                    if rb == 1 { "row" } else { "rows" }
                ),
                ops: vec![Op {
                    delete: Some(show(&pa)),
                    move_to: (ra > 0).then(|| show(&pb)),
                    ..Default::default()
                }],
            });
        }
    }

    // 4. Rarely used top-level categories whose rows look like another
    //    top-level category: move them under it.
    for &i in &alive {
        let rows: Vec<&Row> = tree.rows.iter().filter(|r| r.node == i).collect();
        if touched.contains(&i)
            || tree.nodes[i].parent.is_some()
            || tree.children(i) > 0
            || rows.is_empty()
            || rows.len() > RARE
            || is_used(i)
        {
            continue;
        }
        let guesses: HashSet<Option<&str>> = rows
            .iter()
            .map(|r| {
                store
                    .txns
                    .iter()
                    .find(|t| t.gid == r.gid && !r.gid.is_empty())
                    .and_then(moneywiz::guessed_category)
            })
            .collect();
        let [Some(guess)] = guesses.into_iter().collect::<Vec<_>>()[..] else {
            continue;
        };
        let Some(parent) = alive.iter().copied().find(|&j| {
            j != i
                && tree.nodes[j].parent.is_none()
                && norm(&tree.nodes[j].name) == norm(guess)
                && same_kind(i, j)
        }) else {
            continue;
        };
        let mut new = tree.path(parent);
        new.push(tree.nodes[i].name.clone());
        if tree.find(&new).is_some() {
            continue;
        }
        touched.insert(i);
        out.push(Suggestion {
            reason: format!(
                "\"{}\" has only {} {}, and they look like {guess}: move it under \"{}\".",
                show(&tree.path(i)),
                rows.len(),
                if rows.len() == 1 { "row" } else { "rows" },
                show(&tree.path(parent))
            ),
            ops: vec![
                Op {
                    add: Some(show(&new)),
                    ..Default::default()
                },
                Op {
                    delete: Some(show(&tree.path(i))),
                    move_to: Some(show(&new)),
                    ..Default::default()
                },
            ],
        });
    }

    // 5. Empty categories nothing uses.
    for &i in &alive {
        if touched.contains(&i) || tree.children(i) > 0 || tree.rows_in(i) > 0 || is_used(i) {
            continue;
        }
        touched.insert(i);
        let other = tree.other_links(i);
        if other > 0 {
            out.push(Suggestion {
                reason: format!(
                    "\"{}\" has no rows here, but {other} other MoneyWiz links use it (such as scheduled transactions). Delete it in MoneyWiz if you no longer need it.",
                    show(&tree.path(i))
                ),
                ops: Vec::new(),
            });
            continue;
        }
        out.push(Suggestion {
            reason: format!("\"{}\" has no rows.", show(&tree.path(i))),
            ops: vec![Op {
                delete: Some(show(&tree.path(i))),
                ..Default::default()
            }],
        });
    }

    // Keep only suggestions that still run after the ones before them.
    let mut kept = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    for s in out {
        let mut trial = ops.clone();
        trial.extend(s.ops.iter().cloned());
        if simulate(&mut tree.clone(), &trial).is_ok() {
            ops = trial;
            kept.push(s);
        }
    }
    kept
}

/// `moneywiz categories suggest`: writes the proposals as a migration file.
pub fn write_suggestions(store: &Store, home: &Path, today: &str) -> Result<String, String> {
    let dir = home.join("inbox").join(DIR);
    let (todo, _) = pending(home)?;
    let mut number = None;
    for f in &todo {
        if f.file.ends_with("-suggested.toml") {
            number = Some(f.id.clone());
        } else {
            return Err(format!(
                "{} is not applied yet. Apply it (`bin/cashcast moneywiz categories migrate --yes`) or delete it, then suggest again",
                f.file
            ));
        }
    }
    let list = suggest(store, &used_paths(&home.join("profile.toml")));
    let mut out = String::from("# MoneyWiz category suggestions\n\n");
    let changes = list.iter().filter(|s| !s.ops.is_empty()).count();
    if changes == 0 {
        for s in &list {
            out.push_str(&format!("- {}\n", s.reason));
        }
        out.push_str("No changes to suggest. The tree looks tidy.\n");
        return Ok(out);
    }
    ensure_dir(&dir)?;
    for f in todo.iter() {
        let _ = std::fs::remove_file(&f.path);
    }
    let number = match number {
        Some(n) => n,
        None => next_number(&dir)?,
    };
    let file = format!("{number}-suggested.toml");
    let mut text = format!(
        "# MoneyWiz category changes suggested by `bin/cashcast moneywiz categories suggest` on {today}.\n\
         # Nothing changes until you run `bin/cashcast moneywiz categories migrate --yes`.\n\
         # Review each [[op]]: delete the ones you do not want, or edit names.\n\
         # Preview: `bin/cashcast moneywiz categories migrate --dry-run`.\n\n\
         description = \"Suggested changes\"\n\
         expect_tree = {}   # the category tree this file is for; do not edit\n",
        q(&fingerprint(store))
    );
    for (n, s) in list.iter().enumerate() {
        text.push_str(&format!("\n# {}. {}\n", n + 1, s.reason));
        for op in &s.ops {
            text.push_str(&render_op(op));
        }
        out.push_str(&format!("{}. {}\n", n + 1, s.reason));
    }
    crate::write_private(&dir.join(&file), &text)?;
    out.push_str(&format!(
        "\nWrote inbox/{DIR}/{file} ({changes} changes). Review it, then preview with `bin/cashcast moneywiz categories migrate --dry-run`.\n"
    ));
    Ok(out)
}

// --- The web app's match table ------------------------------------------------

/// The category paths an op names (its source and its target).
fn op_paths(op: &Op) -> Vec<String> {
    [&op.add, &op.rename, &op.to, &op.mv, &op.delete, &op.move_to]
        .into_iter()
        .flatten()
        .map(|p| show(&split_path(p)))
        .collect()
}

/// `moneywiz categories json`: each MoneyWiz category beside the cashcast
/// category it matches, the cashcast categories MoneyWiz lacks, the
/// suggestions, and the migrations. Names and counts only, no rows.
pub fn view_json(store: &Store, home: &Path) -> Result<String, String> {
    let tree = Tree::from(store);
    let used = used_paths(&home.join("profile.toml"));
    let lower = |p: &[String]| p.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
    let suggestions = suggest(store, &used);
    let mut idx: Vec<usize> = (0..tree.nodes.len())
        .filter(|&i| tree.nodes[i].alive)
        .collect();
    idx.sort_by_key(|&i| tree.path(i));
    let categories: Vec<serde_json::Value> = idx
        .iter()
        .map(|&i| {
            let path = tree.path(i);
            let cashcast = used
                .iter()
                .find(|u| lower(u) == lower(&path))
                .map(|u| u.join(" > "));
            let shown = show(&path);
            let suggested: Vec<usize> = suggestions
                .iter()
                .enumerate()
                .filter(|(_, s)| s.ops.iter().any(|op| op_paths(op).contains(&shown)))
                .map(|(n, _)| n)
                .collect();
            serde_json::json!({
                "path": shown,
                "kind": kind_name(tree.nodes[i].kind),
                "rows": tree.rows_in(i),
                "incomeRows": tree.rows.iter().filter(|r| r.node == i && r.cents > 0).count(),
                "subcategories": tree.children(i),
                "cashcast": cashcast,
                "shared": tree.shared_at(&path).is_some(),
                "suggested": suggested,
            })
        })
        .collect();
    let missing: Vec<String> = used
        .iter()
        .filter(|u| !idx.iter().any(|&i| lower(&tree.path(i)) == lower(u)))
        .map(|u| show(u))
        .collect();
    let (todo, applied) = pending(home)?;
    let last = applied.migrations.last();
    let out = serde_json::json!({
        "tree": tree.fingerprint(),
        "categories": categories,
        "missing": missing,
        "suggestions": suggestions
            .iter()
            .map(|s| serde_json::json!({"reason": s.reason, "ops": s.ops}))
            .collect::<Vec<_>>(),
        "applied": applied.migrations.len(),
        "lastApplied": last.map(|m| serde_json::json!({"file": m.file, "backup": m.backup})),
        "pending": todo.iter().map(|f| f.file.clone()).collect::<Vec<_>>(),
    });
    serde_json::to_string(&out).map_err(|e| e.to_string())
}

/// `moneywiz categories new --ops FILE`: a migration file from ops picked in
/// the web app (a JSON list of `[[op]]` tables), checked on the live tree
/// first. It replaces an unapplied file the web app or `suggest` wrote, and
/// refuses while the owner's own file waits.
pub fn write_ops(
    store: &Store,
    home: &Path,
    name: Option<&str>,
    ops: &[Op],
) -> Result<String, String> {
    if ops.is_empty() {
        return Err("Pick at least one change".into());
    }
    simulate(&mut Tree::from(store), ops)?;
    let dir = home.join("inbox").join(DIR);
    let (todo, _) = pending(home)?;
    if let Some(f) = todo
        .iter()
        .find(|f| !f.file.ends_with("-suggested.toml") && !f.file.ends_with("-web.toml"))
    {
        return Err(format!(
            "{} is not applied yet. Apply it (`bin/cashcast moneywiz categories migrate --yes`) or delete it, then pick changes again",
            f.file
        ));
    }
    ensure_dir(&dir)?;
    let number = match todo.first() {
        Some(f) => f.id.clone(),
        None => next_number(&dir)?,
    };
    for f in &todo {
        let _ = std::fs::remove_file(&f.path);
    }
    let file = format!("{number}-web.toml");
    let mut text = format!(
        "# MoneyWiz category changes picked in the cashcast web app.\n\
         # Nothing changes until you apply them (the web app, or `bin/cashcast moneywiz categories migrate --yes`).\n\n\
         description = {}\n\
         expect_tree = {}   # the category tree this file is for; do not edit\n",
        q(name.unwrap_or("Picked in the web app")),
        q(&fingerprint(store))
    );
    for op in ops {
        text.push('\n');
        text.push_str(&render_op(op));
    }
    crate::write_private(&dir.join(&file), &text)?;
    Ok(format!(
        "Wrote inbox/{DIR}/{file} ({} changes). Preview with `bin/cashcast moneywiz categories migrate --dry-run`.\n",
        ops.len()
    ))
}
