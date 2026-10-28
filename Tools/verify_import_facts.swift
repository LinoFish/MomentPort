// Compile together with MomentPort/Models.swift; never accesses a photo library.
import Foundation
@main struct VerifyImportFacts {
    static func main() {
        let cases: [(Int, [String?], Int, Bool)] = [
            (3, ["A", "A", "A"], 1, true),
            (3, ["A", nil, nil], 1, false), // previously falsely accepted
            (3, ["A", "", "A"], 1, false),
            (3, ["A", "A"], 1, false),
            (3, ["A", "B", "A"], 1, false),
            (3, ["A", "A", "A"], 0, false),
            (3, ["A", "A", "A"], 2, false),
            (1, ["A"], 1, false), (0, [], 0, false)
        ]
        for (count, ids, representatives, expected) in cases {
            precondition(BurstVerificationFacts(expectedCount: count, identifiers: ids,
                representativeCount: representatives).recognized == expected)
        }
        print("PASS: 9 import-verification cases; no Photos access")
    }
}
