import Foundation

@main struct PiTranscriptScrollChecks {
    @MainActor static func main() async {
        let follower = PiTranscriptScrollCoordinator()
        var scrolls = 0
        var allowed = true
        func request() {
            follower.request(canScroll: { allowed }, scroll: { scrolls += 1 })
        }
        func settle() async {
            // Queue behind the production DispatchQueue.main.async hop.
            await withCheckedContinuation { continuation in
                DispatchQueue.main.async { continuation.resume() }
            }
        }
        follower.update(contentHeight: 1000, viewportHeight: 400, visibleBottom: 400)
        for _ in 0..<20 { request() }
        precondition(scrolls == 0, "Layout callbacks must never scroll synchronously")
        await settle()
        precondition(scrolls == 1, "Burst requests must coalesce")
        request()
        await settle()
        precondition(scrolls == 1, "Unchanged geometry must not feed back")
        follower.update(contentHeight: 1100, viewportHeight: 400, visibleBottom: 400)
        request()
        await settle()
        precondition(scrolls == 2, "Late markdown/tool resizing must follow")
        follower.update(contentHeight: 1000, viewportHeight: 400, visibleBottom: 400)
        request()
        await settle()
        precondition(scrolls == 2, "Alternating lazy layout sizes must not feed back")
        follower.outputChanged()
        follower.update(contentHeight: 1100, viewportHeight: 400, visibleBottom: 1100)
        request()
        await settle()
        precondition(scrolls == 2, "Already at bottom must be a no-op")
        follower.update(contentHeight: 1200, viewportHeight: 400, visibleBottom: 400)
        request()
        allowed = false
        await settle()
        precondition(scrolls == 2, "Scrollback/browsing changes must be checked at execution")
        allowed = true
        request()
        await settle()
        precondition(scrolls == 3, "Latest must resume following")
        follower.outputChanged()
        request()
        follower.cancel()
        await settle()
        precondition(scrolls == 3, "Disappearing/session changes must cancel stale work")
        follower.reset()
        request()
        await settle()
        precondition(scrolls == 4, "New session/view must allow its initial scroll")
        follower.outputChanged()
        request()
        follower.reset()
        request()
        await settle()
        precondition(scrolls == 5, "Stale work must not consume a replacement request")
        follower.reset()
        follower.update(contentHeight: 1500, viewportHeight: 0, visibleBottom: 0)
        request()
        await settle()
        precondition(scrolls == 5, "A zero viewport must suppress pending scrolling")
        follower.update(contentHeight: 1500, viewportHeight: 400, visibleBottom: 400)
        request()
        await settle()
        precondition(scrolls == 6, "Valid geometry after a zero viewport must follow")
        // Latest intentionally retries a size that was already attempted before scrollback.
        follower.outputChanged()
        request()
        await settle()
        precondition(scrolls == 7, "Latest must clear prior attempted sizes")
        follower.reset()
        var feedbackScrolls = 0
        func feedbackRequest() {
            follower.request(canScroll: { true }, scroll: {
                feedbackScrolls += 1
                follower.update(contentHeight: feedbackScrolls.isMultiple(of: 2) ? 1600 : 1700,
                                viewportHeight: 400, visibleBottom: 400)
                feedbackRequest()
            })
        }
        follower.update(contentHeight: 1600, viewportHeight: 400, visibleBottom: 400)
        feedbackRequest()
        for _ in 0..<10 { await settle() }
        precondition(feedbackScrolls == 2, "Scroll-induced alternating geometry must terminate")
        print("Pi transcript scroll checks passed")
    }
}
