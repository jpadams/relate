package relate

// DecodeNotificationsForTest exposes the notification decoder to the
// external test package: the C ABI always returns a JSON array, so pinning
// the null/non-array rejection needs the decoder without a real ABI
// violation.
var DecodeNotificationsForTest = decodeNotifications

// DecodePlanForTest exposes the plan decoder to the external test package:
// the C ABI returns NULL or a JSON object, so pinning the null/non-object
// rejections needs the decoder without a real ABI violation.
var DecodePlanForTest = decodePlan

// DecodeQueryTypeForTest exposes the query-type decoder to the external
// test package: the C ABI always returns one of the four vocabulary
// values, so pinning the unknown-value rejection needs the decoder without
// a real ABI violation.
var DecodeQueryTypeForTest = decodeQueryType

// DecodeNamespaceForTest exposes the namespace decoder to the external
// test package: the C ABI always returns one of the three fixed names, so
// pinning the unknown-value rejection needs the decoder without a real
// ABI violation.
var DecodeNamespaceForTest = decodeNamespace

// DecodeErrorForTest exposes the error-shape decoder to the external test
// package: pinning the missing/mistyped-field rejections needs the decoder
// without a real ABI violation.
var DecodeErrorForTest = decodeError

// DecodeStatsForTest exposes the stats decoder to the external test
// package: the C ABI always returns the canonical 11 counters, so pinning
// the missing-key and non-object rejections needs the decoder without a
// real ABI violation.
var DecodeStatsForTest = decodeStats
