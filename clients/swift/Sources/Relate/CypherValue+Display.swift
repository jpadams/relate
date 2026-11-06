import Foundation

// MARK: - Display

/// The pure renderers `description` composes from — file-scope so they are
/// not rebuilt as closures on every call: `description` recurses once per
/// list item and map value.

private func props(_ m: [String: CypherValue]) -> String {
    "{" + m.sorted { $0.key < $1.key }.map { "\($0.key): \($0.value)" }.joined(separator: ", ") + "}"
}

/// Pads a number to a fixed width. Sign-aware: pads the magnitude, so a
/// value of any width or sign (`-12345`, `12345`) never yields a negative
/// repeat count.
private func pad(_ v: Int64, _ width: Int) -> String {
    let digits = String(v.magnitude)
    return (v < 0 ? "-" : "") + String(repeating: "0", count: max(0, width - digits.count)) + digits
}

private func wall(_ h: Int64, _ m: Int64, _ s: Int64, _ ns: Int64) -> String {
    let nanos = ns == 0 ? "" : "." + pad(ns, 9)
    return "\(pad(h, 2)):\(pad(m, 2)):\(pad(s, 2))\(nanos)"
}

private func offset(_ seconds: Int64) -> String {
    let sign = seconds < 0 ? "-" : "+"
    let magnitude = Int64(seconds.magnitude)
    return "\(sign)\(pad(magnitude / 3600, 2)):\(pad((magnitude % 3600) / 60, 2))"
}

private func datePart(_ y: Int64, _ mo: Int64, _ d: Int64) -> String {
    "\(pad(y, 4))-\(pad(mo, 2))-\(pad(d, 2))"
}

private func rendered<T: LosslessStringConvertible>(_ values: [T]) -> String {
    values.map { String($0) }.joined(separator: ", ")
}

extension CypherValue: CustomStringConvertible {
    /// A Cypher-literal-like rendering for logs and debugging; not a parser
    /// round-trip format. Map keys print sorted.
    public var description: String {
        switch self {
        case .null: return "null"
        case .bool(let v): return String(v)
        case .int(let v): return String(v)
        case .float(let v): return String(v)
        case .string(let v): return "'" + v.replacingOccurrences(of: "'", with: "\\'") + "'"
        case .list(let items): return "[" + items.map(\.description).joined(separator: ", ") + "]"
        case .map(let m): return props(m)
        case .node(let n):
            let labels = n.labels.map { ":" + $0 }.joined()
            return "(\(n.elementId)\(labels) \(props(n.properties)))"
        case .relationship(let r):
            return "(\(r.startElementId))-[\(r.elementId):\(r.type) \(props(r.properties))]->(\(r.endElementId))"
        case .path(let p):
            // Walk nodes and relationships in path order; no fabricated
            // default for a first node — the struct's invariant
            // (nodes.count == relationships.count + 1) is what pairs them.
            var text = ""
            for (index, node) in p.nodes.enumerated() {
                text += "(\(node.elementId))"
                if index < p.relationships.count { text += "-[:\(p.relationships[index].type)]->" }
            }
            return text
        case .point(let p):
            var parts = ["srid: \(p.srid)", "x: \(p.x)", "y: \(p.y)"]
            if let z = p.z { parts.append("z: \(z)") }
            return "point({" + parts.joined(separator: ", ") + "})"
        case .date(let d):
            return datePart(d.year, d.month, d.day)
        case .time(let t):
            return wall(t.hour, t.minute, t.second, t.nanosecond) + offset(t.offsetSeconds)
        case .localTime(let t):
            return wall(t.hour, t.minute, t.second, t.nanosecond)
        case .dateTime(let d):
            var text = datePart(d.year, d.month, d.day) + "T"
                + wall(d.hour, d.minute, d.second, d.nanosecond) + offset(d.offsetSeconds)
            if let zone = d.zone { text += "[\(zone)]" }
            return text
        case .localDateTime(let d):
            return datePart(d.year, d.month, d.day) + "T"
                + wall(d.hour, d.minute, d.second, d.nanosecond)
        case .duration(let d):
            return "duration(months: \(d.months), days: \(d.days), seconds: \(d.seconds), nanoseconds: \(d.nanoseconds))"
        case .vector(let v):
            switch v {
            case .floats(let dtype, let values):
                return "vector<\(dtype.rawValue)>[\(rendered(values))]"
            case .ints(let dtype, let values):
                return "vector<\(dtype.rawValue)>[\(rendered(values))]"
            }
        }
    }
}
