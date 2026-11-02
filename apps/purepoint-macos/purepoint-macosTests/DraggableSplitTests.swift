import SwiftUI
import Testing

@testable import PurePoint

struct DraggableSplitTests {
    typealias Split = DraggableSplit<EmptyView, EmptyView>

    @Test func bothSidesAndTheDividerFillTheSplit() {
        for total: CGFloat in [801, 1000, 1194] {
            for ratio: CGFloat in [0.1, 0.33, 0.5, 0.9] {
                let sizes = Split.sizes(total: total, ratio: ratio)
                #expect(sizes.first + Split.dividerThickness + sizes.second == total)
            }
        }
        #expect(Split.sizes(total: 1001, ratio: 0.5) == (500, 500))
    }

    @Test func aCollapsedSideDropsTheDivider() {
        #expect(Split.sizes(total: 800, ratio: 0) == (0, 800))
        #expect(Split.sizes(total: 800, ratio: 1) == (800, 0))
    }

    @Test func aTinySplitNeverGivesANegativeSize() {
        let sizes = Split.sizes(total: 0.5, ratio: 0.5)
        #expect(sizes.first >= 0 && sizes.second >= 0)
        #expect(Split.sizes(total: -10, ratio: 0.5) == (0, 0))
    }

    @Test func dragMovesTheRatioByTheTranslation() {
        #expect(abs(Split.ratio(start: 0.5, translation: 100, total: 1000) - 0.6) < 1e-9)
        #expect(abs(Split.ratio(start: 0.5, translation: -200, total: 1000) - 0.3) < 1e-9)
    }

    @Test func dragKeepsEachPaneAtItsMinimum() {
        let total: CGFloat = 1000
        let min = Split.minPaneSize / total
        #expect(Split.ratio(start: 0.5, translation: -900, total: total) == min)
        #expect(Split.ratio(start: 0.5, translation: 900, total: total) == 1 - min)
    }

    @Test func aSplitTooSmallForTwoMinimumsStaysCentred() {
        #expect(Split.ratio(start: 0.3, translation: 50, total: Split.minPaneSize * 1.5) == 0.5)
        #expect(Split.ratio(start: 0.3, translation: 50, total: 0) == 0.5)
    }

    @Test func theDividerHitAreaIsWideEnoughForAFingerOrPointer() {
        #expect(Split.dragInset * 2 >= 16)
    }
}
