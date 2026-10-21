#if os(iOS)
import SwiftUI
import Testing
import UIKit

@testable import PurePoint

@MainActor
struct ChannelComposerIOSTests {
    private func makeComposer(text: String, submits: @escaping () -> Void) -> (ComposerTextView, ChannelComposer.Coordinator) {
        let composer = ChannelComposer(text: .constant(text), placeholder: "Message channel", onSubmit: submits)
        let coordinator = composer.makeCoordinator()
        let view = ComposerTextView(usingTextLayoutManager: true)
        view.delegate = coordinator
        view.submit = submits
        view.text = text
        view.selectedRange = NSRange(location: (text as NSString).length, length: 0)
        return (view, coordinator)
    }

    @Test func returnSendsAndInsertsNothing() {
        var sent = 0
        let (view, coordinator) = makeComposer(text: "hello") { sent += 1 }
        let allowed = coordinator.textView(view, shouldChangeTextIn: NSRange(location: 5, length: 0), replacementText: "\n")
        #expect(!allowed)
        #expect(sent == 1)
        #expect(view.text == "hello")
    }

    @Test func shiftReturnInsertsANewlineWithoutSending() {
        var sent = 0
        let (view, _) = makeComposer(text: "hello") { sent += 1 }
        #expect(view.handleReturn(shift: true))
        #expect(sent == 0)
        #expect(view.text == "hello\n")
    }

    @Test func otherTextIsInsertedNormally() {
        let (view, coordinator) = makeComposer(text: "hi") {}
        #expect(coordinator.textView(view, shouldChangeTextIn: NSRange(location: 2, length: 0), replacementText: "!"))
        #expect(coordinator.textView(view, shouldChangeTextIn: NSRange(location: 2, length: 0), replacementText: "a\nb"))
    }

    @Test func mentionRangeEndsAtTheCaretAndStopsAtWhitespace() {
        let (view, _) = makeComposer(text: "hey @Al") {}
        #expect(view.rangeForUserCompletion == NSRange(location: 4, length: 3))
        view.text = "hey @Al ice"
        view.selectedRange = NSRange(location: 11, length: 0)
        #expect(view.rangeForUserCompletion.location == NSNotFound)
        view.text = "no mention"
        #expect(view.rangeForUserCompletion.location == NSNotFound)
    }

    @Test func completionsMatchMentionsCaseInsensitively() {
        let (view, _) = makeComposer(text: "") {}
        view.mentions = ["Alice", "alfred", "Bob"]
        #expect(view.completions(forPartialWord: "@al") == ["@Alice", "@alfred"])
        #expect(view.completions(forPartialWord: "al").isEmpty)
    }

    @Test func openCompletionTakesReturnAndInsertsTheMention() {
        var sent = 0
        let (view, _) = makeComposer(text: "hey @bo") { sent += 1 }
        view.mentions = ["Bob", "Bobby"]
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 800, height: 600))
        window.addSubview(view)
        view.frame = CGRect(x: 0, y: 500, width: 400, height: 60)

        view.complete()
        #expect(view.completionOpen)
        #expect(view.handleReturn(shift: false))
        #expect(sent == 0)
        #expect(view.text == "hey @Bob")
        #expect(!view.completionOpen)
        // With the list closed, Return sends again.
        #expect(view.handleReturn(shift: false))
        #expect(sent == 1)
    }

    @Test func completionClosesWhenNothingMatches() {
        let (view, _) = makeComposer(text: "hey @zz") {}
        view.mentions = ["Bob"]
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 800, height: 600))
        window.addSubview(view)
        view.complete()
        #expect(!view.completionOpen)
    }

    @Test func heightFollowsTheMacClamp() async {
        var reported: [CGFloat] = []
        let composer = ChannelComposer(
            text: .constant(""), placeholder: "", onHeightChanged: { reported.append($0) }, onSubmit: {})
        let coordinator = composer.makeCoordinator()
        let view = ComposerTextView(usingTextLayoutManager: true)
        view.font = .systemFont(ofSize: 13)
        view.textContainerInset = UIEdgeInsets(top: 8, left: 10, bottom: 8, right: 10)
        view.frame = CGRect(x: 0, y: 0, width: 400, height: 60)
        view.text = "one line"
        coordinator.updateHeight(view)
        view.text = String(repeating: "line\n", count: 40)
        coordinator.updateHeight(view)
        await Task.yield()
        try? await Task.sleep(for: .milliseconds(50))
        #expect(reported == [60, 180])
    }
}
#endif
