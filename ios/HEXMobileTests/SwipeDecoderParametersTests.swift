import XCTest
@testable import HEX

/// Swipe typing's settings come through the keyboard core's C interface (`HkSwipeParameters`,
/// filled by `hk_swipe_default_parameters`), so a field the header and the core's `repr(C)`
/// struct order differently would land in another field. Each default is checked by name
/// against the Swift decoder's own, from before the core read swipes (dev 2d52c720), as are
/// the contraction and own-word costs.
final class SwipeDecoderParametersTests: XCTestCase {
    func testTheCoresDefaultsAreTheSwiftDecodersFieldByField() {
        let parameters = SwipeDecoder.Parameters()
        let counts: [(String, KeyPath<SwipeDecoder.Parameters, Int>, Int)] = [
            ("samples", \.samples, 32),
            ("maxStartKeys", \.maxStartKeys, 4),
            ("maxEndKeys", \.maxEndKeys, 8),
            ("shortlist", \.shortlist, 80),
            ("prefilter", \.prefilter, 1500),
        ]
        let values: [(String, KeyPath<SwipeDecoder.Parameters, Double>, Double)] = [
            ("startRadius", \.startRadius, 1.6),
            ("endRadius", \.endRadius, 2.1),
            ("nearRadius", \.nearRadius, 1.5),
            ("minLengthRatio", \.minLengthRatio, 0.4),
            ("maxLengthRatio", \.maxLengthRatio, 1.8),
            ("startSigma", \.startSigma, 0.3),
            ("endSigma", \.endSigma, 0.3),
            ("startStraySigma", \.startStraySigma, 0.8),
            ("endStraySigma", \.endStraySigma, 0.9),
            ("stray", \.stray, 0.25),
            ("locationWeight", \.locationWeight, 30.0),
            ("shapeWeight", \.shapeWeight, 60.0),
            ("lmWeight", \.lmWeight, 1.0),
            ("band", \.band, 0.1),
            ("minimumLength", \.minimumLength, 0.6),
            ("contractionMissingPair", \.contractionMissingPair, 3.0),
            ("evidenceStep", \.evidenceStep, 0.1),
            ("turnWindow", \.turnWindow, 0.2),
            ("turnFrom", \.turnFrom, 35.0),
            ("turnFull", \.turnFull, 120.0),
            ("dwellWindow", \.dwellWindow, 0.1),
            ("dwellFrom", \.dwellFrom, 1.3),
            ("dwellFull", \.dwellFull, 4.0),
            ("evidenceFloor", \.evidenceFloor, 0.1),
            ("evidenceSpacing", \.evidenceSpacing, 0.8),
            ("evidenceGuard", \.evidenceGuard, 0.7),
            ("evidenceSigma", \.evidenceSigma, 0.35),
            ("evidenceReach", \.evidenceReach, 1.4),
            ("unexplainedCost", \.unexplainedCost, 6.0),
            ("straightKeyCost", \.straightKeyCost, 2.5),
            ("cornerKeyCost", \.cornerKeyCost, 2.0),
            ("straightAngle", \.straightAngle, 20.0),
        ]
        XCTAssertEqual(counts.count + values.count, Mirror(reflecting: parameters).children.count, "every field is checked")
        for (name, field, expected) in counts {
            XCTAssertEqual(parameters[keyPath: field], expected, name)
        }
        for (name, field, expected) in values {
            XCTAssertEqual(parameters[keyPath: field], expected, name)
        }
        XCTAssertEqual(SwipeDecoder.Parameters(parameters.core), parameters, "through the C struct and back")
    }

    func testContractionAndOwnWordCostsAreTheSwiftDecoders() {
        XCTAssertEqual(SwipeDecoder.contractionCost, 7.5)
        XCTAssertEqual(SwipeDecoder.ownWordCost, 11.0)
        XCTAssertEqual(SwipeDecoder.costOfContraction("I'm"), 6.0)
        XCTAssertEqual(SwipeDecoder.costOfContraction("Don't"), 6.0)
        XCTAssertEqual(SwipeDecoder.costOfContraction("we're"), 7.5)
        XCTAssertEqual(SwipeDecoder.costOfContraction("ma'am"), 9.5)
        XCTAssertEqual(SwipeDecoder.costOfContraction("don't\u{0}"), 9.5, "a NUL makes it no contraction, as in Swift")
        XCTAssertEqual(SwipeDecoder.costOfContraction("isn't", base: 1), 1)
    }
}
