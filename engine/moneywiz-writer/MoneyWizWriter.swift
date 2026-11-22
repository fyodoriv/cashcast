// moneywiz-writer: applies a cashcast edit plan to a MoneyWiz database through
// Core Data, the way MoneyWiz saves its own changes (decision 0007).
//
// Raw SQL writes broke MoneyWiz's Core Data and iCloud sync for another tool,
// so this writer loads MoneyWiz's own managed-object model from the app,
// opens the store with persistent history, and saves once, after every edit
// in the plan has been checked. Any failure rolls back and changes nothing.
// The open, the checksum check, the transaction author and the value
// transformers follow marcomc/moneywiz-tools (MIT), whose writer is verified
// on MoneyWiz 2026.32.1.
//
// Build (macOS): swiftc -O -o build/moneywiz-writer MoneyWizWriter.swift
//
//   moneywiz-writer checksum --store DB --model MOMD
//   moneywiz-writer schema --model MOMD
//   moneywiz-writer apply --store DB --model MOMD --plan PLAN.json --checksum C [--allow-running]
//
// It prints counts, entity and attribute names only, never ledger values.

import AppKit
import CoreData
import Foundation

let transactionAuthor = "MWLocalAuthor"
let checksumKey = "NSStoreModelVersionChecksumKey"
let plainEntities: Set<String> = ["WithdrawTransaction", "DepositTransaction", "RefundTransaction"]

struct Fail: Error, CustomStringConvertible {
    let description: String
    init(_ message: String) { description = message }
}

struct Plan: Decodable {
    let contract: Int
    let rows: [RowEdit]
    let accounts: [AccountEdit]
    let categories: [CategoryOp]
    let trades: [TradeAdd]?
    let holdings: [HoldingEdit]?
    let archive: [ArchiveEdit]?
    let clear: [ClearEdit]?
}

/// A pending row to mark cleared (status 2).
struct ClearEdit: Decodable {
    let entity: String
    let gid: String
}

/// Archive an account, or show it again: MoneyWiz drops links sent to an
/// archived account.
struct ArchiveEdit: Decodable {
    let gid: String
    let expect: Bool
    let archived: Bool
}

/// A holding's opening shares, checked before the change.
struct HoldingEdit: Decodable {
    let gid: String
    let expect_opening: Double
    let opening: Double
    let price: Double?
    let expect_price: Double?
}

struct TradeAdd: Decodable {
    let account_gid: String
    let sell: Bool
    let date: String
    let symbol: String
    let shares: Double
    let price: Double
    let amount: Double
}

struct RowEdit: Decodable {
    let entity: String
    let gid: String
    let expect_amount: Double
    let expect_date: String
    let amount: Double?
    let category: [String]?
    /// 1 expense, 2 income: which of two categories that share a path.
    let category_kind: Int?
    let payee: String?
    let memo: String?
    /// A new local day (YYYY-MM-DD); the time of day is kept.
    let date: String?
    /// Remove the row and its category assignments.
    let delete: Bool?
    /// Move the row to this account (its GID), in the same currency.
    let account: String?
    /// With delete, on a coin transfer: the other side's account keeps its
    /// coins moving as a plain buy or sell with no cash, and both sides go.
    let unlink: Bool?
}

struct AccountEdit: Decodable {
    let gid: String
    let expect_opening: Double
    let opening: Double
}

struct CategoryOp: Decodable {
    let op: String
    let path: [String]
    let to: String?
    let move_to: [String]?
    let like: [String]?
    let kind: Int?
}

final class PassthroughTransformer: ValueTransformer {
    override class func allowsReverseTransformation() -> Bool { true }
    override class func transformedValueClass() -> AnyClass { NSObject.self }
    override func transformedValue(_ value: Any?) -> Any? { value }
    override func reverseTransformedValue(_ value: Any?) -> Any? { value }
}

func arguments() -> (String, [String: String], Set<String>) {
    let args = Array(CommandLine.arguments.dropFirst())
    var values: [String: String] = [:]
    var flags: Set<String> = []
    var i = 1
    while i < args.count {
        if i + 1 < args.count, !args[i + 1].hasPrefix("--") {
            values[args[i]] = args[i + 1]
            i += 2
        } else {
            flags.insert(args[i])
            i += 1
        }
    }
    return (args.first ?? "", values, flags)
}

func loadModel(_ path: String?) throws -> NSManagedObjectModel {
    guard let path, let model = NSManagedObjectModel(contentsOf: URL(fileURLWithPath: path)) else {
        throw Fail("cannot load the MoneyWiz model (--model MoneyWizDataModel.momd)")
    }
    return model
}

func storeChecksum(_ path: String) throws -> String {
    let meta = try NSPersistentStoreCoordinator.metadataForPersistentStore(
        ofType: NSSQLiteStoreType, at: URL(fileURLWithPath: path), options: nil)
    return meta[checksumKey] as? String ?? ""
}

func moneyWizRunning() -> Bool {
    NSWorkspace.shared.runningApplications.contains {
        ($0.bundleIdentifier ?? "").hasPrefix("com.moneywiz") || ($0.localizedName ?? "").hasPrefix("MoneyWiz")
    }
}

func open(store: String, model: NSManagedObjectModel) throws -> NSPersistentContainer {
    let url = URL(fileURLWithPath: store)
    let meta = try NSPersistentStoreCoordinator.metadataForPersistentStore(ofType: NSSQLiteStoreType, at: url, options: nil)
    guard model.isConfiguration(withName: nil, compatibleWithStoreMetadata: meta) else {
        throw Fail("the MoneyWiz model does not match this database")
    }
    for name in ["HMRCMappingTransformer", "TransactionsFilterArrayTransformer", "UIColorTransformer"] {
        ValueTransformer.setValueTransformer(PassthroughTransformer(), forName: NSValueTransformerName(name))
    }
    let container = NSPersistentContainer(name: "MoneyWizDataModel", managedObjectModel: model)
    let description = NSPersistentStoreDescription(url: url)
    description.setOption(true as NSNumber, forKey: NSPersistentHistoryTrackingKey)
    description.setOption(true as NSNumber, forKey: NSPersistentStoreRemoteChangeNotificationPostOptionKey)
    description.shouldMigrateStoreAutomatically = false
    description.shouldInferMappingModelAutomatically = false
    container.persistentStoreDescriptions = [description]
    var failure: Error?
    container.loadPersistentStores { _, error in failure = error }
    if let failure { throw Fail("cannot open the MoneyWiz database: \(failure.localizedDescription)") }
    return container
}

// MARK: - Object helpers

func need(_ object: NSManagedObject, _ key: String) throws {
    let e = object.entity
    guard e.attributesByName[key] != nil || e.relationshipsByName[key] != nil else {
        throw Fail("MoneyWiz's \(e.name ?? "?") has no '\(key)'. Run `moneywiz-writer schema` and report it.")
    }
}

func get(_ object: NSManagedObject, _ key: String) throws -> Any? {
    try need(object, key)
    return object.value(forKey: key)
}

func set(_ object: NSManagedObject, _ key: String, _ value: Any?) throws {
    try need(object, key)
    object.setValue(value, forKey: key)
}

func has(_ object: NSManagedObject, _ key: String) -> Bool {
    object.entity.attributesByName[key] != nil || object.entity.relationshipsByName[key] != nil
}

func newGID() -> String {
    UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
}

func fetch(_ entity: String, _ ctx: NSManagedObjectContext, sub: Bool = false, _ predicate: NSPredicate? = nil) throws -> [NSManagedObject] {
    let request = NSFetchRequest<NSManagedObject>(entityName: entity)
    request.includesSubentities = sub
    request.predicate = predicate
    return try ctx.fetch(request)
}

func one(_ entity: String, gid: String, _ ctx: NSManagedObjectContext, sub: Bool = false) throws -> NSManagedObject {
    let found = try fetch(entity, ctx, sub: sub, NSPredicate(format: "GID == %@", gid))
    guard found.count == 1 else { throw Fail("expected one \(entity) with that GID, found \(found.count)") }
    return found[0]
}

func double(_ value: Any?) -> Double? { (value as? NSNumber)?.doubleValue }

func close(_ a: Double?, _ b: Double) -> Bool { a.map { abs($0 - b) < 0.005 } ?? false }

func same(_ a: NSManagedObject?, _ b: NSManagedObject?) -> Bool { a?.objectID == b?.objectID }

func localDay(_ date: Date) -> String {
    let f = DateFormatter()
    f.locale = Locale(identifier: "en_US_POSIX")
    f.timeZone = TimeZone.current  // the engine reads dates on this computer's clock too
    f.dateFormat = "yyyy-MM-dd"
    return f.string(from: date)
}

/// Fails unless the entity has these keys. An unknown key in a predicate or
/// in value(forKey:) raises an Objective-C exception Swift cannot catch.
func requireKeys(_ entity: String, _ keys: [String], _ ctx: NSManagedObjectContext) throws {
    guard let e = ctx.persistentStoreCoordinator?.managedObjectModel.entitiesByName[entity] else {
        throw Fail("the MoneyWiz model has no \(entity)")
    }
    for key in keys where e.attributesByName[key] == nil && e.relationshipsByName[key] == nil {
        throw Fail("MoneyWiz's \(entity) has no '\(key)'. Run `moneywiz-writer schema` and report it.")
    }
}

func assignmentEntity(_ ctx: NSManagedObjectContext) throws -> String {
    let names = ctx.persistentStoreCoordinator?.managedObjectModel.entities.compactMap(\.name) ?? []
    guard let name = names.first(where: { $0.hasPrefix("CategoryAssig") }) else {
        throw Fail("the MoneyWiz model has no category assignment entity")
    }
    try requireKeys(name, ["transaction", "category", "amount"], ctx)
    return name
}

// MARK: - Categories

final class Categories {
    let ctx: NSManagedObjectContext
    let all: [NSManagedObject]
    init(_ ctx: NSManagedObjectContext) throws {
        self.ctx = ctx
        try requireKeys("Category", ["GID", "name", "parentCategory", "user"], ctx)
        all = try fetch("Category", ctx)
    }

    func path(_ c: NSManagedObject) -> [String] {
        var out: [String] = []
        var cur: NSManagedObject? = c
        while let node = cur, out.count < 9 {
            out.insert((node.value(forKey: "name") as? String) ?? "", at: 0)
            cur = node.value(forKey: "parentCategory") as? NSManagedObject
        }
        return out
    }

    func user() throws -> NSManagedObject {
        var users: [NSManagedObjectID: NSManagedObject] = [:]
        for c in all { if let u = try get(c, "user") as? NSManagedObject { users[u.objectID] = u } }
        guard users.count == 1, let u = users.values.first else {
            throw Fail("category changes need exactly one MoneyWiz user; found \(users.count)")
        }
        return u
    }

    func live() -> [NSManagedObject] { all.filter { !$0.isDeleted } + inserted }
    var inserted: [NSManagedObject] = []

    func find(_ path: [String], user: NSManagedObject?, kind: Int? = nil) throws -> NSManagedObject? {
        let found = live().filter {
            self.path($0) == path && (user == nil || same($0.value(forKey: "user") as? NSManagedObject, user))
                && (kind == nil || ($0.value(forKey: "type") as? NSNumber)?.intValue == kind)
        }
        guard found.count <= 1 else {
            let kinds = found.map { "\(($0.value(forKey: "type") as? NSNumber)?.intValue ?? 0)" }.joined(separator: ",")
            throw Fail("\(found.count) categories have the same path \(path.joined(separator: " > ")) (types \(kinds))")
        }
        return found.first
    }

    func require(_ path: [String], user: NSManagedObject?, kind: Int? = nil) throws -> NSManagedObject {
        guard let c = try find(path, user: user, kind: kind) else { throw Fail("a category in the plan does not exist") }
        return c
    }

    func add(_ path: [String], like: [String]?, kind: Int? = nil) throws {
        let user = try user()
        if try find(path, user: user, kind: kind) != nil { return }
        // kind picks between two parents of one name (an expense and an income one).
        let parent = path.count > 1 ? try require(Array(path.dropLast()), user: user, kind: kind) : nil
        // A new category copies the required settings (such as income or
        // expense) of its parent, of the `like` category, or of another
        // top-level category.
        let similar = try like.map { try require($0, user: user) }
        guard let template = parent ?? similar ?? live().first(where: { $0.value(forKey: "parentCategory") == nil }) else {
            throw Fail("no category to copy settings from")
        }
        let c = NSEntityDescription.insertNewObject(forEntityName: "Category", into: ctx)
        for (key, attribute) in c.entity.attributesByName
        where !attribute.isTransient && (!attribute.isOptional || key == "type")
            && !["GID", "name", "objectCreationDate"].contains(key) {
            c.setValue(template.value(forKey: key), forKey: key)
        }
        try set(c, "GID", newGID())
        try set(c, "name", path.last)
        if has(c, "objectCreationDate") { c.setValue(Date(), forKey: "objectCreationDate") }
        try set(c, "user", user)
        try set(c, "parentCategory", parent)
        inserted.append(c)
    }

    func rename(_ path: [String], to name: String) throws {
        try set(try require(path, user: try user()), "name", name)
    }

    func delete(_ path: [String], moveTo: [String]?) throws {
        let user = try user()
        let c = try require(path, user: user)
        if live().contains(where: { same($0.value(forKey: "parentCategory") as? NSManagedObject, c) }) {
            throw Fail("a category to delete still has subcategories")
        }
        let assignments = try fetch(try assignmentEntity(ctx), ctx, NSPredicate(format: "category == %@", c))
        if !assignments.isEmpty {
            guard let moveTo else { throw Fail("rows still use a category to delete") }
            let target = try require(moveTo, user: user)
            for a in assignments { try set(a, "category", target) }
        }
        ctx.delete(c)
    }
}

// MARK: - Rows

func assignments(of t: NSManagedObject, _ ctx: NSManagedObjectContext) throws -> [NSManagedObject] {
    try fetch(try assignmentEntity(ctx), ctx, NSPredicate(format: "transaction == %@", t))
}

func edit(_ e: RowEdit, _ ctx: NSManagedObjectContext, _ cats: Categories) throws {
    try requireKeys(e.entity, ["GID", "amount", "date", "account"], ctx)
    let t = try one(e.entity, gid: e.gid, ctx)
    let old = double(try get(t, "amount"))
    guard close(old, e.expect_amount), let date = try get(t, "date") as? Date, localDay(date) == e.expect_date else {
        throw Fail("a row changed since the plan was made; run the edit again")
    }
    if e.delete == true {
        if e.unlink == true { try unlinkOther(t, e.entity, ctx) }
        for s in try assignments(of: t, ctx) { ctx.delete(s) }
        ctx.delete(t)
        return
    }
    let account = try get(t, "account") as? NSManagedObject
    let user = try account.flatMap { try get($0, "user") as? NSManagedObject }
    if let gid = e.account { try move(t, e.entity, from: account, to: gid, ctx) }
    if let amount = e.amount {
        let transfers: Set<String> = ["TransferDepositTransaction", "TransferWithdrawTransaction"]
        guard plainEntities.union(transfers).contains(e.entity), let old else {
            throw Fail("only expenses, income, refunds and transfers can change amount")
        }
        guard old != 0 else { throw Fail("a row of 0 cannot be scaled to a new amount") }
        // The bank's amount is in the account's currency. A foreign row keeps
        // its original amount; its rate moves so the two still agree.
        if has(t, "originalAmount"), let orig = double(t.value(forKey: "originalAmount")) {
            if close(orig, old) {
                t.setValue(amount, forKey: "originalAmount")
            } else if has(t, "originalExchangeRate"), let rate = double(t.value(forKey: "originalExchangeRate")), orig != 0 {
                // The rate is derived; the bank's amount wins. Keep the
                // direction (amount = orig * rate, or orig / rate) that fits
                // the old amount best, even when MoneyWiz rounded it.
                let times = abs(rate * orig - old)
                let over = rate != 0 ? abs(orig / rate - old) : .infinity
                if times <= over {
                    t.setValue(amount / orig, forKey: "originalExchangeRate")
                } else {
                    t.setValue(orig / amount, forKey: "originalExchangeRate")
                }
            }
        }
        // Split parts scale with the row; the last part takes the rounding.
        let splits = try assignments(of: t, ctx)
        let parts = try splits.map { double(try get($0, "amount")) ?? 0 }
        let target = ((parts.reduce(0, +) * amount / old) * 100).rounded() / 100
        var given = 0.0
        for (i, s) in splits.enumerated() {
            let v = i == splits.count - 1 ? target - given : (parts[i] * amount / old * 100).rounded() / 100
            try set(s, "amount", v)
            given += v
        }
        // The other side of a transfer names this side's amount.
        if e.entity == "TransferWithdrawTransaction", let o = try get(t, "recipientTransaction") as? NSManagedObject,
           has(o, "originalSenderAmount"), close(double(o.value(forKey: "originalSenderAmount")), old) {
            o.setValue(amount, forKey: "originalSenderAmount")
        }
        if e.entity == "TransferDepositTransaction", let o = try get(t, "senderTransaction") as? NSManagedObject,
           has(o, "originalRecipientAmount"), close(double(o.value(forKey: "originalRecipientAmount")), old) {
            o.setValue(amount, forKey: "originalRecipientAmount")
        }
        try set(t, "amount", amount)
    }
    if let path = e.category {
        let target = try cats.require(path, user: user, kind: e.category_kind)
        let splits = try assignments(of: t, ctx)
        switch splits.count {
        case 0:
            let s = NSEntityDescription.insertNewObject(forEntityName: try assignmentEntity(ctx), into: ctx)
            if has(s, "GID") { s.setValue(newGID(), forKey: "GID") }
            if has(s, "objectCreationDate") { s.setValue(Date(), forKey: "objectCreationDate") }
            try set(s, "transaction", t)
            try set(s, "category", target)
            try set(s, "amount", try signedShare(of: double(t.value(forKey: "amount")) ?? 0, ctx))
        case 1:
            try set(splits[0], "category", target)
        default:
            throw Fail("a split row cannot change category in place")
        }
    }
    if let memo = e.memo { try set(t, "notes", memo) }
    if let day = e.date {
        // Each side of a transfer takes its own bank's day: the money leaves
        // one bank and lands in the other on the days each bank shows.
        let dated = plainEntities.union(["TransferDepositTransaction", "TransferWithdrawTransaction"])
        guard dated.contains(e.entity) else { throw Fail("only expenses, income, refunds and transfers can change date") }
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.timeZone = .current
        f.dateFormat = "yyyy-MM-dd"
        guard let start = f.date(from: day) else { throw Fail("date must be YYYY-MM-DD") }
        let cal = Calendar.current
        let into = date.timeIntervalSince(cal.startOfDay(for: date))
        try set(t, "date", start.addingTimeInterval(into))
    }
    if let name = e.payee {
        try requireKeys("Payee", ["GID", "name", "user"], ctx)
        let payees = try fetch("Payee", ctx, NSPredicate(format: "name ==[c] %@", name))
            .filter { same($0.value(forKey: "user") as? NSManagedObject, user) }
        guard payees.count <= 1 else { throw Fail("two payees have the same name; merge them in MoneyWiz first") }
        var payee = payees.first
        if payee == nil {
            let p = NSEntityDescription.insertNewObject(forEntityName: "Payee", into: ctx)
            try set(p, "GID", newGID())
            try set(p, "name", name)
            if has(p, "objectCreationDate") { p.setValue(Date(), forKey: "objectCreationDate") }
            try set(p, "user", user)
            payee = p
        }
        try set(t, "payee", payee)
    }
}

/// Moves a row to another account in the same currency. The other side of a
/// transfer then names the new account as its sender or recipient.
func move(_ t: NSManagedObject, _ entity: String, from: NSManagedObject?, to gid: String, _ ctx: NSManagedObjectContext) throws {
    try requireKeys("Account", ["GID", "currencyName"], ctx)
    let target = try one("Account", gid: gid, ctx, sub: true)
    guard let from else { throw Fail("a row to move has no account") }
    guard (from.value(forKey: "currencyName") as? String) == (target.value(forKey: "currencyName") as? String) else {
        throw Fail("a row moves only into an account in the same currency")
    }
    if has(t, "investmentHolding"), let h = t.value(forKey: "investmentHolding") as? NSManagedObject {
        // A forex account keeps its own currency on a holding: that row is cash.
        let symbol = ((h.value(forKey: "symbol") as? String) ?? "").trimmingCharacters(in: .whitespaces)
        guard symbol.caseInsensitiveCompare((target.value(forKey: "currencyName") as? String) ?? "") == .orderedSame else {
            throw Fail("an investment row cannot move to another account")
        }
        t.setValue(nil, forKey: "investmentHolding")
    }
    let link: (String, String)? = switch entity {
    case "TransferWithdrawTransaction": ("recipientTransaction", "senderAccount")
    case "TransferDepositTransaction": ("senderTransaction", "recipientAccount")
    default: nil
    }
    if let (other, side) = link, let o = try get(t, other) as? NSManagedObject {
        guard !same(try get(o, "account") as? NSManagedObject, target) else {
            throw Fail("a transfer cannot move into the account on its other side")
        }
        try set(o, side, target)
    }
    try set(t, "account", target)
}

/// The amount of a new single assignment, with the sign MoneyWiz uses for
/// assignments (read from an existing one).
func signedShare(of amount: Double, _ ctx: NSManagedObjectContext) throws -> Double {
    for s in try fetch(try assignmentEntity(ctx), ctx) {
        guard let part = double(s.value(forKey: "amount")), part != 0,
              let t = s.value(forKey: "transaction") as? NSManagedObject, has(t, "amount"),
              let whole = double(t.value(forKey: "amount")), whole != 0 else { continue }
        return (part > 0) == (whole > 0) ? amount : -amount
    }
    throw Fail("no existing category assignment to copy the sign from")
}

func apply(_ plan: Plan, _ ctx: NSManagedObjectContext) throws -> [String: Int] {
    guard plan.contract == 1 else { throw Fail("this writer reads plan contract 1, not \(plan.contract)") }
    let cats = try Categories(ctx)
    for op in plan.categories where op.op == "add" { try cats.add(op.path, like: op.like, kind: op.kind) }
    for op in plan.categories where op.op == "rename" {
        guard let to = op.to else { throw Fail("a rename has no new name") }
        try cats.rename(op.path, to: to)
    }
    // Holdings are checked before any change: a new trade moves its
    // holding's price, and the plan's market price is set after it.
    var holdings: [(NSManagedObject, HoldingEdit)] = []
    for h in plan.holdings ?? [] {
        try requireKeys("InvestmentHolding", ["GID", "openningNumberOfShares"], ctx)
        let holding = try one("InvestmentHolding", gid: h.gid, ctx)
        guard abs((double(try get(holding, "openningNumberOfShares")) ?? 0) - h.expect_opening) < 1e-9 else {
            throw Fail("a holding's opening shares changed since the plan was made")
        }
        if h.price != nil {
            guard abs((double(try get(holding, "pricePerShare")) ?? 0) - (h.expect_price ?? 0)) < 1e-6 else {
                throw Fail("a holding's price changed since the plan was made")
            }
        }
        holdings.append((holding, h))
    }
    var archives: [(NSManagedObject, ArchiveEdit)] = []
    for a in plan.archive ?? [] {
        try requireKeys("Account", ["GID", "archived"], ctx)
        let account = try one("Account", gid: a.gid, ctx, sub: true)
        guard ((try get(account, "archived") as? NSNumber)?.boolValue ?? false) == a.expect else {
            throw Fail("an account's archived flag changed since the plan was made")
        }
        archives.append((account, a))
    }
    for c in plan.clear ?? [] {
        try requireKeys(c.entity, ["GID", "status"], ctx)
        try set(try one(c.entity, gid: c.gid, ctx), "status", 2)
    }
    for e in plan.rows { try edit(e, ctx, cats) }
    for tr in plan.trades ?? [] { try addTrade(tr, ctx) }
    for (holding, h) in holdings {
        try set(holding, "openningNumberOfShares", h.opening)
        if let p = h.price { try set(holding, "pricePerShare", p) }
    }
    for a in plan.accounts {
        try requireKeys("Account", ["GID", "openingBalance"], ctx)
        let account = try one("Account", gid: a.gid, ctx, sub: true)
        guard close(double(try get(account, "openingBalance")), a.expect_opening) else {
            throw Fail("an opening balance changed since the plan was made")
        }
        try set(account, "openingBalance", a.opening)
    }
    for (account, a) in archives { try set(account, "archived", a.archived) }
    for op in plan.categories where op.op == "delete" { try cats.delete(op.path, moveTo: op.move_to) }
    let inserted = ctx.insertedObjects.count
    try ctx.obtainPermanentIDs(for: Array(ctx.insertedObjects))
    if ctx.hasChanges { try ctx.save() }
    return ["rows": plan.rows.count, "accounts": plan.accounts.count, "categories": plan.categories.count, "archive": (plan.archive ?? []).count, "cleared": (plan.clear ?? []).count, "inserted": inserted]
}

// MARK: - Commands

func schema(_ model: NSManagedObjectModel) -> [String: Any] {
    let wanted: (String) -> Bool = {
        $0 == "Category" || $0 == "Payee" || $0 == "InvestmentHolding" || $0.hasPrefix("CategoryAssig") || $0.hasSuffix("Transaction") || $0.hasSuffix("Account")
    }
    var out: [String: Any] = [:]
    for e in model.entities {
        guard let name = e.name, wanted(name) else { continue }
        out[name] = [
            "parent": e.superentity?.name ?? "",
            "attributes": e.attributesByName.mapValues { "\($0.attributeType.rawValue)\($0.isOptional ? "?" : "")" },
            "relationships": e.relationshipsByName.mapValues { "\($0.destinationEntity?.name ?? "")\($0.isOptional ? "?" : "")" },
        ]
    }
    return out
}

func printJSON(_ value: Any) throws {
    let data = try JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0A]))
}

func run() throws {
    let (command, values, flags) = arguments()
    switch command {
    case "checksum":
        let model = try loadModel(values["--model"])
        guard let store = values["--store"] else { throw Fail("--store is required") }
        try printJSON(["model": model.versionChecksum, "store": try storeChecksum(store)])
    case "schema":
        try printJSON(schema(try loadModel(values["--model"])))
    case "apply":
        guard let store = values["--store"], let planPath = values["--plan"], let allowed = values["--checksum"] else {
            throw Fail("apply needs --store, --model, --plan and --checksum")
        }
        let model = try loadModel(values["--model"])
        guard model.versionChecksum == allowed, try storeChecksum(store) == allowed else {
            throw Fail("the MoneyWiz model checksum is not the one allowed for writes (\(model.versionChecksum))")
        }
        if !flags.contains("--allow-running"), moneyWizRunning() {
            throw Fail("quit MoneyWiz before a write")
        }
        let plan = try JSONDecoder().decode(Plan.self, from: Data(contentsOf: URL(fileURLWithPath: planPath)))
        let container = try open(store: store, model: model)
        let ctx = container.newBackgroundContext()
        ctx.transactionAuthor = transactionAuthor
        var result: Result<[String: Int], Error> = .failure(Fail("the writer did not run"))
        ctx.performAndWait {
            do { result = .success(try apply(plan, ctx)) } catch {
                ctx.rollback()
                result = .failure(error)
            }
        }
        try printJSON(try result.get())
    default:
        throw Fail("usage: moneywiz-writer checksum|schema|apply ...")
    }
}

do {
    try run()
} catch {
    FileHandle.standardError.write(Data("error: \(error)\n".utf8))
    exit(2)
}

/// The other side of a coin transfer becomes a plain buy or sell with no
/// cash in its own account (same day, coin and count), then goes.
func unlinkOther(_ t: NSManagedObject, _ entity: String, _ ctx: NSManagedObjectContext) throws {
    let key = entity == "TransferWithdrawTransaction" ? "recipientTransaction" : "senderTransaction"
    guard entity.hasPrefix("Transfer"), let o = try get(t, key) as? NSManagedObject else {
        throw Fail("unlink needs a transfer with another side")
    }
    guard let acct = try get(o, "account") as? NSManagedObject, let gid = acct.value(forKey: "GID") as? String,
          let date = o.value(forKey: "date") as? Date else {
        throw Fail("unlink: the other side has no account or date")
    }
    let investment = ctx.persistentStoreCoordinator!.managedObjectModel.entitiesByName["InvestmentAccount"]!
    let held = o.value(forKey: "investmentHolding") as? NSManagedObject
    let heldSymbol = normSymbol((held?.value(forKey: "symbol") as? String) ?? "")
    let currency = normSymbol((acct.value(forKey: "currencyName") as? String) ?? "")
    if !acct.entity.isKindOf(entity: investment) || held == nil || heldSymbol == currency {
        // Money, not a coin: the account keeps a plain deposit or withdrawal
        // (on its own-currency holding, where a forex account has one).
        let plain = o.entity.name == "TransferWithdrawTransaction" ? "WithdrawTransaction" : "DepositTransaction"
        let p = NSEntityDescription.insertNewObject(forEntityName: plain, into: ctx)
        try set(p, "GID", newGID())
        if has(p, "objectCreationDate") { p.setValue(Date(), forKey: "objectCreationDate") }
        try set(p, "account", acct)
        try set(p, "date", date)
        try set(p, "amount", double(o.value(forKey: "amount")) ?? 0)
        for k in ["desc", "status", "reconciled"] where has(p, k) && has(o, k) { p.setValue(o.value(forKey: k), forKey: k) }
        if has(p, "notes") { p.setValue("Added by cashcast", forKey: "notes") }
        if let held, has(p, "investmentHolding") { p.setValue(held, forKey: "investmentHolding") }
        for s in try assignments(of: o, ctx) { ctx.delete(s) }
        ctx.delete(o)
        return
    }
    let holding = o.value(forKey: "investmentHolding") as? NSManagedObject
    let symbol = (holding?.value(forKey: "symbol") as? String) ?? (o.value(forKey: "symbol") as? String) ?? ""
    let shares = abs(double(o.value(forKey: "numberOfShares")) ?? 0)
    guard !symbol.isEmpty, shares > 0 else {
        throw Fail("unlink: the other side has no coin and count")
    }
    let sell = o.entity.name == "TransferWithdrawTransaction"
    try addTrade(TradeAdd(account_gid: gid, sell: sell, date: localDay(date), symbol: symbol,
                          shares: shares, price: 0, amount: 0), ctx)
    for s in try assignments(of: o, ctx) { ctx.delete(s) }
    ctx.delete(o)
}

/// Same fund name on both sides: case and runs of spaces ignored.
func normSymbol(_ s: String) -> String {
    s.split(whereSeparator: { $0.isWhitespace }).joined(separator: " ").uppercased()
}

/// Adds a buy or sell row to an investment account, on its holding (made
/// when the account has none for the symbol). The holding's price becomes
/// the trade's price when the trade is its newest.
func addTrade(_ tr: TradeAdd, _ ctx: NSManagedObjectContext) throws {
    let account = try one("InvestmentAccount", gid: tr.account_gid, ctx, sub: true)
    let f = DateFormatter()
    f.calendar = Calendar(identifier: .gregorian)
    f.timeZone = .current
    f.dateFormat = "yyyy-MM-dd"
    guard let day = f.date(from: tr.date) else { throw Fail("trade date must be YYYY-MM-DD") }
    let date = day.addingTimeInterval(12 * 3600)
    let holdings = (account.value(forKey: "investmentHoldings") as? Set<NSManagedObject>) ?? []
    var holding = holdings.first { normSymbol(($0.value(forKey: "symbol") as? String) ?? "") == normSymbol(tr.symbol) }
    if holding == nil {
        let h = NSEntityDescription.insertNewObject(forEntityName: "InvestmentHolding", into: ctx)
        try set(h, "GID", newGID())
        if has(h, "objectCreationDate") { h.setValue(Date(), forKey: "objectCreationDate") }
        try set(h, "symbol", tr.symbol)
        try set(h, "investmentAccount", account)
        try set(h, "pricePerShare", tr.price)
        holding = h
    }
    let entity = tr.sell ? "InvestmentSellTransaction" : "InvestmentBuyTransaction"
    let t = NSEntityDescription.insertNewObject(forEntityName: entity, into: ctx)
    try set(t, "GID", newGID())
    if has(t, "objectCreationDate") { t.setValue(Date(), forKey: "objectCreationDate") }
    try set(t, "account", account)
    try set(t, "date", date)
    try set(t, "symbol", tr.symbol)
    try set(t, "numberOfShares", tr.shares)
    try set(t, "pricePerShare", tr.price)
    try set(t, "amount", tr.amount)
    // Cleared, as the broker file records it: without a status MoneyWiz
    // lists the row as pending.
    if has(t, "status") { t.setValue(2, forKey: "status") }
    if has(t, "notes") { t.setValue("Added by cashcast", forKey: "notes") }
    try set(t, "investmentHolding", holding)
    let others = (holding?.value(forKey: "investmentTransactions") as? Set<NSManagedObject>) ?? []
    let newest = others.compactMap { $0.value(forKey: "date") as? Date }.allSatisfy { $0 <= date }
    if newest { try set(holding!, "pricePerShare", tr.price) }
}
