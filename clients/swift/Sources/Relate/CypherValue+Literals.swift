// MARK: - Literals (parameters)

extension CypherValue: ExpressibleByNilLiteral, ExpressibleByBooleanLiteral,
    ExpressibleByIntegerLiteral, ExpressibleByFloatLiteral, ExpressibleByStringLiteral,
    ExpressibleByArrayLiteral, ExpressibleByDictionaryLiteral
{
    public init(nilLiteral: ()) { self = .null }
    public init(booleanLiteral value: Bool) { self = .bool(value) }
    public init(integerLiteral value: Int64) { self = .int(value) }
    public init(floatLiteral value: Double) { self = .float(value) }
    public init(stringLiteral value: String) { self = .string(value) }
    public init(arrayLiteral elements: CypherValue...) { self = .list(elements) }
    public init(dictionaryLiteral elements: (String, CypherValue)...) {
        self = .map(Dictionary(uniqueKeysWithValues: elements))
    }
}

extension CypherValue {
    public init(_ value: Int) { self = .int(Int64(value)) }
    public init(_ value: Double) { self = .float(value) }
    public init(_ value: String) { self = .string(value) }
    public init(_ value: Bool) { self = .bool(value) }
}
